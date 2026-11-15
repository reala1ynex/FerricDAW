# Progress Ledger

**Last updated:** 2026-11-15  
**HEAD at prior handoff:** `e0c1af5` / ship `a79dc84` (v1.1.6)  
**Latest release:** v1.1.7; remaining coverage limits are recorded under I-05.

## v1.1.7 release verified

- Published https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.7 at tagged commit `3765857`.
- Downloaded release zip and feed independently: SHA-256 `7ef744c94350ecbd79333abcb844be55abdc857506b8249f1d3bc88db6fd9524` matches the feed; feed starts with `{` and has no BOM. `/latest` returns the same version and hash.
- Removed v1.1.6 release and tag after verification. GitHub release list and remote tags show v1.1.7 only.
- Visual coverage has limits: not every widget, tutorial action, or dirty-close Discard path was exercised. See I-05; do not describe those as verified.

## v1.1.7 implementation and verification history

- Final visual run on the rebuilt release: inspected the new bounce chrome, selected FLAC and WAV, loaded Afterglow, and exported a four-beat WAV (936,068 bytes; UI reported 3.2 s). Navigated all 11 Night Current pages, started a clean session, and verified Save before applying the chord step writes the project and continues. Earlier visual checks covered the other browser and dock surfaces. This does not establish every widget, every tutorial action, or the dirty-close Discard path; I-05 remains open.
- After the bounce chrome change, warning-denied workspace check, workspace tests, and Windows function-test (59/59) passed again. Release build succeeded in `target/ship`.

- Added drawbar organ and electric piano processors, instrument controls, factory presets, palette actions, and function-test cases.
- Removed per-sample exponential pitch calculations (I-16). Warning-denied workspace check, workspace tests, and function-test 59/59 passed on the v1.1.7 candidate; logs in `last-*.txt`. Release build succeeded.
- Release-build visual exploration has covered empty startup, hardware, browser and dock surfaces, tutorial entry, both new instrument insertions, XY movement, update and bounce dialogs, and dirty-close Cancel and Save (test project `visual-check-session-1.1.7.ferric`). This is partial: the complete tutorial, Afterglow, actual bounce, dirty-close Discard and exhaustive controls remain unverified.
- v1.1.7 is a local candidate only. Public release remains v1.1.6; do not publish until all mandated gates are satisfied.
- Subsequent review found I-17 in save-before-transition handling. The code now checks the actual save result and focused success/failure tests pass; rerun all three suites and rebuild/recheck the release binary because earlier results predate this fix.
- I-18 separates recovery-autosave state from unsaved-project state; its focused regression test passes.
- After I-18, all three mandated suites passed again: warning-denied check, workspace tests, and function test 59/59. A release rebuild compiled the candidate but failed at the final copy because an earlier v1.1.7 test instance had `target/release/ferricdaw.exe` open. Do not assume the top-level release executable includes I-17/I-18. The new release visual pass and publication remain pending.
- The alternate `target/ship` release build and Windows package now succeed without closing the open session (I-19). The v1.1.7 candidate launched to an empty session; local feed begins with `{`, has no BOM, and matches zip SHA-256 `cb9839c55ed1aae1da4a9f8ff91d9e68cc742e38b4b6f348f3ee5ede74722f04`. This is local package evidence, not verification of a remote release. A full final visual acceptance and publication remain open.

## Completed (prior sessions)

- [x] `/engineering` registries + **HANDOFF_CODEX.md**
- [x] P0/P1 repairs through v1.1.5
- [x] Pad A/B chrome pass (v1.1.6)
- [x] Published v1.1.6; only-current channel

## Historical assignment at start of v1.1.7

- [ ] Adversarial flaw review → ISSUE_REGISTRY → fix P0/P1 (+ solid P2s)
- [ ] Add more instruments (real DSP + palette + presets + function-test)
- [ ] Final end-user visual run (every surface; Pad A/B chrome bar)
- [ ] `cargo check -D warnings` + `cargo test --workspace` + FunctionTest-Windows.ps1
- [ ] Bump, publish GitHub release, delete prior tag, update registries

## Blockers / accepted limits

- I-05 egui every-widget hit-test (accepted unless Codex expands coverage)
- I-12 operator `GH_TOKEN` stale env (clear before `gh`)
- I-13 true ASIO (blocked by design for portable)
- I-14 Mac/Linux ship (out of scope)
