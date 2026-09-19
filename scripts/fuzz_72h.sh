#!/usr/bin/env bash
#
# Phase 6: the long fuzz run.
#
# # Budgets are per target, not one clock for all of them
#
# The 2026-08-22 run gave all five targets the same 72 hours. `docs/design.md`
# D36 records what that bought: 206 billion executions, 34 new edges, and 31 of
# them in `bundle_decode`. Three targets never reached an edge their seed corpus
# had not already reached, and `utxo_proof_decode` found its three at execution
# 101. Four of five cores spent three days confirming saturation.
#
# `bundle_decode` was the opposite problem. It found edges at 19.7 h, 66.3 h and
# **71.5 h** — the last one with twenty-five minutes left on the clock. It was
# still discovering when the run was cut off, so 72 h was too short for it and
# far too long for everything else, at the same time.
#
# Hence two budgets below. Note what the split is really saying: for a saturated
# target more hours is the wrong lever entirely, and its budget here buys crash
# coverage, not code coverage.
#
# D36 went on to say that widening a zero-gain target's coverage needs better
# seeds or a structured generator. D45 measured that claim and it was wrong for
# all three: `compact_state_decode`, `nonmembership_decode` and
# `utxo_proof_decode` already execute every reachable region of their decoders.
# They are finished, not stuck.
#
# # Which target gets the long slot (D45)
#
# The 2026-09-10 run gave the long slot to `bundle_decode`, the target D36 had
# seen still discovering. With eight forks it reached its ceiling in 2.1 hours
# and then ran for seven days. Meanwhile `snapshot_decode`, on one worker and a
# 24 h clock, found its last new edge at 18.1 h. That made it the only target
# cut off while still discovering, and the 10x rule gives it 7.5 days.
#
# So `snapshot_decode` gets the long slot and the forks now. Everything else
# gets 72 h. For each of them the 10x rule asks for less, but CLAUDE.md
# Phase 6's DoD ("fuzzers run 72 h clean") asks for 72, and the DoD sets the
# floor.
#
# Re-derive these numbers after any run — the script prints the analysis at the
# end, or run `scripts/fuzz_saturation.py` by hand against `fuzz-runs/`.
#
# # Which targets, and why not all seven
#
# All seven, as of 2026-09-08. `forest_decode` and `snapshot_decode` were
# excluded for four phases: both reach `MemForest::deserialize`, which panicked
# on a malformed node-type field and overflowed the stack on deeply nested
# input (`docs/design.md` D33). `UtxoForest::from_bytes` contained the panic
# with `catch_unwind` — but not the overflow, which aborts rather than unwinds —
# and libfuzzer-sys installs a panic hook that aborts before unwinding anyway,
# so under the fuzzer both died within seconds.
#
# The fork fix is now pushed and pinned in **both** manifests, and all five
# committed crash artifacts replay clean.
#
# Usage: nohup scripts/fuzz_72h.sh > fuzz-runs/driver.log 2>&1 &

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ZUTREEXO_FUZZ_OUT:-$REPO/fuzz-runs}"
RSS_MB="${ZUTREEXO_FUZZ_RSS_MB:-4096}"

# The one target still finding edges when its clock ran out (D45), so it gets
# days and the spare cores.
#
# `-fork=N` rather than `-jobs=N -workers=N`, for two measured reasons: `-jobs`
# writes each worker's output to `fuzz-<n>.log` in the *current directory*
# (dropping stray files in the repo root) and leaves the main log carrying only
# one worker's numbers, so the analysis at the end would silently under-report.
# Fork mode keeps one stream and merges the workers' corpora, which is the
# actual point of running them together.
#
# Measured on this box: 2 forks took `bundle_decode` from ~18k to ~40k exec/s,
# so throughput scales close to linearly. Coverage discovery does not —
# independent workers duplicate each other's exploration — so read this as
# "more executions", not "8x sooner to the next edge".
LONG_SECS="${ZUTREEXO_FUZZ_LONG_SECS:-604800}"    # 7 d
LONG_WORKERS="${ZUTREEXO_FUZZ_WORKERS:-8}"

