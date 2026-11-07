# 2026-09-19 — aborted at 47 h by a reboot

Launched 2026-09-19 22:08:15 UTC with the budgets D45 corrected: 7 d on 8 forks
for `snapshot_decode`, 72 h for the other six. The box rebooted at
2026-09-21 21:10 UTC, about 47 hours in, so the driver never wrote its
per-target totals or its saturation analysis. The logs are kept because what
they do record is worth keeping:

- **Zero crashes.** No new artifacts; the five in `fuzz/artifacts/` are the
  August D33 files.
- **`snapshot_decode` reached 1,299 edges**, from 1,217 at launch and 1,097
  before the campaign that preceded this one, and was still climbing. Third
  independent confirmation that it was the starved target (D45).
- The three targets D45 called finished — `compact_state_decode`,
  `nonmembership_decode`, `utxo_proof_decode` — did not move one edge in 47
  hours, which is what D45 predicts.

`snapshot_decode.log` ends in null bytes: the file was extended but the writes
never reached disk before the reboot. Everything before the tail reads fine;
strip them with `tr -d '\0'`.

Corpora are not here — they live in `fuzz/corpus/` and carried forward into the
relaunch, which is where this run's real product went. `forest_decode` came out
of it with 2,311 inputs against 248, `bundle_decode` 1,489 against 313, and
`snapshot_decode` 1,384 against 536.
