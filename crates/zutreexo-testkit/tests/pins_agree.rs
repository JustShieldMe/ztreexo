//! The workspace and the fuzz crate must resolve the same `rustreexo`.
//!
//! `fuzz/` is deliberately outside the workspace — `cargo fuzz` builds with
//! `-Zsanitizer=address` and its own profile, and pulling it in would put those
//! flags on every ordinary build. The cost of that separation is a **second
//! `Cargo.lock`**, and a second lockfile is a second thing to forget.
//!
//! It was forgotten once, when the dependency was still a fork pin. Bumping the
//! workspace pin to the D33 fix (`MemForest::deserialize` no longer panics on a
//! bad node-type byte, no longer overflows the stack on nested input) left
//! `fuzz/Cargo.toml` at the old `dc368cc`. All five committed crash artifacts
//! still reproduced, against a bug that had already been fixed, and the replay
//! read as "the fix does not work" rather than "you are testing the wrong
//! code". The give-away was the build path in the panic —
//! `.../rustreexo-.../dc368cc/src/mem_forest/mod.rs`.
//!
//! The fork is gone (D47: crates.io 0.7.0 carries every fix it did), but the
//! failure mode is not: `cargo update -p rustreexo` in one directory and not
//! the other does the same thing with a version number instead of a rev. So
//! this test compares what the two lockfiles actually resolved — version and
//! source — rather than what the manifests ask for.

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

/// The `(version, source)` of every `rustreexo` package in one lockfile.
///
/// Parsed with a string search rather than a TOML crate: this test guards a
/// dependency, so it should not itself acquire a dependency that could drift.
/// Lockfile entries are `[[package]]` blocks with `name`, `version` and
/// `source` on their own lines, in that order.
fn resolved_rustreexo(lockfile: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(lockfile)
        .unwrap_or_else(|e| panic!("read {}: {e}", lockfile.display()));
    text.split("[[package]]")
        .filter(|block| block.lines().any(|l| l.trim() == "name = \"rustreexo\""))
        .map(|block| {
            let field = |key: &str| {
                block
                    .lines()
                    .find_map(|l| l.trim().strip_prefix(key))
                    .map(|v| v.trim_matches('"').to_owned())
                    .unwrap_or_else(|| panic!("rustreexo entry without {key:?} in {block}"))
            };
            (field("version = "), field("source = "))
        })
        .collect()
}

#[test]
fn the_workspace_and_fuzz_crate_resolve_the_same_rustreexo() {
    let root = repo_root();
    let workspace = resolved_rustreexo(&root.join("Cargo.lock"));
    let fuzz = resolved_rustreexo(&root.join("fuzz").join("Cargo.lock"));
    assert_eq!(
        workspace.len(),
        1,
        "the workspace resolves {} copies of rustreexo: {workspace:?}",
        workspace.len()
    );
    assert_eq!(
        workspace, fuzz,
        "\nrustreexo has drifted between the two lockfiles:\n  \
         Cargo.lock       {workspace:?}\n  fuzz/Cargo.lock  {fuzz:?}\n\n\
         The fuzz targets would be exercising different code from the one the \
         workspace ships, and a crash artifact replayed against the wrong \
         version reads as a failed fix rather than a stale lockfile. Run \
         `cargo update -p rustreexo` in whichever directory is behind."
    );
}

#[test]
fn rustreexo_comes_from_crates_io() {
    // A git source here means someone has reintroduced a fork. That may be
    // right — it was for D10 and D33 — but it should be a decision recorded in
    // docs/design.md and deny.toml, not something a `[patch]` does quietly.
    for lockfile in ["Cargo.lock", "fuzz/Cargo.lock"] {
        for (version, source) in resolved_rustreexo(&repo_root().join(lockfile)) {
            assert!(
                source.starts_with("registry+https://github.com/rust-lang/crates.io-index"),
                "{lockfile} resolves rustreexo {version} from {source}; a fork \
                 needs a design note and a deny.toml allow-git entry"
            );
        }
    }
}
