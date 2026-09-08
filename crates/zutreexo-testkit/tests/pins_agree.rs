//! The `rustreexo` fork pin exists in two files, and they must not drift.
//!
//! `fuzz/` is deliberately outside the workspace — `cargo fuzz` builds with
//! `-Zsanitizer=address` and its own profile, and pulling it in would put those
//! flags on every ordinary build. The cost of that separation is a **second
//! copy of the `[patch.crates-io]` pin**, and a second copy is a second thing
//! to forget.
//!
//! It was forgotten. Bumping the workspace pin to the D33 fix
//! (`MemForest::deserialize` no longer panics on a bad node-type byte, no
//! longer overflows the stack on nested input) left `fuzz/Cargo.toml` at the
//! old `dc368cc`. All five committed crash artifacts still reproduced, against
//! a bug that had already been fixed, and the replay read as "the fix does not
//! work" rather than "you are testing the wrong code". The give-away was the
//! build path in the panic — `.../rustreexo-.../dc368cc/src/mem_forest/mod.rs`.
//!
//! This test is cheap and it is the guard on that. It compares the two revs
//! textually rather than resolving them, because the failure mode is a stale
//! literal, not a bad resolution.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

/// Repository root, from this crate's manifest directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root")
}

/// The `rev = "..."` on the `rustreexo` patch line in one manifest.
///
/// Parsed with a string search rather than a TOML crate: this test guards a
/// dependency pin, so it should not itself acquire a dependency that could
/// need pinning.
fn rustreexo_rev(manifest: &Path) -> String {
    let text = std::fs::read_to_string(manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("rustreexo = { git"))
        .unwrap_or_else(|| panic!("no rustreexo patch line in {}", manifest.display()));
    let at = line
        .find("rev = \"")
        .unwrap_or_else(|| panic!("no rev in {line}"));
    let rest = &line[at + 7..];
    let end = rest
        .find('"')
        .unwrap_or_else(|| panic!("unterminated rev in {line}"));
    rest[..end].to_owned()
}

#[test]
fn the_workspace_and_fuzz_crate_pin_the_same_rustreexo() {
    let root = repo_root();
    let workspace = rustreexo_rev(&root.join("Cargo.toml"));
    let fuzz = rustreexo_rev(&root.join("fuzz").join("Cargo.toml"));
    assert_eq!(
        workspace, fuzz,
        "\nthe fork pin has drifted between the two manifests:\n  \
         Cargo.toml       {workspace}\n  fuzz/Cargo.toml  {fuzz}\n\n\
         The fuzz targets would be exercising different code from the one the \
         workspace ships, and a crash artifact replayed against the wrong rev \
         reads as a failed fix rather than a stale pin."
    );
}

#[test]
fn both_pins_are_full_length_commit_hashes() {
    // An abbreviated rev resolves today and can become ambiguous later, and it
    // makes the two literals harder to compare by eye in review. Cargo accepts
    // a short rev, so nothing else would complain.
    for manifest in ["Cargo.toml", "fuzz/Cargo.toml"] {
        let rev = rustreexo_rev(&repo_root().join(manifest));
        assert_eq!(
            rev.len(),
            40,
            "{manifest} pins an abbreviated rev {rev:?}; use the full 40-character hash"
        );
        assert!(
            rev.chars().all(|c| c.is_ascii_hexdigit()),
            "{manifest} rev {rev:?} is not hex"
        );
    }
}
