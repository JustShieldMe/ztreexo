# Fuzz campaign, 2026-08-25

The 72-hour run recorded in `docs/design.md` D36: 206,659,449,674 executions,
zero crashes, **five of seven targets**. `forest_decode` and `snapshot_decode`
were excluded because both died within seconds on the upstream
`MemForest::deserialize` defects (D33); they became runnable at Phase 6d.

Kept because D36's budget conclusions are read off these logs — in particular
that `bundle_decode` was still finding new edges at 71.5 h, which is why its
budget is now 7 days with `-fork=8` while the others get 24 h.

Superseded by the campaign in the parent directory. Do not delete: the
saturation analysis in `scripts/fuzz_saturation.py` is only meaningful against
the run it was computed from.
