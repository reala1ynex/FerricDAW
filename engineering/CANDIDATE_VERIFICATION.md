# FerricDAW 1.1.13 · Local candidate verification

## v1.1.14 local candidate - 2026-11-16 17:23 ET (branch `fix/v1.1.14-updater-install-dir`)

| Item | Value |
|---|---|
| Staged EXE | `dist\candidate\FerricDAW-stage-20261116-172324\ferricdaw.exe`, 13,824,000 bytes, SHA-256 `31ed0fba228499be43d203d7ebf71af418396be7969a7ba0e0e41afe0f64ccfb`, FileVersion 1.1.14.0 |
| Plugin bridge | 1,494,016 bytes, SHA-256 `98805c638e5e5fc685ebb89b30fb375c67e819f40184311f0a96411097966e83` |
| ZIP | `dist\candidate\FerricDAW-v1.1.14-win-x64.zip`, 12,474,886 bytes, SHA-256 `31ed7f70a6c5e679dc291a833911cbba3e5be3b14dd7cd64eaad4161cf4ae4b2`, 18 entries (adds `ferricdaw-manifest.json`) |
| Feed | `dist\candidate\latest.json`, version 1.1.14, URL `https://github.com/reala1ynex/FerricDAW/releases/download/v1.1.14/FerricDAW-v1.1.14-win-x64.zip`, SHA matches the ZIP |

Sandbox runs used `C:\Users\ndrpe\FerricSandbox\<scenario>` with
`LOCALAPPDATA`, `TEMP` and the `FERRIC_*_DIR` overrides pointed into the
sandbox; the real Desktop, Downloads, Start menu and OneDrive were not touched.
Final state per scenario: `target\release-verification\grok-evidence\v1.1.14\sandbox-final-state.txt`.
The installed EXE in each sandbox matches the staged EXE above.

## Resumed verification - 2026-11-16 14:25 ET (supersedes the hashes below)

| Item | Value |
|---|---|
| Staged EXE | `dist\candidate\FerricDAW-stage-20261116-142529\ferricdaw.exe` (launched; identical to `...-142824`), 13,611,520 bytes, SHA-256 `9699008b230274e53f4a41cb25ae544b21fced76dfd9db3c2162ab7191e59b3f`, FileVersion 1.1.13.0 |
| Plugin bridge | 1,494,016 bytes, SHA-256 `fc6d43e1ac203db6fd561b0e4ef7e7df0e83056e277a7709ea254ed8430943d0` |
| ZIP | `dist\candidate\FerricDAW-v1.1.13-win-x64.zip`, 12,394,214 bytes, SHA-256 `4037dc01d1c6aa3536847d0ee2384e1f0a8c230fbe644dc54a06efa6557a72ae`; 17 entries byte-identical to `FerricDAW-stage-20261116-142824`, repackaged from commit after the docs update (EXE hash unchanged from the launched build) |
| Feed | `dist\candidate\latest.json`, BOM-free, version 1.1.13, URL `https://github.com/reala1ynex/FerricDAW/releases/download/v1.1.13/FerricDAW-v1.1.13-win-x64.zip`, SHA matches the ZIP |

| Command | Result |
|---|---|
| `RUSTFLAGS=-D warnings cargo check --workspace --all-targets` | Passed |
| `cargo test --workspace` | 37 passed, 0 failed (adds heavy drum voices, dark techno patches and Kick Rumble tail, bundled MP3 decode) |
| `powershell -ExecutionPolicy Bypass -File tools\FunctionTest-Windows.ps1` | 62/62 passed |
| `CARGO_TARGET_DIR=target/ship` + `tools\Package-WindowsRelease.ps1 -LocalCandidate -GitHubOwner reala1ynex` | Passed |
| Packaged launch (isolated `LOCALAPPDATA`) | Window "FerricDAW v1.1.13" responding; welcome dialog, empty session and Afterglow captured at 2048x1280; status bar reports 60 UI fps |

Fixed during this pass: I-30 (ruler marker/loop overlap, chord-lane caption
overlap). `cargo fmt --check` reports formatting drift across many files that
predates this work; it is not a CI gate and was left unchanged. The sections
below are the earlier candidate record and are kept for history.
**Status: built and verified within the evidence below; unpublished.**
The requested full reference implementation and exhaustive manual release gates
are still open. No release or tag was created, deleted, or changed.

| Item | Verified value |
|---|---|
| Checkout | `C:\Users\ndrpe\Projects\FerricDAW` |
| Source | Working tree based on `6c5d1ccb7692282750d6b4c2369cf732c2dd935c`, local changes uncommitted |
| Target | Windows x64, `x86_64-pc-windows-gnu`; Rust 1.100.0; `target/release` |
| Tested running EXE | `C:\Users\ndrpe\Projects\FerricDAW\dist\candidate\FerricDAW-stage-20261116-133852\ferricdaw.exe` |
| Tested EXE SHA-256 | `bb55ef03ebf7278044069701e577e365c893280bc096e9e9bd4deb778f94f444` |
| ZIP and feed | `C:\Users\ndrpe\Projects\FerricDAW\dist\candidate\FerricDAW-v1.1.13-win-x64.zip` and adjacent `latest.json` |
| ZIP SHA-256 | `00ac5aa4943afa2ff7d31d89ed40cd80c8a48d0971376cf2480776ce32c492a7` |
| Integrity | BOM-free JSON starts with `{`; the feed hash matches the zip. All 16 files passed CRC and matched every staged file byte for byte. EXE metadata reported 1.1.13. |
| Release channel | [Public v1.1.12](https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.12), sole release and tag; tag commit `4e3c7542d2ce7045e083e0e552408e6b9e216a99` |
| Public ZIP SHA-256 | `df47ae3c82ec0f1afbb92b629bcf6439a06c83597db79d408b80e3d8a871f91b`; independently downloaded public ZIP matches the independently downloaded BOM-free feed. |

