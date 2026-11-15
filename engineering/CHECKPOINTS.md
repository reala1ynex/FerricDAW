# Development Checkpoints

## CP-2026-11-15-E — v1.1.7 candidate in progress

- **Base:** `bd4efdb` on `master`; candidate implementation commit `0914940` is local and not yet pushed.
- **Changes:** Drawbar Organ and Electric Piano with presets, MIDI DSP, UI and function tests; session-save safety fixes I-17/I-18.
- **Tests:** warning-denied workspace check, workspace tests, and Windows function test (59/59) pass on the candidate; logs in `engineering/last-*.txt`.
- **Visual:** partial release-build exploration completed; the rebuilt candidate launches to the empty session, but final verification after safety fixes remains open. The alternate `target/ship` release build and package succeed despite the running instance locking the default executable.
- **Release:** public v1.1.6 unchanged. Do not publish v1.1.7 until final candidate build and visual acceptance are complete; then verify feed/hash before removing the prior release and tag.

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
- **Next:** *(done — see CP-2026-11-15-D / E)*

## CP-2026-11-15-D — v1.1.6 UI chrome pass

- **Stage:** Pad A/B chrome across Samples, Presets, DNA, Launch, Mixer, FX, piano roll, routing
- **Commit / tag:** `a79dc84` / `v1.1.6`
- **Tests:** function-test 57/57 before publish
- **Cleanup:** deleted `v1.1.5` release/tag
- **Next:** Codex handoff doc

## CP-2026-11-15-E — Codex handoff

- **Stage:** Full Codex continuity package
- **Commit:** `31b76b8`
- **Artifact:** `engineering/HANDOFF_CODEX.md`
- **Registries:** PROGRESS / RELEASE_READINESS / ARCHITECTURE synced to 1.1.6
- **Next:** none required unless owner opens new work
