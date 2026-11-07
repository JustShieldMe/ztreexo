# 2026-09-23 — the campaign that met the Phase 6 fuzzing DoD

Launched 2026-09-23 03:23 UTC with D45's budgets: 7 days on 8 forks for
`snapshot_decode`, 72 hours on one worker for the other six. **87.3 billion
executions, zero crashes, every target at or past 72 hours continuous.**

**The result is `final-analysis.txt`, not the tail of `driver.log`.** Read the
note below before reading the driver log.

| target | edges | last new edge | continuous run |
|---|---|---|---|
| `snapshot_decode` | 1060 -> 1304 (+244) | 3.5 days in | 144 h (519,819 s) |
| `forest_decode` | 308 -> 313 (+5) | 11 min | 72 h |
| `bundle_decode` | 815 | never | 72 h |
| `wire_request_decode` | 187 | never | 72 h |
| `utxo_proof_decode` | 184 | never | 72 h |
| `compact_state_decode` | 301 | never | 72 h |
| `nonmembership_decode` | 323 | never | 72 h |

Both of D45's predictions held. The five targets it called *finished* gained no
edges in tens of billions of further executions. `snapshot_decode`, which it
called *starved*, gained 244 and was still finding code three and a half days
in — so the 24 h it was given two campaigns earlier stopped it roughly a quarter
of the way through.

## Why `driver.log` is misleading after line 10

On 2026-09-24 a `pkill` meant for a test run matched `snapshot_decode`'s
fork-mode children, because libFuzzer had given them a `-max_total_time` of
exactly 300, the value the filter was looking for. That killed
`snapshot_decode` and the process-group wrappers the driver was `wait`ing on,
so `wait` returned at 03:00:23 and the driver wrote its end-of-run summary
**while the six other targets were still running**. Lines 11 onward are that
premature summary: `execs=unknown` for every target and a saturation analysis
of a run one day into three.

`snapshot_decode` was relaunched by hand with its remaining 519,782 s and its
output appended to `snapshot_decode.log`, so that log has two segments. The
six other targets were never interrupted. `final-analysis.txt` is
`scripts/fuzz_saturation.py` run against these logs after everything had
exited, and is the campaign's actual result.

The `pkill` mistake, and a second one like it — a queued job whose
`pgrep -f` matched its own parent shell and waited a week — are why
`pgrep -x <name>` is the only safe way to find a fuzz process on this box.
