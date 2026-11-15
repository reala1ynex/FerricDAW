# Progress Ledger

**Last updated:** 2026-11-15  
**HEAD:** `9ce2698` (v1.1.6 lock sync; ship commit `a79dc84`)  
**Active goal:** Handoff complete. No required open P0/P1 work.

## Completed

- [x] `/engineering` registries + checkpoints + **HANDOFF_CODEX.md**
- [x] P0/P1 repairs (dirty close, updater, banners, ASIO honesty, CI function-test)
- [x] Pad A/B chrome pass across browser + dock (v1.1.6)
- [x] `cargo test --workspace` + function-test **57/57** (pre-1.1.6 chrome ship)
- [x] Published **v1.1.6**; removed prior **v1.1.5** for only-current channel

## Next executable actions

1. None required. Optional: live visual QA of 1.1.6 chrome vs Pad A/B density.
2. If owner requests new work: load `engineering/HANDOFF_CODEX.md` first.

## Blockers / accepted limits

- I-05 egui every-widget hit-test (accepted)
- I-12 operator `GH_TOKEN` stale env (mitigated in publish script)
- I-13 true ASIO (blocked by design for portable)
- I-14 Mac/Linux ship (out of scope)
