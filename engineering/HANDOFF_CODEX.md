# FerricDAW — Full Codex Handoff

**Audience:** Codex (or any fresh agent) continuing FerricDAW with zero chat memory.  
**Authority:** Treat this file + `/engineering/*.md` as the continuity source of truth. Verify claims against git, code, and tests before marking work complete.  
**Handoff date:** 2026-11-15 (mission amended same day)  
**Do not invent unrelated product categories** (no cloud accounts, no liquid glass). You **are** authorized to add instruments, fix flaws, polish UI chrome, and ship.

---

## ★ 2026-11-15 mission status — v1.1.12 candidate

v1.1.12 is the next candidate after the arrangement control-containment cleanup. Automated gates and the inspected visual surfaces pass; publish and remote feed/hash verification remain. Read `PROGRESS.md` and `RELEASE_READINESS.md` for exact visual coverage and the still-open I-05 limitation. The directive below is retained as historical context; do not claim every UI widget was pixel-tested.

## ★ HISTORICAL MISSION (owner directive)

Owner wants a **final end-user quality pass**, then ship. Do the real work — not a plan-only reply.

### A. Flaw review (adversarial)

1. Recon the app as an end user: every browser tab, dock tab, modal, transport control, palette command.  
2. File findings in `engineering/ISSUE_REGISTRY.md` (new I-IDs). Fix P0/P1 immediately; fix worthwhile P2s in the same ship.  
3. Anything you discover while reviewing (broken buttons, silent failures, chrome mismatches, missing presets, dead instrument paths) → fix it.

### B. Add more instruments (+ related polish)

1. Expand the built-in instrument set beyond the current `FxKind` synths (PolySynth, MonoBass, Prism, HostDNA, Wavetable, DrumKit, SamplePlayer, …).  
2. Prefer **real DSP + model + UI wiring + palette/factory presets**, not stubs.  
3. Wire new instruments into: palette “Add … Track”, FX insert menus, factory presets where natural, and **function-test cases**.  
4. Keep Pad A/B `chrome_card` / `chrome_row_label` visual language.

### C. Same automated tests we ran (mandatory)

```powershell
cd C:\Users\ndrpe\Projects\FerricDAW
$env:RUSTFLAGS="-D warnings"; cargo check --workspace --all-targets; Remove-Item Env:RUSTFLAGS
cargo test --workspace
powershell -ExecutionPolicy Bypass -File tools\FunctionTest-Windows.ps1
```

- Function-test must stay **debug** (release `panic=abort` breaks isolation).  
- Extend `function_test.rs` for every new instrument / button handler.  
- Record results in `engineering/last-*.txt` + `TEST_REGISTRY.md`.  
- **Do not** delete or weaken failing tests.

### D. Final end-user visual run (mandatory)

1. Launch the **packaged or release** build (or `cargo run --release`) on Windows.  
2. Walk **every** surface as a user: first-run / empty session, Night Current tutorial steps, Afterglow, Hardware Rig, Audio I/O, Pads, Samples, Presets/Plugins, arrangement, piano roll, Kaoss Write XY, Mix, FX, Launch Pad, Bounce, Check for Updates modal, dirty close Save/Discard/Cancel.  
3. Visual bar: Pad A/B chrome density — no orphan flat panels; labels use `Foo ->` row recipe where pickers exist.  
4. Confirm every control you touch **does what it says** (not just that a handler exists).  
5. Log visual findings + fixes in ISSUE_REGISTRY / PROGRESS.

### E. Ship to GitHub (mandatory when green)

1. Bump workspace version (next: **1.1.7** unless you need a larger bump).  
2. Update `README.md`, `docs/DEVLOG.md`, `/engineering` readiness.  
3. Commit logical changes.  
4. Publish:

```powershell
Remove-Item Env:GH_TOKEN -ErrorAction SilentlyContinue
Remove-Item Env:GITHUB_TOKEN -ErrorAction SilentlyContinue
powershell -ExecutionPolicy Bypass -File tools\Publish-GitHubRelease.ps1
```

5. **Only-current channel:** delete prior GitHub release + tag (e.g. `v1.1.6`) after `v1.1.7` is live.  
6. Verify remote `latest.json`: starts with `{`, **no UTF-8 BOM**, `sha256` matches zip.  
7. Push `master` + tag; leave registries accurate.

### Mission success criteria

- [ ] Flaw review done; P0/P1 cleared or explicitly blocked with evidence  
- [ ] New instrument(s) implemented, audible, UI-reachable, tested  
- [ ] `cargo test --workspace` PASS  
- [ ] Function-test PASS (all cases, including new ones)  
- [ ] Visual end-user run completed; chrome consistent with Pad A/B  
- [ ] New version published on GitHub Releases; only one Latest  
- [ ] `/engineering` updated + checkpoint recorded  

---

## 0. Load state first (mandatory)

```text
1. Read engineering/HANDOFF_CODEX.md          (this file) — execute ★ ACTIVE MISSION
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

## 10. Next work

**Owner has opened ★ ACTIVE MISSION (top of this file).** That is the priority — not optional.

Still out of scope unless owner reopens:

- True ASIO SDK in portable zip (I-13) without approval  
- Mac/Linux portable packaging (I-14)  
- Liquid-glass / full rebrand  

Instrument design notes (guidance, not a closed list):

- Existing: PolySynth, MonoBass, Prism, HostDNA, Wavetable, DrumKit, SamplePlayer  
- Good additions: keys/epiano, organ, pluck/karplus, noise/percussion one-shots, FM operator variant, simple sampler-kit expansions — real DSP in `ferric-dsp`, `FxKind` in model, engine graph wire, palette + presets + function-test  
- Prefer quality over quantity: 1–3 solid instruments beat 10 stubs  

Ship path when mission green: bump → docs → tests → `Publish-GitHubRelease.ps1` → delete previous release/tag → update `/engineering`.

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

## 13. Paste this into Codex (owner brief)

> Continue FerricDAW at `C:\Users\ndrpe\Projects\FerricDAW`. Owner: Patrick Rudolph / @reala1ynex. **Read `engineering/HANDOFF_CODEX.md` and execute the ★ ACTIVE MISSION at the top.** Current public ship is **v1.1.6**; your job is the final end-user visual run, the same automated suite (`cargo check` with `-D warnings`, `cargo test --workspace`, `tools/FunctionTest-Windows.ps1`), a flaw review with fixes, **add more real instruments** (DSP+UI+presets+tests — no stubs), fix anything else you find, then **bump and publish to GitHub** with `tools/Publish-GitHubRelease.ps1` (clear stale `GH_TOKEN`), keep only-current release channel, and update `/engineering`. Keep Pad A/B `chrome_card` / `chrome_row_label` chrome — no liquid glass. Do not invent cloud/accounts/Mac-Linux packaging. Never write under OneDrive. Evidence only — do not claim complete without tests + visual pass.

---

## 14. Contact / repo ops

- Issues / PR: GitHub on `reala1ynex/FerricDAW`  
- `gh` account: `reala1ynex` (keyring)  
- Remote: `origin` → `https://github.com/reala1ynex/FerricDAW.git`

**End of handoff.** If this file and the live repo disagree, **trust the repo + test evidence**, then fix the registries.
