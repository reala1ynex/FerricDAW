# FerricDAW Devlog

Notes on what shipped and when. Newest first.

**Owner:** Patrick Rudolph ([@reala1ynex](https://github.com/reala1ynex))  
**Repo:** [github.com/reala1ynex/FerricDAW](https://github.com/reala1ynex/FerricDAW)  
**License:** MIT OR Apache-2.0

---

## 2026-11-15 — v1.1.8 UI polish

- Unified buttons, toggles, menus, and action rows around the compact Ferric chrome: one 24 px geometry, 5 px radius, shared inactive/hover/open/active states, and semantic teal/orange/purple/amber/red accents.
- Migrated transport, browser, arrangement, mixer, piano-roll, tutorial, help, update, footer, and dock actions to the shared styling helpers; destructive actions now share one danger treatment.
- Replaced duplicated inline accent RGB values with named palette tokens and added theme invariant tests.
- Verified the isolated release build visually across the toolbar, Hardware Rig, Help tabs, Ferric menu, Kaoss dock, and FX dock.

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
## 2026-11-15 — v1.1.9 visual contrast follow-up

- Raised the shared neutral button surface and hover/active contrast to match the reference chrome without changing accent semantics.
- Styled the remaining legacy arrangement and piano-roll buttons with the same shared surface, border, radius, and geometry.
- Tightened browser hints and raised dense sample/preset labels; truncated names now expose the full value on hover.
- Visual loop rechecked Hardware, Samples, Presets/Plugins, Ferric menu, FX dock, Launch dock, and Kaoss surface; no remaining obvious mismatch was observed in those inspected surfaces.
- Verification: warning-denied workspace check, workspace tests, and Windows function test 59/59 passed.

## 2026-11-15 — v1.1.10 arrangement cleanup

- Removed the painted dark rectangles for unused Session launcher slots; empty slots stay clean and reveal a `+` affordance only on hover.
- Preserved click-to-create 4-bar sketch loops and added an explicit hover hint so the interaction remains discoverable without visual noise.
- Verification follows after the release build: warning-denied workspace check, workspace tests, Windows function test, and a final visual pass.

