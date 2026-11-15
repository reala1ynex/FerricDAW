# Issue Registry

**Last updated:** 2026-11-15

| ID | Sev | Subsystem | Description | Root cause | Status | Tests | Evidence |
|----|-----|-----------|-------------|------------|--------|-------|----------|
| I-01 | P0 | App/UI | Window close discarded dirty session without confirm | Close path set quit without CancelClose + modal | **Resolved** | Manual + code path | `PendingDestructive::QuitApp`; CancelClose when `session_has_user_work` |
| I-02 | P0 | Updater | Missing SHA / arbitrary URL could apply untrusted zip | Optional sha256; no host allowlist | **Resolved** | `update::tests::reject_non_github_url_and_missing_sha` | `validate_feed` + `feed_url_is_allowed` |
| I-03 | P1 | Audio | ASIO combo implied native ASIO on portable build | Label/hint understated WASAPI fallback | **Resolved** | Label strings + honesty note path | `AudioDriverKind::Asio` label/hover; browser status `ASIO → WASAPI fallback` |
| I-04 | P2 | Audio | Exclusive/ASIO status line ignored ASIO selection | Status only Exclusive/Shared | **Resolved** | Code review | browser status match includes ASIO |
| I-05 | P2 | UI | Full every-widget egui hit-test incomplete | Computer-use pass partial | **Open / accepted** | Function-test covers handlers | Documented limitation |
| I-06 | P3 | CI | Mac/Linux CI noise on unpaid/missing deps | Matrix without continue-on-error historically | **Resolved earlier** | Workflow | `continue-on-error` non-Windows |
| I-07 | P1 | CI | Function-test not in CI | Missing workflow step | **Resolved** | CI YAML | Windows `FunctionTest-Windows.ps1` step |
| I-08 | P1 | Persistence | Autosave failures silent | `let _ = save…` | **Resolved** | Code path | Banner `Autosave failed: …` |
| I-09 | P1 | MIDI | Export .mid discarded `Result` | `let _ = export_smf_clip` | **Resolved** | DockAction::StatusMessage | Success/fail banner |
| I-10 | P2 | Docs | README still said 1.1.3 after 1.1.4 ship | Docs lag | **Resolved** | README | Links to v1.1.4 |
| I-11 | P2 | Updater | PowerShell UTF-8 BOM broke latest.json | Set-Content BOM | **Resolved earlier** | BOM strip + WriteAllText no BOM | Prior release |
| I-12 | P3 | Ship | GH_TOKEN / gh keyring auth failures on some machines | Operator env | **Open / env** | N/A | Operator must use valid `gh auth` |
| I-13 | P2 | Audio | True ASIO needs cpal asio + SDK | By design portable | **Blocked** | N/A | Honesty messaging only |
| I-14 | P3 | Scope | Mac/Linux portable packaging | Out of scope | **Blocked** | Advisory CI | N-06 |
| I-15 | P2 | Docs | Version badge inconsistency | Same as I-10 | **Resolved** | README | Merged with I-10 |
| I-16 | P2 | Audio performance | New instrument prototypes calculated exponential pitch for each voice on every sample | Frequency computed inside real-time sample loop | **Resolved** | DSP runtime and instrument tests; workspace check and tests pass | Note frequency calculated on note-on; pitch bend ratio once per block |
| I-17 | P1 | Session safety | Save before quitting could continue after a failed save when autosave had cleared the dirty flag; Save before other transitions did not continue | Confirmation used project path and autosave state instead of the save result | **Fixed; release-build visual verification pending** | `save_guard_tests`, workspace and function tests pass | Save returns success; pending action executes only after success |
| I-18 | P1 | Session safety | Recovery autosave could suppress the unsaved-work guard for a new project | Recovery and user-project dirty state shared one flag | **Fixed; release-build visual verification pending** | `autosave_does_not_clear_unsaved_project_guard`, workspace and function tests pass | Separate project-save dirty flag is retained through recovery autosave |
| I-19 | P2 | Packaging | A running build locks the default Cargo executable and blocks packaging; alternate Cargo target directories were ignored when copying files | Packaging hard-coded `target/release` | **Resolved** | Alternate target release build and package pass | `Package-WindowsRelease.ps1` now resolves `CARGO_TARGET_DIR` |
| I-20 | P3 | Packaging docs | START_HERE implied first launch prompts for a session type | Stale packaging text | **Resolved** | Release startup visual smoke and zip contents | Text now states the empty-session startup and menu entry points |
| I-21 | P2 | UI | Bounce settings did not share the reference card and rounded-control styling | Legacy flat dialog rows | **Resolved** | Release visual check: format change and actual WAV export; required suites pass | Bounce settings use `chrome_card`, `chrome_row_label`, and `pill_toggle` |
| I-22 | P2 | UI | Button chrome drifted across transport, browser, arrangement, dock, mixer, piano roll, and dialogs | Raw egui buttons, duplicated accent RGBs, role-specific hover/active defaults, unused launcher-slot paint, and track-header control overflow | **Resolved in v1.1.12** | Theme invariants; release visual pass across toolbar, Help, menu, browser, Kaoss, FX, Launch, and arrangement paths | Shared `StudioUiExt`, semantic accents, raised neutral contrast, legacy button migration, quiet unused slots and launcher region, contained track-header control cluster, readable browser rows, standardized danger treatment, and consistent 24 px button geometry |

## Severity counts (including current candidate)

| Severity | Discovered | Resolved | Open/Blocked |
|----------|------------|----------|--------------|
| P0 | 2 | 2 | 0 |
| P1 | 6 | 4 | 2 fixed in candidate, final visual verification pending (I-17, I-18) |
| P2 | 10 | 8 | 2 (I-05 accepted, I-13 blocked) |
| P3 | 4 | 2 | 2 (I-12 env, I-14 blocked) |
