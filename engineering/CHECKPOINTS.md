# Development Checkpoints

## CP-2026-11-15-A — Pre-finalization baseline

- **Stage:** Start of MASTER PROMPT finalization
- **Commit:** `8aa6ce4` (v1.1.4 published)
- **Tests:** `cargo test --workspace` PASS → `engineering/last-cargo-test.txt`
- **Outstanding:** I-01..I-15 open/partial as listed at recon
- **Next:** P0/P1 repairs + registries

## CP-2026-11-15-B — P0/P1 repair + engineering system (this session)

- **Stage:** Dirty-close, updater harden, autosave/MIDI banners, ASIO honesty, README 1.1.4, CI function-test, full `/engineering` set
- **Commit:** `4875ef9`
- **Branch:** `master`
- **Files:** `main.rs`, `update.rs`, `piano_roll.rs`, `browser.rs`, `audio_backend.rs`, `README.md`, `ci-release.yml`, `engineering/*`
- **Tests:** `cargo test --workspace` PASS; function-test **57/57** PASS (re-run after sleep recovery)
- **Completed reqs:** R-05, R-08(export UX), R-10 honesty, R-14 harden, R-18, R-19 CI, R-20
- **Outstanding:** I-05 (egui hit-test), I-12 (operator gh auth), I-13 (true ASIO), I-14 (Mac/Linux ship)
- **Next:** *(done — see CP-2026-11-15-C)*

## CP-2026-11-15-C — v1.1.5 ship

- **Stage:** Version bump, package, GitHub release, only-current cleanup
- **Commit:** `f14bcd8`
- **Tag:** `v1.1.5`
- **Branch:** `master` (pushed)
- **Assets:** `FerricDAW-v1.1.5-win-x64.zip`, `latest.json` (BOM-free, sha matches zip)
- **Tests:** packaged `ferricdaw.exe --function-test` **57/57 PASS**
- **Cleanup:** deleted GitHub release + tag `v1.1.4`
- **Readiness:** CONDITIONALLY READY (I-05/I-13/I-14 documented limits)
- **Next:** none required
