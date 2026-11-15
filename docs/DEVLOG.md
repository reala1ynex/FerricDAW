# FerricDAW Devlog

Notes on what shipped and when. Newest first.

**Owner:** Patrick Rudolph ([@reala1ynex](https://github.com/reala1ynex))  
**Repo:** [github.com/reala1ynex/FerricDAW](https://github.com/reala1ynex/FerricDAW)  
**License:** MIT OR Apache-2.0

---

## 2026-11-15 — v1.1.7

- Added polyphonic drawbar organ and velocity-sensitive electric piano, with factory presets and palette actions.
- Reduced per-sample pitch calculation in both instruments; added MIDI and runtime audio tests.
- Hardened save-before-close and session transitions against failed saves and recovery-autosave state.
- Matched bounce controls to the existing compact Ferric chrome and made packaging honor `CARGO_TARGET_DIR`.

---

## 2026-11-15 — v1.1.6

- UI chrome pass: Samples, Presets/Plugins, DNA, Launch Pad, Mixer recall, FX rack, piano-roll toolbar, routing — same Pad A/B `chrome_card` / `chrome_row_label` recipe as Hardware Rig + Audio I/O + Kaoss

Grab it: [Releases · v1.1.6](https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.6)

---

## 2026-11-15 — v1.1.5

- Dirty window-close guard (CancelClose + Save / Discard / Cancel)
- Updater requires SHA-256 and GitHub HTTPS hosts only
- Autosave and MIDI export failures surface in the status banner
- ASIO picker honesty labels + WASAPI fallback status
- CI runs Windows function-test; `/engineering` registries for continuity
- README / release channel: current version only

---

## 2026-11-15 — v1.1.4

- Function test harness: `ferricdaw.exe --function-test` / `tools\FunctionTest-Windows.ps1`
- Cold start opens empty session; Afterglow only from menu
- Tutorial Apply skips rebuild when step already complete; Esc overlay race fixed
- Hardware Rig + Kaoss use Pad A/B `chrome_card`; recording offset syncs browser state
- Package START_HERE / demo README without UTF-8 BOM

---

## 2026-11-15 — v1.1.3

- Fix **Backspace/Delete** for selected piano-roll MIDI notes
- Kaoss **Write XY automation** while Play/Rec (FxParam lanes)
- Hardware Rig Audio I/O chrome matched to Pad A/B rows
- Auto-update feed: `latest.json` UTF-8 without BOM (fixes invalid JSON parse)

---

## 2026-11-15 — v1.1.2

- Portable Windows zip with in-app **Check for Updates** (`latest.json` feed on GitHub Releases)
- Clearer Night Current tutorial (numbered steps, “Do this step for me”)
- Docs rewrite — README, beginner guide, feature guide, F1 help
- Track types, playhead scrub, piano-roll polish, Kaoss XY live morph
- Packaging scripts for zip + GitHub release upload

---

## 2026-11-14 — v1.1.1

Earlier cut: icon, version info, F1 quickstart, and studio workflow polish.