# Saturated, or never unsaturated, and set by the Phase 6 DoD rather than by
# coverage. This is crash-hunting: new values through paths already covered.
SHORT_SECS="${ZUTREEXO_FUZZ_SHORT_SECS:-259200}"  # 72 h

# target:seconds:workers
TARGETS=(
  "snapshot_decode:$LONG_SECS:$LONG_WORKERS"
  # Reached its ceiling in 2.1 h on eight forks (D45); the rule says 21 h.
  "bundle_decode:$SHORT_SECS:1"
  "forest_decode:$SHORT_SECS:1"
  "wire_request_decode:$SHORT_SECS:1"
  # Every reachable region already covered (D45). These run for crashes only.
  "utxo_proof_decode:$SHORT_SECS:1"
  "compact_state_decode:$SHORT_SECS:1"
  "nonmembership_decode:$SHORT_SECS:1"
)

mkdir -p "$OUT"
cd "$REPO"
say() { echo "[$(date -Is)] $*"; }

# Artifacts from earlier runs stay in `fuzz/artifacts/` on purpose: they are
# regression seeds. So "artifacts in the directory" is not "crashes this run
# found". The 2026-09-10 run reported `artifacts=2` and `artifacts=3` for the
# five D33 files from August, which read as new crashes until checked (D45).
# Everything newer than this stamp is from this run.
STAMP="$OUT/.run-start"
touch "$STAMP"

say "building ${#TARGETS[@]} targets"
for spec in "${TARGETS[@]}"; do
  t="${spec%%:*}"
  rustup run nightly cargo fuzz build "$t" >> "$OUT/build.log" 2>&1 || {
    say "BUILD FAILED for $t — see $OUT/build.log"; exit 1; }
done
say "built"

for spec in "${TARGETS[@]}"; do
  IFS=: read -r t secs workers <<< "$spec"
  say "launching $t for ${secs}s with ${workers} worker(s)"
  parallel=()
  if [ "$workers" -gt 1 ]; then
    parallel=(-fork="$workers")
  fi
  nohup nice -n 10 rustup run nightly cargo fuzz run "$t" -- \
      -max_total_time="$secs" \
      -rss_limit_mb="$RSS_MB" \
      -print_final_stats=1 \
      "${parallel[@]}" \
    > "$OUT/$t.log" 2>&1 &
  echo "$!" > "$OUT/$t.pid"
done

say "all launched; waiting"
wait
say "all targets finished"

for spec in "${TARGETS[@]}"; do
  t="${spec%%:*}"
  # Counted with `find | wc -l`, never `ls | grep -c .`: that returns 1 when
  # the count is zero, so a `|| echo 0` fallback appends a *second* line and
  # the log reads "artifacts=0\n0".
  dir="$REPO/fuzz/artifacts/$t"
  new=$(find "$dir" -type f -newer "$STAMP" 2>/dev/null | wc -l)
  old=$(find "$dir" -type f ! -newer "$STAMP" 2>/dev/null | wc -l)
  # Sequential runs report a `stat::` block; fork mode does not, so fall back to
  # the running count on its last line.
  execs=$(grep -oE 'stat::number_of_executed_units: *[0-9]+' "$OUT/$t.log" | grep -oE '[0-9]+$' | tail -1)
  if [ -z "$execs" ]; then
    execs=$(grep -oE '^#[0-9]+:' "$OUT/$t.log" | tail -1 | tr -cd '0-9')
  fi
  say "$t: new_artifacts=$new (pre-existing $old) execs=${execs:-unknown}"
done

# The point of the run is the analysis, so do not make anyone remember to run
# it. Reads the logs just written; needs no flags once a run has finished.
say "saturation analysis"
python3 "$REPO/scripts/fuzz_saturation.py" --logs "$OUT" || true
