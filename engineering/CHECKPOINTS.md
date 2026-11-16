# Development Checkpoints

## CP-2026-11-15-K — v1.1.12 track-header containment released

- **Commit / tag:** `4e3c754` / `v1.1.12`
- **Release:** `https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.12`
- **Changes:** Expanded the track header to keep the freeze/import controls inside the card; retained the quiet empty launcher region from v1.1.11.
- **Verification:** warning-denied workspace check, workspace tests, Windows function test 59/59, release feed/zip hash verification, and final visual loop across the default arrangement, Samples browser, and Presets/Plugins browser.
- **Channel:** v1.1.11 release/tag removed; v1.1.12 is the only current release.
- **Artifact SHA-256:** `df47ae3c82ec0f1afbb92b629bcf6439a06c83597db79d408b80e3d8a871f91b`

## CP-2026-11-15-F — v1.1.7 released

- **Tag:** v1.1.7 at `3765857`; release and zip are public.
- **Validation:** warning-denied check, workspace tests, 59/59 function test; release UI checked and a four-beat Afterglow WAV exported. See `PROGRESS.md` for visual limits.
- **Feed/zip SHA-256:** `7ef744c94350ecbd79333abcb844be55abdc857506b8249f1d3bc88db6fd9524`; downloaded assets and `/latest` verified.
- **Channel:** v1.1.6 release and tag removed after validation; v1.1.7 is the only release and version tag.

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
## CP-2026-11-15-G — v1.1.8 UI polish released

- **Commit:** `13f830c` / **Tag:** `v1.1.8`
- **Scope:** unified button/theme chrome, semantic accents, visual feedback pass, and theme invariant tests.
- **Verification:** warning-denied check, workspace tests, function-test 59/59, isolated release visual loop, package/feed hash verification.
- **Channel:** v1.1.7 release and tag removed; v1.1.8 is the only current release.

## CP-2026-11-15-H — v1.1.9 visual contrast follow-up released

- **Commit:** `7a46173` / **Tag:** `v1.1.9`
- **Release:** `https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.9`
- **Verification:** zero-warning workspace check, workspace tests, Windows function test 59/59, and final visual loop across browser, menu, dock, and arrangement surfaces.
- **Channel:** v1.1.8 release and tag removed; v1.1.9 is the only current release.
- **Artifact SHA-256:** `c7b53e51fc38a558f49bc3335400c3f20f0efb905824127da9806fb034eb076c`

## CP-2026-11-15-I — v1.1.10 arrangement cleanup released

- **Commit:** `728be8c` / **Tag:** `v1.1.10`
- **Release:** `https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.10`
- **Verification:** zero-warning workspace check, workspace tests, Windows function test 59/59, and final visual pass with unused launcher boxes removed.
- **Channel:** v1.1.9 release and tag removed; v1.1.10 is the only current release.
- **Artifact SHA-256:** `83f291631f15fa67bb0ac6299eacfa8f91af35d3088ba6b0bcd6681c55400b61`

## CP-2026-11-15-J — v1.1.11 flat launcher cleanup released

- **Commit:** `1859d32` / **Tag:** `v1.1.11`
- **Release:** `https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.11`
- **Verification:** zero-warning workspace check, workspace tests, Windows function test 59/59, and final visual pass with the empty launcher region flattened.
- **Channel:** v1.1.10 release and tag removed; v1.1.11 is the only current release.
- **Artifact SHA-256:** `a0dbc7149acefc30d3033befbe6d6275cd8665ebff05a0aa62c34241b290c7f4`

