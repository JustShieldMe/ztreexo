//! Where the bytes inside a transparent proof actually go, and what a compact
//! encoding could recover.
//!
//! # Why this exists before any encoder does
//!
//! `docs/benchmarks.md` puts **65.3% of a compact node's proof bandwidth in
//! Utreexo inclusion proofs**, which reads like an invitation to compress them.
//! But that figure is the whole proof, and a proof is mostly 32-byte digests.
//! A digest is a hash: it does not compress, and no encoding recovers a byte of
//! it. The recoverable part is everything else — the `u64` length prefixes, the
//! `u64` target positions, and the one-byte tag `rustreexo` writes in front of
//! every hash.
//!
//! So the question this answers is not "can the encoding be tightened" — it can
//! — but **how much of the 65.3% is even addressable**. If digests are 95% of
//! it, the ceiling on this work is 5% of 65.3%, and that is worth knowing
//! before writing a codec, a version bump and a fuzz target rather than after.
//!
//! # What it measures
//!
//! Real proofs over real mainnet blocks, replayed from genesis so the forest
//! grows the way it actually grows. Target positions scale with forest size,
//! so a small or synthetic forest would understate their width and flatter the
//! varint candidate.
//!
//! Three candidates, cumulative:
//!
//! | candidate | change |
//! |---|---|
//! | `varint-len` | the three `u64` count prefixes become LEB128 |
//! | `+ varint-targets` | positions sorted, delta-coded, LEB128 |
//! | `+ tag-bitmap` | one bit per hash saying "full digest", instead of a byte |
//!
//! Each is reported as bytes saved against the current encoding, and against
//! the *bundle* total, which is the number that matters to a node operator.
//!
//! # Running it
//!
//! Needs a synced `zebrad` with RPC on 127.0.0.1:8232.
//!
//! ```text
//! ZUTREEXO_RPC=127.0.0.1:8232 ZUTREEXO_END=150000 \
//!   cargo run --release -p zutreexo-testkit --bin proof_encoding
//! ```
//!
//! 150,000 is the default because it is the range `docs/benchmarks.md` reports
//! the 65.3% over, so the two are directly comparable.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::time::Instant;

use zebra_chain::block::Block;
use zebra_chain::serialization::ZcashDeserialize;

use zutreexo_accumulator::imt::DEFAULT_DEPTH;
use zutreexo_accumulator::proof::encode_utxo_proof;
use zutreexo_accumulator::utreexo::ZcashNodeHash;
use zutreexo_accumulator::CanonicalSerialize;
use zutreexo_chain::{apply_and_prove, summarize_block, ApplyOptions, ChainAccumulators};
use zutreexo_testkit::source::{BlockStream, RpcSource};

