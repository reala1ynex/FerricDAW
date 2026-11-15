# FerricDAW — Full Codex Handoff

**Audience:** Codex (or any fresh agent) continuing FerricDAW with zero chat memory.  
**Authority:** Treat this file + `/engineering/*.md` as the continuity source of truth. Verify claims against git, code, and tests before marking work complete.  
**Handoff date:** 2026-11-15  
**Do not invent major product features.** Finish, repair, verify, ship.

---

## 0. Load state first (mandatory)

```text
1. Read engineering/HANDOFF_CODEX.md          (this file)
2. Read engineering/PROGRESS.md
3. Read engineering/ISSUE_REGISTRY.md
4. Read engineering/RELEASE_READINESS.md
5. Read engineering/DECISIONS.md
6. git status -sb && git log -8 --oneline && git rev-parse HEAD
7. Confirm Cargo.toml workspace version matches README + latest GitHub release
```

Never assume a prior agent finished a task unless git + tests prove it.

---

## 1. Identity & ownership

| Field | Value |
|-------|-------|
| Product | **FerricDAW** — portable Windows hybrid DAW (Rust + egui) |
| Owner | Patrick Rudolph ([@reala1ynex](https://github.com/reala1ynex)) |
| Repo | https://github.com/reala1ynex/FerricDAW |
| Local path | `C:\Users\ndrpe\Projects\FerricDAW` |
| License | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) |
| Authors | `AUTHORS.md` |
| Devlog | `docs/DEVLOG.md` |
| Guides | `docs/BEGINNER_GUIDE.md`, `docs/FEATURE_GUIDE.md`, `docs/NIGHT_CURRENT_DEMO.md` |

Preserve owner/docs attribution. Do not strip AUTHORS, licenses, or human-facing guides.

---

## 2. Current shipped state (verified)

| Item | Value |
|------|-------|
| **Current public version** | **1.1.6** (only current release — older tags deleted) |
| **Release** | https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.6 |
| **Branch** | `master` (tracks `origin/master`) |
| **HEAD at handoff** | `9ce2698` (Cargo.lock sync after `a79dc84` UI chrome ship) |
| **Tagged release commit** | `a79dc84` (`v1.1.6`) |
| **Feed** | https://github.com/reala1ynex/FerricDAW/releases/latest/download/latest.json |
| **Feed SHA-256** | `29b2a6d3ba91507b701ec11d22656666357e198a41381699f97d398e8e74b5a3` |
| **Desktop zip** | `C:\Users\ndrpe\Desktop\FerricDAW-v1.1.6-win-x64.zip` |
| **Stage dir** | `C:\Users\ndrpe\Projects\FerricDAW\dist\FerricDAW\` |
| **Readiness** | **CONDITIONALLY READY** (see §9) |

### Recent ship sequence (newest first)

1. **v1.1.6** — Pad A/B chrome pass across Samples, Presets, DNA, Launch, Mixer, FX, piano roll, routing  
2. **v1.1.5** — Dirty close, updater SHA/URL harden, autosave/MIDI banners, ASIO honesty, `/engineering`, CI function-test  
3. **v1.1.4** — Function-test harness, tutorial/cold-start fixes, Hardware Rig + Kaoss chrome  
4. **v1.1.3** — MIDI Backspace delete, Kaoss Write XY automation, BOM-free latest.json  

---

## 3. What FerricDAW is (product truth)

Hybrid DAW aimed at home recording + portable Windows:

- Universal tracks (audio + MIDI + FX + session slots)
- WASAPI Shared / Exclusive (+ ASIO *request* with honesty fallback)
- USB MIDI, musical typing, dual keyboard pads (Pad A / Pad B)
- Piano roll, drum grid, bounce (WAV/FLAC/MP3/OGG + LUFS/TP)
- CLAP/VST3 via out-of-process `ferric-plugin-bridge.exe`
- Night Current tutorial + Afterglow playground
- In-app portable updater (`latest.json` + SHA-256)

**Not in scope:** Mac/Linux portable packaging, accounts/cloud, liquid-glass redesign, inventing large new feature families without requirements evidence.

---

## 4. Architecture (crate map)

```
ferricdaw.exe (crates/ferric-app)
  ├─ UI: main.rs, browser.rs, arrangement.rs, piano_roll.rs, mixer.rs,
  │      keyboard_pads.rs, theme.rs, update.rs, function_test.rs, …
  ├─ ferric-model   — Project graph, clips, FX, SMF, theory, serialization
  ├─ ferric-dsp     — Processors (EQ, synths, HostDNA, dynamics, …)
  ├─ ferric-engine  — RT audio/MIDI, device I/O, bounce, plugin host client
  └─ ferric-plugin-bridge.exe — Crash-isolated CLAP/VST3 worker
```

### Critical UI chrome convention (do not regress)

**Canonical look = Pad A / Pad B strip.**

Shared helpers in `crates/ferric-app/src/theme.rs`:

- `chrome_card(ui, accent, …)` — elevated inset card, 6px radius, 1px accent stroke  
- `chrome_row_label(ui, "In ->", color)` — colored `Label ->` picker row caption  
- `chrome_section_title(ui, title, color)` — card header  

Already matched (keep consistent):

- Hardware Rig theater / spawn / USB MIDI / Audio I/O  
- Keyboard pads (Pad A/B pickers)  
- Kaoss  
- Samples + Presets/Plugins  
- DNA strip, Launch Pad, Mixer recall, Routing header  
- FX rack toolbar, piano-roll toolbar  

**Forbidden:** liquid-glass, purple/glow redesigns, replacing Pad A/B chrome with a new visual language.

---

## 5. Hard constraints (operator / agent policy)

1. **No OneDrive writes** — never create/modify under any path containing `OneDrive` (see `C:\Users\ndrpe\AGENTS.md`). Prefer `C:\Users\ndrpe\Projects\FerricDAW` or `D:\CodexProjects`.  
2. **Only-current release channel** — after shipping `vX.Y.Z`, delete prior GitHub release + tag. Keep one Latest.  
3. **Updater feed must be BOM-free JSON** — PowerShell `Set-Content -Encoding utf8` historically broke the app; packaging uses `UTF8Encoding(false)`.  
4. **Clear stale `GH_TOKEN` / `GITHUB_TOKEN`** before `gh` — env token often 401s; keyring login is valid. `tools/Publish-GitHubRelease.ps1` already removes those env vars.  
5. **Do not invent major features** without requirements evidence in `/engineering` or docs.  
6. **Never weaken tests** or delete failing coverage to fake green.  
7. **Release profile uses `panic = "abort"`** — function-test must run **debug** (`catch_unwind`).  
8. **Preserve AUTHORS / licenses / human docs** when cleaning old local zip copies.

---

## 6. Engineering registry system (permanent)

Directory: `C:\Users\ndrpe\Projects\FerricDAW\engineering\`

| File | Purpose |
|------|---------|
| `HANDOFF_CODEX.md` | This handoff |
| `REQUIREMENTS.md` | Req / R-IDs |
| `FEATURE_REGISTRY.md` | Feature status + evidence |
| `ISSUE_REGISTRY.md` | Defects I-01… |
| `TEST_REGISTRY.md` | Test suites + results |
| `ARCHITECTURE.md` | Subsystems |
| `DECISIONS.md` | ADRs D-01… |
| `PROGRESS.md` | Active ledger / next actions |
| `CHECKPOINTS.md` | Session checkpoints |
| `RELEASE_READINESS.md` | Gate table + classification |
| `last-cargo-test.txt` | Last workspace test log |
| `last-function-test.txt` | Last function-test log |

**Cycle:** Load → Select highest P0/P1 → Implement → Test → Update registries → Checkpoint commit.

---

## 7. Build / test / ship commands

```powershell
cd C:\Users\ndrpe\Projects\FerricDAW

# Dev run
cargo run --release

# Zero-warning check (CI style)
$env:RUSTFLAGS="-D warnings"; cargo check --workspace --all-targets; Remove-Item Env:RUSTFLAGS

# Unit / integration
cargo test --workspace

# Sequential every-button handler suite (DEBUG build)
powershell -ExecutionPolicy Bypass -File tools\FunctionTest-Windows.ps1

# Package + publish (clears bad GH_TOKEN internally)
Remove-Item Env:GH_TOKEN -ErrorAction SilentlyContinue
Remove-Item Env:GITHUB_TOKEN -ErrorAction SilentlyContinue
powershell -ExecutionPolicy Bypass -File tools\Publish-GitHubRelease.ps1

# After publish: only-current cleanup example
# gh release delete vPREV --yes
# git tag -d vPREV
# git push origin :refs/tags/vPREV
```

### Key tools

| Script | Role |
|--------|------|
| `tools/Package-WindowsRelease.ps1` | Build release binaries, stage `dist/FerricDAW`, zip, write BOM-free `latest.json` |
| `tools/Publish-GitHubRelease.ps1` | Package + tag + push + `gh release create/upload` |
| `tools/FunctionTest-Windows.ps1` | Builds (debug) + `ferricdaw.exe --function-test` |
| `tools/Setup-GitHub.ps1` | Remote/auth helpers |
| `tools/SetLineInCapture.ps1` / `SetDs10Capture.ps1` | Windows capture tips |

### CI

`.github/workflows/ci-release.yml`

- Matrix: Windows required; Mac/Linux `continue-on-error: true`  
- Windows step runs `FunctionTest-Windows.ps1`  
- Tag `v*` → package + softprops release upload  

---

## 8. Important code entry points

| Concern | Location |
|---------|----------|
| App entry / transport / menus / DNA / Launch / dirty close | `crates/ferric-app/src/main.rs` |
| Hardware Rig, Samples, Presets, Kaoss, Audio I/O | `crates/ferric-app/src/browser.rs` |
| Pad A/B keyboard pads | `crates/ferric-app/src/keyboard_pads.rs` |
| Piano roll + FX rack dock | `crates/ferric-app/src/piano_roll.rs` |
| Mixer + routing | `crates/ferric-app/src/mixer.rs` |
| Theme / chrome helpers | `crates/ferric-app/src/theme.rs` |
| Updater (SHA + GitHub HTTPS allowlist) | `crates/ferric-app/src/update.rs` |
| Function-test cases (57) | `crates/ferric-app/src/function_test.rs` |
| Audio driver kinds / ASIO probe | `crates/ferric-engine/src/audio_backend.rs` |
| Workspace version | root `Cargo.toml` → `[workspace.package] version` |

### Behavioral contracts Codex must not break

- **Dirty close:** `ViewportCommand::CancelClose` + `PendingDestructive::QuitApp` when `session_has_user_work()`; updater quit (`quit_for_update`) bypasses.  
- **Updater:** require 64-hex `sha256`; URL allowlist HTTPS github.com / `*.githubusercontent.com`; strip UTF-8 BOM on feed body.  
- **Function-test:** extend cases when adding button handlers; keep debug-capable.  
- **Cold start:** empty session by default; Afterglow via menu; Night Current tutorial optional.  
- **MIDI delete:** Backspace/Delete removes selected piano-roll notes (global shortcut path + roll).  
- **Kaoss Write XY:** while Play/Rec, writes FxParam automation.

---

## 9. Open / blocked / accepted (do not “fix” by lying)

| ID | Sev | Status | Notes |
|----|-----|--------|-------|
| I-05 | P2 | **Accepted** | No full egui pixel hit-test; function-test covers handlers |
| I-12 | P3 | **Env** | Stale `GH_TOKEN` breaks `gh`; clear env, use keyring |
| I-13 | P2 | **Blocked** | True ASIO needs cpal `asio` + SDK — portable ships honesty fallback |
| I-14 | P3 | **Blocked** | Mac/Linux portable packaging out of scope |

All former P0/P1 ship blockers (I-01..I-03, I-07..I-09) are **resolved in 1.1.5+**.

**Release classification:** CONDITIONALLY READY — shipping gates green; not VERIFIED READY while I-05/I-13/I-14 remain incomplete by design/scope.

---

## 10. Suggested next work (priority order)

Only pick these if the owner asks, or if a regression appears:

1. **Visual QA** of 1.1.6 chrome on live UI (Hardware → Samples → Presets → Mix → FX → Editor → Launch → Kaoss) — screenshot/compare density to Pad A/B.  
2. **Regression:** if any panel still looks “flat” vs Pad strip, wrap with `chrome_card` / `chrome_row_label` (same recipe).  
3. **Optional I-05:** egui interaction harness beyond handler function-test (expensive; not required for ship).  
4. **Do not** enable ASIO SDK in portable zip without explicit owner approval + SDK availability.  
5. **Do not** expand Mac/Linux packaging unless owner reopens I-14.

If starting a new ship: bump `Cargo.toml` version → update README + DEVLOG → tests → `Publish-GitHubRelease.ps1` → delete previous release/tag → refresh `/engineering` readiness.

---

## 11. Local cleanup policy

When removing old local Ferric copies:

- Delete outdated `FerricDAW-v*-win-x64.zip` / extracted folders  
- **Keep** `AUTHORS.md`, `docs/`, licenses, Night Current demo WAV if present  
- Prefer keeping only **current** zip on Desktop + `dist/`

---

## 12. Verification checklist for any Codex session end

- [ ] `cargo test --workspace` (or justify skip)  
- [ ] `tools/FunctionTest-Windows.ps1` if UI/handlers touched  
- [ ] README / DEVLOG / Cargo version agree  
- [ ] `/engineering/PROGRESS.md` + ISSUE/TEST registries updated  
- [ ] Checkpoint note in `CHECKPOINTS.md` with commit hash  
- [ ] No OneDrive paths written  
- [ ] If published: feed JSON starts with `{`, no BOM, sha256 matches zip  
- [ ] Only one GitHub Latest release remains  

---

## 13. One-paragraph brief you can paste into Codex

> Continue FerricDAW at `C:\Users\ndrpe\Projects\FerricDAW`. Current ship is **v1.1.6** on `master` (HEAD `9ce2698`; tag on `a79dc84`). Owner: Patrick Rudolph / @reala1ynex. Read `engineering/HANDOFF_CODEX.md` then PROGRESS/ISSUE/RELEASE_READINESS. Windows portable DAW (Rust/egui). Do not invent major features; do not liquid-glass the UI; keep Pad A/B `chrome_card` / `chrome_row_label` chrome. Only-current GitHub release channel. Clear stale `GH_TOKEN` before `gh`. Publish via `tools/Publish-GitHubRelease.ps1`. Function-test is debug `tools/FunctionTest-Windows.ps1` (57 cases). Accepted limits: no egui pixel hit-test (I-05), no true ASIO in portable (I-13), no Mac/Linux ship (I-14). Update `/engineering` registries every cycle. Never write under OneDrive.

---

## 14. Contact / repo ops

- Issues / PR: GitHub on `reala1ynex/FerricDAW`  
- `gh` account: `reala1ynex` (keyring)  
- Remote: `origin` → `https://github.com/reala1ynex/FerricDAW.git`

**End of handoff.** If this file and the live repo disagree, **trust the repo + test evidence**, then fix the registries.
