# Issue Registry

**Last updated:** 2026-11-16

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

## v1.1.14 audit findings — fixed on `fix/v1.1.15-audit` (local, unpublished)

| ID | Sev | Finding | Fix (commit) | Evidence / limits |
|---|---|---|---|---|
| A-01 | High | Project save truncated the file in place | Temp file + fsync + `.bak` + rename (`fd587d6`) | `fsutil::tests` incl. locked target |
| A-02 | High | Save/Save As left takes and samples outside the project; missing audio loaded silently | Collect every buffer into the project; missing/unreadable audio reported (`d83fc54`) | 4 bundle tests in ferric-engine |
| A-03/A-04 | High | Live recording allocated on the audio thread, capped length, dropped quiet takes | Lock-free rings + collector thread (`fdd290c`) | 200 s take, overflow, separate-take tests; callback→UI only component-tested |
| A-05 | High | Unbounded plugin-bridge pipe waits; per-block allocation | Overlapped pipe with timeouts, reusable encode (`88812a7`) | Hung worker <3 s, bounded shutdown tests |
| A-06 | High | FX/modulator/project/preview drops on the audio thread | Boxed pools, `RtGarbage` ring (`17df55e`) | 1 test |
| A-07 | Med | Silent in-process fallback when sandbox failed | Not loaded + notice (`fd8fdcd`) | No automated test (needs audio devices) |
| A-08/A-16 | Med/Low | Cleanup removed copies anywhere; interrupted deletes left holds | Auto scope = Desktop, Downloads, relocation source; others confirmed; hold sweep (`1a8d631`) | 3 tests; sandbox confirm Keep/Remove verified 2026-11-16 |
| A-09/A-10 | Med | PowerShell for known folders and shortcuts (Unicode, policy) | Native Shell COM (`2eabf59`) | Unicode shortcut round trip test |
| A-11 | Med | Failed update left no running app | Restore + restart with `--update-failed` (`182500b`) | Helper test with fake exe |
| A-12 | Med | Loose update URL allowlist; redirects unchecked | Repo-pinned URLs, per-hop redirect check, optional Authenticode (`931f9af`) | Allow/deny tables; live redirect test; Authenticode untested (no certificate) |
| A-13 | Med | Publish could ship a dirty or mismatched build | `build-info.json` + publish guards (`e88c59e`) | Parse-checked; guards not executed (publishing not run) |
| A-14 | Low | NaN panics in float sorts | `total_cmp` (`6076017`) | NaN sort test |
| A-15 | Low | Relocation copy blocked the UI | Background copy + progress (`e780a4c`) | Sandbox move from 1.1.12/1.1.13 |
| A-17 | Low | Misc: decline not remembered, any `.git` disabled cleanup, CLAP host version, Desktop ZIP copy | `e780a4c`, `e88c59e` | Discovery test; sandbox decline verified |
| Clippy | — | 18 dsp + 4 graph `approx_constant`, public raw-pointer fn | `664e5d6` | `cargo clippy` 0 errors |

## 1.1.13 candidate findings — 2026-11-16

| ID | Severity | Finding / source | Repair or outstanding gate |
|---|---|---|---|
| I-23 | P1 | DNA strip clipped by exact panel height; gain/pan outside track card; Pad picker actions touched sidebar edge. `main.rs`, `arrangement.rs`, `keyboard_pads.rs` | Panel content sizes, four-row track minimum, and constrained responsive picker. Checked in packaged workstation capture. |
| I-24 | P1 | Dock drifted to maximum height on idle, reducing arrangement to one visible track in Afterglow. `main.rs` | One resize state owner with a bounded handle. Verified enlarge and shrink on the laptop. |
| I-25 | P1 | OGG format label wrapped vertically and obscured export controls. `main.rs` bounce modal | Short format labels with descriptions on hover, wrapping mastering controls, and action label specific to format and target. Rechecked selected OGG and native Save dialog. |
| I-26 | P1 | MP3/OGG bounced private payloads and WAV trailers. FFprobe independently rejected the prior OGG; a size-only test could not prove interoperability. `lossy_enc.rs`, `wav_io.rs` | Bundled Vorbis, independently installed FFmpeg for MPEG Layer III, transactional replacement for those outputs, and independent decoder regression assertions. Empty Vorbis priming packets are skipped. **Update:** MP3 now uses the bundled pure-Rust `rusty_mp3` 1.0.6 encoder (Apache-2.0, notice in `THIRD-PARTY-NOTICES.txt`); `bundled_mp3_export_decodes_as_standard_stereo` decodes the output through the import path. A clean-machine Windows check without FFmpeg is still unverified. |
| I-27 | P1 | Rapid keyboard chords could be missed when Ctrl was released before frame processing. `main.rs` | Dispatch using key-down modifiers. Added a real egui frame case for Ctrl+B with the frame modifier already released. |
| I-28 | P2 | Main layout still lacks the reference's independent right inspector, persistent master strip and exact MIDI device panels. Speciality browser/routing/FX panels retain layout and typography differences. | Native equivalents and remaining work mapped in `UI_REFERENCE_TRACEABILITY.md`; do not close I-05. |
| I-29 | P3 | Embedded Windows version metadata reported 1.0.0 regardless of Cargo version. `build.rs` | Version resource is generated from Cargo metadata; check the packaged exe properties. |
| I-30 | P2 | Packaged-launch capture: the loop band painted over the song-marker flags (Intro/Hook) on the ruler, and the chord lane key caption (e.g. "A Natural Minor (Aeolian)") overlapped the CoF+/CoFm/Prog/Gen buttons. `arrangement.rs` | Ruler is 36 px with separate rows for loop band, marker flags and bar numbers; the key caption sits right-aligned on the CHORDS title row, clipped to the header. Rechecked in packaged captures of the empty session and Afterglow. |
| I-31 | P1 | Updater / install location | Updating a copy whose `ferricdaw.exe` sat on the Desktop copied every release file (17 in v1.1.13, plus `demo\` and `docs\`) onto the Desktop; the v1.1.12 helper also left its script and the update ZIP in `%TEMP%` | v1.1.12 and v1.1.13 updaters install into `current_exe()`'s folder, whatever that folder is; the ZIP has no top-level folder | **Resolved in 1.1.14 (local, unpublished)** | `install_hygiene` unit tests (13), function test `dirty_session_move_install_guard`, `tools\Sandbox-UpdateTest.ps1` from-1.1.12 / from-1.1.13 / new-updater, fresh-install shortcut run | Shared-folder copies offer to move to `%LOCALAPPDATA%\Programs\FerricDAW`; updates from them install there; old copies, temp leftovers and stray files are removed only on an exact manifest match after the new version starts; shortcuts are retargeted; opt-in desktop shortcut without duplicates. `target\release-verification\grok-evidence\v1.1.14\` |

The older severity totals above describe the pre-1.1.13 registry. The previous
release evidence is retained separately from this candidate review.