fn env_u32(key: &str, fallback: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

/// Bytes a LEB128 encoding of `value` would occupy.
fn varint_len(value: u64) -> usize {
    let mut n = 1;
    let mut v = value >> 7;
    while v != 0 {
        n += 1;
        v >>= 7;
    }
    n
}

#[derive(Default)]
struct Totals {
    blocks: u64,
    bundle_bytes: u64,
    /// What `encode_utxo_proof` actually produced.
    proof_bytes: u64,
    /// Components of that, which must sum to it.
    version_bytes: u64,
    length_bytes: u64,
    target_bytes: u64,
    tag_bytes: u64,
    digest_bytes: u64,
    /// Counters.
    targets: u64,
    hashes: u64,
    full_digests: u64,
    /// Candidate sizes for the same proofs.
    cand_varint_len: u64,
    cand_varint_targets: u64,
    cand_tag_bitmap: u64,
    /// Sorted-target check: a delta encoding is only valid if they arrive
    /// sorted, so this counts the proofs where they do not.
    unsorted_proofs: u64,
}

fn main() {
    let rpc = std::env::var("ZUTREEXO_RPC").unwrap_or_else(|_| "127.0.0.1:8232".to_string());
    let end = env_u32("ZUTREEXO_END", 150_000);
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

    let mut t = Totals::default();
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

        let (_, bundle) = match apply_and_prove(&mut state, &summary, ApplyOptions::default()) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("height {height}: apply failed: {error}");
                std::process::exit(1);
            }
        };

        t.blocks += 1;
        t.bundle_bytes += bundle.to_bytes().len() as u64;

        let proof = &bundle.utxo_proof;
        let encoded = encode_utxo_proof(proof).len() as u64;
        t.proof_bytes += encoded;

        let inner = proof.inner();
        let targets = &inner.targets;
        let hashes = &inner.hashes;

        // Component split of the *current* encoding. `rustreexo` writes each
        // hash through `AccumulatorHash::write`: a one-byte tag, then 32 bytes
        // only when the hash is a real digest.
        let full: u64 = hashes
            .iter()
            .filter(|h| matches!(h, ZcashNodeHash::Some(_)))
            .count() as u64;
        t.version_bytes += 1;
        t.length_bytes += 16; // two u64 counts
        t.target_bytes += 8 * targets.len() as u64;
        t.tag_bytes += hashes.len() as u64;
        t.digest_bytes += 32 * full;
        t.targets += targets.len() as u64;
        t.hashes += hashes.len() as u64;
        t.full_digests += full;

        // Candidates, cumulative.
        let sorted = targets.windows(2).all(|w| match w {
            [a, b] => a < b,
            _ => true,
        });
        if !sorted && targets.len() > 1 {
            t.unsorted_proofs += 1;
        }
        let digests_and_tags = hashes.len() as u64 + 32 * full;
        let len_varint =
            (varint_len(targets.len() as u64) + varint_len(hashes.len() as u64)) as u64;

        // 1: varint length prefixes only.
        t.cand_varint_len += 1 + len_varint + 8 * targets.len() as u64 + digests_and_tags;

        // 2: and delta-coded varint targets. Falls back to absolute varints
        // when a proof's targets are not sorted, which is what an encoder
        // would have to do too.
        let target_varint: u64 = if sorted {
            let mut prev = 0u64;
            targets
                .iter()
                .map(|&x| {
                    let d = x.saturating_sub(prev);
                    prev = x;
                    varint_len(d) as u64
                })
                .sum()
        } else {
            targets.iter().map(|&x| varint_len(x) as u64).sum()
        };
        t.cand_varint_targets += 1 + len_varint + target_varint + digests_and_tags;

        // 3: and a presence bitmap instead of a tag byte per hash.
        let bitmap = (hashes.len() as u64).div_ceil(8);
        t.cand_tag_bitmap += 1 + len_varint + target_varint + bitmap + 32 * full;
    }

    let secs = began.elapsed().as_secs_f64();
    let pct = |part: u64, whole: u64| -> f64 {
        if whole == 0 {
            0.0
        } else {
            100.0 * part as f64 / whole as f64
        }
    };

    println!("blocks {} in {:.1}s", t.blocks, secs);
    println!("bundle bytes    {:>15}", t.bundle_bytes);
    println!(
        "utxo proof      {:>15}  {:.1}% of bundle",
        t.proof_bytes,
        pct(t.proof_bytes, t.bundle_bytes)
    );

    let summed = t.version_bytes + t.length_bytes + t.target_bytes + t.tag_bytes + t.digest_bytes;
    println!();
    println!("--- inside the proof bytes ---");
    println!("{:<16} {:>15} {:>8}", "component", "bytes", "share");
    for (name, bytes) in [
        ("digests", t.digest_bytes),
        ("target positions", t.target_bytes),
        ("hash tags", t.tag_bytes),
        ("length prefixes", t.length_bytes),
        ("version byte", t.version_bytes),
    ] {
        println!(
            "{:<16} {:>15} {:>7.1}%",
            name,
            bytes,
            pct(bytes, t.proof_bytes)
        );
    }
    if summed != t.proof_bytes {
        println!(
            "!! components sum to {summed}, encoder produced {} — the model is wrong",
            t.proof_bytes
        );
    }
    println!(
        "counts: {} targets, {} hashes ({} full digests, {} empty/placeholder)",
        t.targets,
        t.hashes,
        t.full_digests,
        t.hashes - t.full_digests
    );
    println!("proofs with unsorted targets: {}", t.unsorted_proofs);

    println!();
    println!("--- candidates ---");
    println!(
        "{:<22} {:>15} {:>10} {:>14}",
        "encoding", "bytes", "vs now", "vs bundle"
    );
    println!(
        "{:<22} {:>15} {:>9.1}% {:>13.1}%",
        "current", t.proof_bytes, 0.0, 0.0
    );
    for (name, bytes) in [
        ("varint lengths", t.cand_varint_len),
        ("+ varint targets", t.cand_varint_targets),
        ("+ tag bitmap", t.cand_tag_bitmap),
    ] {
        let saved = t.proof_bytes.saturating_sub(bytes);
        println!(
            "{:<22} {:>15} {:>9.1}% {:>13.1}%",
            name,
            bytes,
            -pct(saved, t.proof_bytes),
            -pct(saved, t.bundle_bytes)
        );
    }
    println!();
    println!(
        "irreducible (digests): {:.1}% of proof bytes — no encoding recovers these",
        pct(t.digest_bytes, t.proof_bytes)
    );
}
