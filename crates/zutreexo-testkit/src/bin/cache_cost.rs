//! What a compact node's leaf cache is actually worth, in bytes.
//!
//! # The question this closes
//!
//! `proof_encoding` found **94.1% of transparent proof bytes are digests**, so
//! tightening the framing is worth ~2% of a bundle and the only real lever is
//! needing fewer digests. Utreexo's answer is a cache: a node that still holds
//! a leaf proves the spend itself, and the bridge omits that target from the
//! proof entirely.
//!
//! `spend_age` priced the *coverage* of such a cache — what share of spends are
//! young enough to be in it. Coverage is not bytes. Utreexo proofs are batched,
//! so targets share internal nodes, and removing half the targets from a proof
//! removes much less than half its hashes: the survivors still need most of the
//! same path to the root. Whether the cache pays is therefore a question
//! coverage cannot answer, and this answers it by construction — for each
//! block, build the proof the bridge would actually send when the node
//! remembers everything younger than `W`, and encode it.
//!
//! Three sizes per block per window:
//!
//! | quantity | meaning |
//! |---|---|
//! | `full` | the proof as sent today, all spends |
//! | `uncached` | the proof over only the spends the node cannot prove itself |
//! | `saved` | the difference, which is what the cache buys |
//!
//! The cache's cost is reported too, as the peak number of leaves held, because
//! a window that saves 40% while holding ten million leaves has not made a
//! compact node compact.
//!
//! # Running it
//!
//! ```text
//! ZUTREEXO_RPC=127.0.0.1:8232 ZUTREEXO_END=150000 ZUTREEXO_WARMUP=100000 \
//!   cargo run --release -p zutreexo-testkit --bin cache_cost
//! ```
//!
//! The warm-up exists for the reason `spend_age` documents: a short chain
//! cannot produce an old spend, so windows measured without one are flattered.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::collections::HashMap;
use std::time::Instant;

use zebra_chain::block::Block;
use zebra_chain::serialization::ZcashDeserialize;

use zutreexo_accumulator::imt::DEFAULT_DEPTH;
use zutreexo_accumulator::proof::encode_utxo_proof;
use zutreexo_accumulator::UtxoLeaf;
use zutreexo_chain::{apply_and_prove, summarize_block, ApplyOptions, ChainAccumulators, OutPoint};
use zutreexo_testkit::source::{BlockStream, RpcSource};