Only the staged candidate was targeted during final checks. Older v1.1.11 and
earlier test windows were excluded by their executable path. No OneDrive path
was used for source, package staging, or test-project/export saves.

| Command | Result |
|---|---|
| Set `RUSTFLAGS=-D warnings` and run `cargo check --workspace --all-targets` | Passed |
| `cargo test --workspace` | 35 passed, 0 failed; 8 app, 7 DSP, 10 engine unit + 2 integration, 7 model, 1 bridge |
| `powershell -ExecutionPolicy Bypass -File tools\FunctionTest-Windows.ps1` | 60/60 passed; includes quick-release Ctrl+B through egui events |
| `powershell -ExecutionPolicy Bypass -File tools\Package-WindowsRelease.ps1 -LocalCandidate` | Passed; release binaries, portable ZIP, local feed |
| `ffprobe -v error -show_entries stream=codec_name,duration,sample_rate,channels,bit_rate -of compact` and `ffmpeg -v error -xerror -i <test export> -f null -` | Both exports opened and fully decoded: Vorbis and 192 kbps MPEG Layer III; 48 kHz stereo and 8.885604 s each |

The full output logs live in `engineering/last-*.txt`. The local MP3
integration test ran with FFmpeg present; a separate deterministic test
proves a missing encoder path does not replace an existing destination.

| Changed files | Purpose and issue |
|---|---|
| `theme.rs`, `build.rs`, `help_modal.rs`, `mixer.rs` | Shared reference palette, control and state styling, segmented meters and flat lattice identity; EXE version from Cargo (I-29) |
| `main.rs`, `arrangement.rs`, `browser.rs`, `keyboard_pads.rs`, `piano_roll.rs` | Contained DNA, track and Pad controls, more arrangement space, stable dock, FX toolbar wrapping, readable bounce labels (I-23–I-25) |
| `main.rs`, `function_test.rs` | Shortcut modifiers read from the key event; packaged Ctrl+B and Ctrl+K opened their surfaces (I-27) |
| `lossy_enc.rs`, `wav_io.rs`, engine manifest and lock | Standard Ogg Vorbis and MP3, transactional output and a priming-packet decoder fix; real audio decode coverage (I-26 laptop path fixed; MP3 portability open) |
| Cargo, README, guide, devlog, engineering registries, packaging script/workflow and notice file | Version and capability disclosure; candidate isolation; codec attribution in both ZIP routes |

## Real Windows results and screenshots

Capture dimensions: 1638 × 1024. All images under `FerricDAW-evidence` are native
laptop captures. Paths below refer to the evidence copy beside this report.
Earlier screenshots document intermediate fixes or findings, and are
explicitly separate from the 1.1.13 package proof.

- [Final v1.1.13 workstation](FerricDAW-evidence/22-verified-package-workstation.jpg):
  top-left DNA fits and track mix controls stay inside the card; top-right
  timeline is unblocked; bottom-left Pad pickers fit and device settings are
  scrollable; bottom-right dock no longer takes half the viewport. Reviewed
  browser/arrangement and arrangement/dock seams. The dock resize was dragged
  up and down on the preceding build with identical dock code.
- [Ogg with Golden Bounce](FerricDAW-evidence/23-v1.1.13-ogg-golden-bounce.jpg)
  and [MP3 preview](FerricDAW-evidence/24-v1.1.13-mp3-preview.jpg): top halves
  show the clip and contained format controls, bottom halves show mastering
  settings, report, paired waveforms and playback status. Independent FFmpeg
  decode confirmed file interoperability beyond the UI success banner.
- [Palette](FerricDAW-evidence/25-v1.1.13-palette.jpg): Ctrl+K opens and focuses
  search. Full palette action selection was not replayed.
- Earlier candidate sessions verified Play and Stop, Session toggle,
  Afterglow load, Groove scene activation, Mix display, Samples +Trk,
  Electric Piano preset insertion, Kaoss point/value movement, Route display,
  Night Current start/step A, and Help Quickstart. Dirty Quit Cancel preserved
  the session, Save wrote a 239,390-byte test project and quit, and Discard
  quit a generated tutorial test session. A separate Discard confirmed the
  tutorial replaced only the synthetic sample project.

## Remaining acceptance

I-05 and I-28 remain open. The reference mapping covers every PDF page, but
exact reference parity and an individual screenshot for every applicable page
are not finished. No full check was made at the minimum viewport or across DPI
settings. Unproven workflows include all piano-roll and drum-grid actions,
full tutorial completion, full Mix morph, long recording and autosave recovery,
every plugin editor and audition action, external MIDI and Line-In behavior,
and in-place updater application. True native ASIO remains outside the portable
build. MP3 needs FFmpeg with libmp3lame; a dependency-free MP3 build remains open.
