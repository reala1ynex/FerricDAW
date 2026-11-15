# Progress Ledger

**Last updated:** 2026-11-15  
**HEAD:** `f14bcd8` (v1.1.5)  
**Active goal:** Complete — v1.1.5 published; remaining items are accepted/blocked limits only.

## Completed

- [x] `/engineering` registries + checkpoints
- [x] P0/P1 repairs (dirty close, updater SHA/URL, autosave/MIDI banners, ASIO honesty, CI function-test, README)
- [x] `cargo test --workspace` PASS
- [x] Function-test 57/57 (debug + packaged release binary)
- [x] Bump/package/publish **v1.1.5**; delete prior v1.1.4 for only-current channel
- [x] Verify BOM-free `latest.json` + matching SHA-256

## Next executable actions

1. None required for release. Optional future: egui hit-test harness (I-05), ASIO SDK build flavor (I-13), Mac/Linux packaging (I-14).

## Blockers / accepted limits

- I-05 egui every-widget hit-test (accepted)
- I-12 operator env for `gh` if `GH_TOKEN` stale (mitigated in publish script)
- I-13 true ASIO (blocked by design for portable)
- I-14 Mac/Linux ship (out of scope)