fn env_u32(key: &str, fallback: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

/// Cache windows to price, in blocks of history remembered.
const WINDOWS: [u32; 5] = [100, 1_000, 10_000, 50_000, 100_000];

#[derive(Default, Clone)]
struct Window {
    /// Encoded proof bytes the bridge would still have to send.
    uncached_bytes: u64,
    /// Spends the cache covered, so the bridge omitted them.
    covered: u64,
    /// Peak leaves held by the cache over the *whole replay*, genesis onward.
    ///
    /// Kept for comparison, and not the number to size a cache by. For a short
    /// window the all-time peak lands in the early chain, where blocks carried
    /// far more transparent outputs than they do now: the 10,000-block row
    /// reported 2,608,783 at heights 0-150,000 and the identical figure over
    /// the full chain, because both runs replay the same early peak and nothing
    /// later exceeds it.
    peak_leaves_all: u64,
    /// Peak leaves held while measuring, i.e. at or above the warm-up height.
    /// This is what a node running in the measured era would need.
    peak_leaves_window: u64,
    /// Sum and count of occupancy samples while measuring, for the mean.
    occupancy_sum: u64,
    occupancy_samples: u64,
    /// Blocks where the cache covered every spend, so no proof at all is sent.
    blocks_fully_covered: u64,
}

fn main() {
    let rpc = std::env::var("ZUTREEXO_RPC").unwrap_or_else(|_| "127.0.0.1:8232".to_string());
    let end = env_u32("ZUTREEXO_END", 150_000);
    let warmup = env_u32("ZUTREEXO_WARMUP", 0);
    let depth =
        u8::try_from(env_u32("ZUTREEXO_DEPTH", u32::from(DEFAULT_DEPTH))).unwrap_or(DEFAULT_DEPTH);

    let source = RpcSource::new(&rpc);
    let mut state = match ChainAccumulators::new(depth) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("bad depth {depth}: {error}");
            std::process::exit(2);
        }
    };

    // Creation height per live outpoint, so a spend's age is known when it
    // happens. Not hashed, ordered or serialized, so a HashMap is fine.
    let mut born: HashMap<OutPoint, u32> = HashMap::new();
    let mut windows = vec![Window::default(); WINDOWS.len()];
    let mut full_bytes: u64 = 0;
    let mut spends: u64 = 0;
    let mut blocks: u64 = 0;
    let began = Instant::now();

    for (height, fetched) in BlockStream::new(&source, 0, end, 256, 8) {
        let raw = match fetched {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("height {height}: fetch failed: {error}");
                std::process::exit(1);
            }
        };
        let block = match Block::zcash_deserialize(&raw[..]) {
            Ok(block) => block,
            Err(error) => {
                eprintln!("height {height}: parse failed: {error}");
                std::process::exit(1);
            }
        };
        let summary = match summarize_block(&block) {
            Ok(summary) => summary,
            Err(error) => {
                eprintln!("height {height}: summarize failed: {error}");
                std::process::exit(1);
            }
        };
        let now = summary.height;
        let counts = now >= warmup;

        // Ages, and the per-window split of this block's spends. Done before
        // `apply_and_prove`, which deletes the spent leaves: a leaf that is
        // gone cannot be proved, and measuring afterwards silently reports
        // nothing (the mistake csn_replay documents).
        if counts && !summary.transparent_spends.is_empty() {
            let mut old_leaves: Vec<Vec<UtxoLeaf>> = vec![Vec::new(); WINDOWS.len()];
            let mut covered_here = vec![0u64; WINDOWS.len()];
            for point in &summary.transparent_spends {
                let leaf = match state.utxo(point) {
                    Some(leaf) => leaf.clone(),
                    None => continue,
                };
                let age = born.get(point).map(|b| now.saturating_sub(*b));
                for ((w, hit), olds) in WINDOWS
                    .iter()
                    .zip(covered_here.iter_mut())
                    .zip(old_leaves.iter_mut())
                {
                    match age {
                        // Young enough that the node still holds the leaf.
                        Some(a) if a <= *w => *hit += 1,
                        _ => olds.push(leaf.clone()),
                    }
                }
            }

            for ((win, leaves), hit) in windows
                .iter_mut()
                .zip(old_leaves.iter())
                .zip(covered_here.iter())
            {
                win.covered += *hit;
                if leaves.is_empty() {
                    if *hit > 0 {
                        win.blocks_fully_covered += 1;
                    }
                    continue;
                }
                let hashes: Vec<[u8; 32]> = leaves.iter().map(UtxoLeaf::hash).collect();
                match state.utxos().prove(&hashes) {
                    Ok(proof) => {
                        win.uncached_bytes += encode_utxo_proof(&proof).len() as u64;
                    }
                    Err(error) => {
                        eprintln!("height {now}: subset proof failed: {error}");
                        std::process::exit(1);
                    }
                }
            }
            spends += summary.transparent_spends.len() as u64;
            blocks += 1;
        }

        for (point, _) in &summary.transparent_creates {
            born.insert(*point, now);
        }

        let (_, bundle) = match apply_and_prove(&mut state, &summary, ApplyOptions::default()) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("height {now}: apply failed: {error}");
                std::process::exit(1);
            }
        };
        if counts {
            full_bytes += encode_utxo_proof(&bundle.utxo_proof).len() as u64;
        }

        for point in &summary.transparent_spends {
            born.remove(point);
        }

        // Progress, to stderr so it never lands in a redirected result file.
        // This run takes most of a day over the full chain, and a tool that
        // prints only on completion cannot be distinguished from one that has
        // wedged — which has already cost this project two multi-hour waits.
        // The two phases have very different rates, so the line names which
        // one is running.
        if now % 25_000 == 0 {
            let phase = if counts { "measuring" } else { "replaying" };
            eprintln!(
                "[{:>7.1}s] {phase} height {now} — {} live utxos, {:.0} blocks/s",
                began.elapsed().as_secs_f64(),
                born.len(),
                f64::from(now).max(1.0) / began.elapsed().as_secs_f64().max(0.001),
            );
        }

        // Cache occupancy: leaves created within each window that are still
        // unspent. Sampled rather than recomputed per block, because counting
        // exactly would mean walking the whole map every block.
        if now % 1_000 == 0 {
            for (win, w) in windows.iter_mut().zip(WINDOWS.iter()) {
                let held = born
                    .values()
                    .filter(|b| now.saturating_sub(**b) <= *w)
                    .count() as u64;
                win.peak_leaves_all = win.peak_leaves_all.max(held);
                if counts {
                    win.peak_leaves_window = win.peak_leaves_window.max(held);
                    win.occupancy_sum += held;
                    win.occupancy_samples += 1;
                }
            }
        }
    }

    let secs = began.elapsed().as_secs_f64();
    let pct = |part: u64, whole: u64| -> f64 {
        if whole == 0 {
            0.0
        } else {
            100.0 * part as f64 / whole as f64
        }
    };

    println!("blocks counted {blocks} of 0..={end} in {secs:.1}s");
    if warmup > 0 {
        println!("counted from height {warmup} (state built from genesis)");
    }
    println!("transparent spends {spends}");
    println!("proof bytes today  {full_bytes}");
    println!();
    println!(
        "{:<12} {:>12} {:>14} {:>9} {:>12} {:>12} {:>12} {:>10}",
        "window",
        "spends hit",
        "bytes sent",
        "saved",
        "leaves mean",
        "leaves peak",
        "all-time pk",
        "no-proof"
    );
    for (win, w) in windows.iter().zip(WINDOWS.iter()) {
        let saved = full_bytes.saturating_sub(win.uncached_bytes);
        let mean = win
            .occupancy_sum
            .checked_div(win.occupancy_samples)
            .unwrap_or(0);
        println!(
            "{:<12} {:>11.1}% {:>14} {:>8.1}% {:>12} {:>12} {:>12} {:>10}",
            format!("{w} blocks"),
            pct(win.covered, spends),
            win.uncached_bytes,
            pct(saved, full_bytes),
            mean,
            win.peak_leaves_window,
            win.peak_leaves_all,
            win.blocks_fully_covered
        );
    }
    println!();
    println!("'saved' is bytes off the wire. 'leaves mean' and 'leaves peak' are what a node");
    println!("in the measured era holds to get it; 'all-time pk' includes the replay from");
    println!("genesis and is shown only so the two can be compared, not to size a cache by.");
}
