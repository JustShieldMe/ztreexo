//! How old is a transparent output when it is spent?
//!
//! # Why this is the question
//!
//! `proof_encoding` measured where a transparent proof's bytes go and found
//! **88.3% of them are 32-byte digests**, which no encoding recovers. Tightening
//! the framing around them is worth about 3% of a bundle. To do better, a node
//! has to need *fewer digests*, not smaller ones.
//!
//! Utreexo's own answer is a cache. A node that still remembers a leaf does not
//! need a proof for it, so every spend of a recently-created output is free —
//! the bridge omits the proof and the node proves it from what it kept. The
//! saving is therefore set by one property of the chain that nobody here has
//! measured: **the age distribution of transparent spends.**
//!
//! Bitcoin's Utreexo work found this distribution heavily skewed toward young
//! outputs, which is what makes the cache pay there. Zcash's transparent
//! traffic is a different population — exchange flows, mining payouts, and a
//! long sandblasting era — so the number has to be measured rather than
//! inherited. That is all this does.
//!
//! It deliberately reports *counts*, not bytes. Proof bytes are shared between
//! targets inside a batched proof, so "bytes attributable to young spends" is
//! not a well-defined quantity until you re-derive the proof without them.
//! That is the follow-up measurement; this one decides whether it is worth
//! writing.
//!
//! # Running it
//!
//! ```text
//! ZUTREEXO_RPC=127.0.0.1:8232 ZUTREEXO_END=150000 ZUTREEXO_WARMUP=50000 \
//!   cargo run --release -p zutreexo-testkit --bin spend_age
//! ```
//!
//! # The warm-up matters, and leaving it out reads as a result
//!
//! A chain `H` blocks long cannot show a spend older than `H`, so a run over
//! the first 20,000 blocks reports "97.6% of spends are younger than 10,000
//! blocks" no matter what the chain is really like — there is nowhere for an
//! older output to have come from. `ZUTREEXO_WARMUP` fixes that: creates are
//! tracked from genesis, but a spend is only counted once the chain is at least
//! that tall, so every window well below the warm-up height is measured against
//! a population that could have been older and was not.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::collections::HashMap;
use std::time::Instant;

use zebra_chain::block::Block;
use zebra_chain::serialization::ZcashDeserialize;

use zutreexo_chain::{summarize_block, OutPoint};
use zutreexo_testkit::source::{BlockStream, RpcSource};

fn env_u32(key: &str, fallback: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

/// Cache sizes to price, in blocks of history remembered.
const WINDOWS: [u32; 7] = [1, 6, 100, 1_000, 10_000, 50_000, 100_000];

fn main() {
    let rpc = std::env::var("ZUTREEXO_RPC").unwrap_or_else(|_| "127.0.0.1:8232".to_string());
    let end = env_u32("ZUTREEXO_END", 150_000);
    // Ages are only recorded above this height; see the module comment.
    let warmup = env_u32("ZUTREEXO_WARMUP", 0);
    let source = RpcSource::new(&rpc);

    // Creation height per outpoint. A HashMap is fine here and nowhere near a
    // root: nothing in this binary is hashed, ordered, or serialized, so
    // CLAUDE.md's determinism rule is not in play. Entries are removed when
    // spent, so this tracks the live UTXO set rather than all history.
    let mut created: HashMap<OutPoint, u32> = HashMap::new();

    let mut ages: Vec<u64> = Vec::new(); // index = age bucket (log2), value = count
    let mut spends: u64 = 0;
    let mut unresolved: u64 = 0;
    let mut within = [0u64; WINDOWS.len()];
    let mut age_sum: u128 = 0;
    let mut age_max: u32 = 0;
    let mut same_block: u64 = 0;
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

        // Creates first, so an output spent in the block that made it counts as
        // age 0 rather than as unresolved. Those are real: a chain of
        // transactions inside one block spends its own outputs.
        for (point, _) in &summary.transparent_creates {
            created.insert(*point, summary.height);
        }

        for point in &summary.transparent_spends {
            let counts = summary.height >= warmup;
            if counts {
                spends += 1;
            }
            match created.remove(point) {
                Some(birth) if counts => {
                    let age = summary.height.saturating_sub(birth);
                    if age == 0 {
                        same_block += 1;
                    }
                    age_sum += u128::from(age);
                    age_max = age_max.max(age);
                    for (slot, w) in within.iter_mut().zip(WINDOWS.iter()) {
                        if age <= *w {
                            *slot += 1;
                        }
                    }
                    let bucket = usize::try_from(64 - u64::from(age).leading_zeros()).unwrap_or(0);
                    if ages.len() <= bucket {
                        ages.resize(bucket.saturating_add(1), 0);
                    }
                    if let Some(slot) = ages.get_mut(bucket) {
                        *slot += 1;
                    }
                }
                Some(_) => {}
                None if counts => unresolved += 1,
                None => {}
            }
        }
    }

    let secs = began.elapsed().as_secs_f64();
    let pct = |part: u64| -> f64 {
        if spends == 0 {
            0.0
        } else {
            100.0 * part as f64 / spends as f64
        }
    };

    println!("blocks 0..={end} in {secs:.1}s");
    if warmup > 0 {
        println!("ages counted from height {warmup} (creates tracked from genesis)");
    }
    println!("transparent spends: {spends}");
    if unresolved > 0 {
        println!("unresolved (created before genesis of this run): {unresolved}");
    }
    println!("live utxos at end:  {}", created.len());
    println!(
        "mean age {:.0} blocks, max {age_max}, spent in the same block {} ({:.1}%)",
        if spends > 0 {
            age_sum as f64 / spends as f64
        } else {
            0.0
        },
        same_block,
        pct(same_block)
    );

    println!();
    println!("--- what a cache of the last N blocks would cover ---");
    println!("{:<12} {:>14} {:>9}", "window", "spends", "share");
    for (count, w) in within.iter().zip(WINDOWS.iter()) {
        println!(
            "{:<12} {:>14} {:>8.1}%",
            format!("{w} blocks"),
            count,
            pct(*count)
        );
    }

    println!();
    println!("--- age distribution (log2 buckets) ---");
    for (bucket, count) in ages.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        let lo = if bucket == 0 {
            0
        } else {
            1u64 << bucket.saturating_sub(1)
        };
        let hi = (1u64 << bucket).saturating_sub(1);
        println!("{:>10}..{:<10} {:>12} {:>7.1}%", lo, hi, count, pct(*count));
    }
}
