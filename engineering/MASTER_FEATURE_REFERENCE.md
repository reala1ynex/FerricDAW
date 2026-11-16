# FerricDAW — Master Feature Reference & Handoff Guide

**Audience:** Patrick, Codex, or any human/AI engineer resuming the project.

**Product:** FerricDAW, portable Windows-first hybrid DAW.

**Repository:** `C:\Users\ndrpe\Projects\FerricDAW`

**Current public release:** v1.1.12 — [GitHub release](https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.12)

**Release code commit:** `4e3c754` · **current documentation HEAD:** `4f8fd55`

**Primary handoff:** [`HANDOFF_CODEX.md`](HANDOFF_CODEX.md)

This is the cross-reference map: user-facing feature → UI route → source of
truth → verification → known boundary. It is intentionally evidence-first.
“Complete” below means complete at the stated digital scope, not universal
engineering correctness, physical acceptance, or exhaustive pixel testing.

---

## 1. Product shape

```text
FerricDAW.exe (egui / ferric-app)
├─ Main shell + transport + menus + modal guards
├─ Browser: Hardware Rig / Samples / Presets-Plugins
├─ Arrangement: tracks / clips / Session launcher / Chord Lane / Storyboard
├─ Lower dock: MIDI / FX / Kaoss / Launch Pad / Mix
├─ Piano roll + drum grid + automation
└─ Update / Tutorial / Bounce / Golden Bounce / Help

ferric-model       project graph, clips, routing, MIDI/SMF, theory, persistence
ferric-dsp         instruments, synths, FX, meters, automation DSP
ferric-engine      realtime audio/MIDI, devices, bounce, plugin client
ferric-plugin-bridge  crash-isolated CLAP/VST3 worker
```

### Source-of-truth rules

| Concern | Authoritative location |
|---|---|
| Product requirements | `engineering/REQUIREMENTS.md` |
| Feature status | `engineering/FEATURE_REGISTRY.md` |
| Defects and limits | `engineering/ISSUE_REGISTRY.md` |
| Architecture | `engineering/ARCHITECTURE.md` |
| Decisions | `engineering/DECISIONS.md` |
| Tests and evidence | `engineering/TEST_REGISTRY.md`, `engineering/last-*.txt` |
| Current progress | `engineering/PROGRESS.md` |
| Continuity | `engineering/HANDOFF_CODEX.md` |
| Human feature usage | `docs/FEATURE_GUIDE.md`, `docs/BEGINNER_GUIDE.md` |
| UI chrome | `crates/ferric-app/src/theme.rs` |
| Release packaging | `tools/Package-WindowsRelease.ps1`, `tools/Publish-GitHubRelease.ps1` |

---

## 2. UI map: where a user finds everything

| Surface | Primary controls | Source files |
|---|---|---|
| Top transport | Browser, Ferric menu, play/stop/record, loop, click, capture, BPM, pads, help, tutorial, palette | `main.rs`, `palette.rs` |
| DNA strip | Feel, groove, velocity, accent shape, Apply drums, Apply all MIDI | `main.rs` |
| Left Browser | Hardware Rig, Samples, Presets/Plugins | `browser.rs` |
| Arrangement | Session clips, scenes, chords, storyboard, track headers, timeline | `arrangement.rs` |
| Piano roll | MIDI notes, quantize, draw/edit/delete, drum grid, automation | `piano_roll.rs` |
| Lower dock | MIDI, FX, Kaoss, Launch, Mix, routing | `main.rs`, `mixer.rs`, `browser.rs` |
| Pads | Pad A letter row, Pad B numpad/number row, track assignment, layout | `keyboard_pads.rs` |
| Bounce | Format, loudness, true peak, stems, output path, golden reference | `main.rs`, engine bounce modules |
| Modals | Help, tutorial, update, dirty-close, bounce, palette | `help_modal.rs`, `main.rs`, `update.rs`, `palette.rs` |

### Visual contract

The canonical chrome is the Pad A/B language:

- `chrome_card` for an inset card with a restrained accent stroke.
- `chrome_row_label` for `Label ->` picker rows.
- `chrome_section_title` for compact section headings.
- Neutral buttons share one compact geometry; accent color communicates state,
  not decoration.
- Do not introduce liquid glass, purple glow panels, or unrelated redesigns.
- Empty Session launcher space remains visually quiet until hover/click.
- Track-header controls stay inside the track card; this is the v1.1.12 fix.

---

## 3. Feature-to-code-to-evidence matrix

### 3.1 Core studio and project lifecycle

| ID | Capability | User route | Implementation | Verification | State |
|---|---|---|---|---|---|
| F-01 / R-01 | Hybrid timeline with audio, MIDI, clips, takes, folders, session slots | Main arrangement | `arrangement.rs`, `ferric-model` | Model tests, function test, visual pass | Complete at digital scope |
| F-02 / R-02 | Play, stop, record, loop, metronome/click, capture | Top transport | `main.rs`, `ferric-engine` | Transport function cases | Verified |
| F-03 / R-03 | Save/load `.ferric` / JSON bundle | Ferric → Open/Save | model serialization + file handlers | Model roundtrip tests | Verified |
| F-04 / R-04 | Autosave and recovery snapshot | Automatic / recovery flow | `main.rs` autosave path | Failure banners and save tests | Verified with limits |
| F-05 / R-05 | Dirty close guard | New/Open/Demo/close | `PendingDestructive`, `CancelClose` | Save-guard tests | Verified |
| F-06 / R-06 | Master/stem bounce | Ctrl+B | engine bounce + modal | Function test, DSP/master tests | Verified |
| F-07 / R-07 | Piano roll draw/edit/quantize/delete | Piano roll | `piano_roll.rs` | Function cases including Backspace | Verified |
| F-08 / R-08 | SMF MIDI import/export | MIDI menu/dialog | `ferric-model` SMF | Roundtrip test + error banners | Verified |
| F-13 / R-13 | Tutorial, demo, first-run | 🎓 Tutorial / Ferric menu | `main.rs`, tutorial state | Function test + visual navigation | Verified with I-05 limits |
| F-14 / R-14 | Portable updater | Ferric → Check for Updates | `update.rs`, feed validation | URL/SHA/BOM tests + release feed | Verified |

### 3.2 Audio, MIDI, instruments, and FX

| ID | Capability | User route | Implementation | Verification | State |
|---|---|---|---|---|---|
| R-09 | WASAPI Shared/Exclusive | Hardware Rig / Audio I/O | `audio_backend.rs` | Function + status messaging | Verified |
| R-10 | ASIO request with honest fallback | Audio I/O driver picker | `audio_backend.rs` | Selection path and fallback note | Verified as honest fallback |
| R-11 | USB MIDI, musical typing, MIDI learn | Hardware Rig / Pads / track header | `browser.rs`, `keyboard_pads.rs` | Rescan/learn function cases | Verified |
| R-12 | CLAP/VST3 plugin hosting | Presets/Plugins → Audition/Insert | `ferric-plugin-bridge`, plugin UI | Sandbox tests + real plugin test | Implemented; soak incomplete |
| R-17 | PolySynth, MonoBass, Prism, HostDNA, FerricWave, Wavetable, Drum Kit, Sampler, Vocal Auto-Pitch, Tuner, Amp/Cab, EQ, Comp, OTT, Tape, Delay, Reverb, IR | Palette, FX rack, presets | `ferric-dsp`, `ferric-model`, `browser.rs` | DSP/model tests, function cases, factory presets | Implemented; depth varies |
| DNA | Groove/Velocity DNA | DNA strip | `main.rs`, model groove logic | Function paths + deterministic project behavior | Implemented |
| HostDNA | Seeded host-fingerprint synth | Ctrl+K / FX insert | DSP/model HostDNA | Feature guide + function path | Implemented |
| F-15 / R-15 | Kaoss XY note/FX automation | Lower dock → Kaoss | browser/engine automation | Write XY function case | Verified |
| Pads | Dual keyboard performance | Pad A/B | `keyboard_pads.rs` | Function paths + visual chrome | Verified |

### 3.3 Arrangement, composition, and mix

| Capability | User route | Implementation | Evidence / limit |
|---|---|---|---|
| Chord Storyboard | Arrangement section chips + Chord Lane Gen | `arrangement.rs`, model theory | Generate/undo paths tested; every edge case not exhaustively tested |
| Chord-follow | Chord Lane / generated tracks | model + arrangement | Included in model milestone tests |
| Scene Launch Pad | Lower dock → Launch | `arrangement.rs` / engine | Scene firing and morph paths; edge cases remain |
| Mix A/B/C/D | Lower dock → Mix | `mixer.rs`, model mix recall | Mix recall and routing paths implemented |
| VCA / sidechain / PDC | Mix and routing panels | `mixer.rs`, ferric-model/engine | Model and engine tests; exhaustive UI interaction not claimed |
| Freeze / unfreeze | Track header / Ctrl+F | engine render path | Offline render test; visual header containment verified |
| Golden Bounce | After successful bounce | main + audio analysis | Feature guide; visual/functional coverage present |
| Tempo/meter conductor | Tempo Map / conductor lane | model + arrangement | Conductor tests and UI route |

### 3.4 Release and operational infrastructure

| Capability | Route | Source | Evidence |
|---|---|---|---|
| Portable Windows package | `tools/Package-WindowsRelease.ps1` | packaging scripts | v1.1.12 package built |
| GitHub release | `tools/Publish-GitHubRelease.ps1` | release script | v1.1.12 published |
| Only-current channel | Delete old release/tag after verification | release operator procedure | v1.1.12 only current remote release/tag |
| BOM-free update feed | `dist/latest.json` | packaging/update code | Feed starts `{`, no BOM, SHA matches zip |
| Engineering continuity | `/engineering` registries | markdown registries | Current release/readiness/checkpoints recorded |
| Crash-isolated plugins | Bridge process | `ferric-plugin-bridge` | Bridge crash test passes |

---

## 4. Test and evidence map

### Mandatory gates

```powershell
cd C:\Users\ndrpe\Projects\FerricDAW
$env:RUSTFLAGS='-D warnings'
cargo check --workspace --all-targets
Remove-Item Env:RUSTFLAGS
cargo test --workspace
powershell -ExecutionPolicy Bypass -File tools\FunctionTest-Windows.ps1
```

Current v1.1.12 evidence:

- Warning-denied workspace check: PASS.
- Workspace tests: PASS.
- Windows function test: PASS, 59/59.
- Visual loop: default arrangement, Samples browser, Presets/Plugins browser;
  track-header controls contained and empty launcher region quiet.
- Release feed: v1.1.12, BOM-free, SHA-256
  `df47ae3c82ec0f1afbb92b629bcf6439a06c83597db79d408b80e3d8a871f91b`.

### Test ownership

| Test surface | Primary files |
|---|---|
| Rust unit/integration | `crates/*/src`, `crates/*/tests` |
| Button/action sequence | `crates/ferric-app/src/function_test.rs` |
| Theme invariants | `crates/ferric-app/src/theme.rs` tests |
| Plugin crash isolation | `crates/ferric-engine/tests/plugin_bridge_sandbox.rs` |
| Release packaging | `tools/Package-WindowsRelease.ps1`, publish script |
| Visual evidence | `engineering/PROGRESS.md`, `visual-check-session-1.1.7.ferric` |

### Accepted limits

- No automated egui pixel/widget hit-test suite; I-05 remains open.
- No 8-hour audio/session soak harness.
- True ASIO is not shipped in the portable build.
- Mac/Linux portable packaging is out of scope.
- A passing handler test does not prove every visual path or every tutorial
  action is complete.

---

## 5. AI execution guide

When an AI receives a FerricDAW task:

1. Read this file, `HANDOFF_CODEX.md`, `PROGRESS.md`, `ISSUE_REGISTRY.md`, and
   `RELEASE_READINESS.md`.
2. Run `git status -sb`, `git log -8 --oneline`, and verify the current version.
3. Classify the request as UI, DSP/instrument, model/engine, packaging, or docs.
4. Locate the authoritative module in the matrix above; do not patch a demo or
   generated artifact when a source module exists.
5. Record any new flaw in `ISSUE_REGISTRY.md` before or with the fix.
6. Implement the smallest complete change, preserving Pad A/B chrome and the
   Windows-only product boundary.
7. Add or extend a real test/function-test case for user-visible behavior.
8. Run the mandatory gates and a visual pass on the relevant packaged surface.
9. Update the registries and checkpoint with exact evidence.
10. Only publish after the release feed, zip hash, and only-current channel are
    verified.

Never claim “perfect,” “frontier,” “production,” or “complete” beyond the
specific gates and surfaces actually verified.

---

## 6. Human quick-start guide

1. Download the current v1.1.12 Windows release and unzip it.
2. Keep `ferricdaw.exe` beside `ferric-plugin-bridge.exe`.
3. Launch FerricDAW and choose **🎓 Tutorial** for Night Current, or use an
   empty session.
4. Use **Browser** to open Hardware Rig, Samples, or Presets/Plugins.
5. Build a track with **+ Track** or the palette; use Pad A/B for performance.
6. Arrange clips/scenes in the center, edit MIDI in the piano roll, and use the
   lower dock for FX, Kaoss, Launch, or Mix.
7. Bounce with Ctrl+B; inspect loudness/true peak and Golden Bounce when useful.
8. Save a `.ferric` project. Dirty close will offer Save, Discard, or Cancel.
9. Use Ferric → Check for Updates only when you want the portable updater.

### If something looks wrong

| Symptom | First check | Likely boundary |
|---|---|---|
| Audio silent | Hardware Rig device, driver, monitor, meters | Device permissions or unavailable hardware |
| MIDI absent | USB MIDI Rescan, track input, monitor | No controller connected |
| Plugin fails | Audition/quarantine status | Plugin crash is isolated by design |
| Button looks different | Compare to Pad A/B chrome | New UI code bypassed `StudioUiExt` |
| Update rejected | Feed URL, SHA, GitHub HTTPS | Updater intentionally rejects unsafe feeds |
| ASIO unavailable | Status note and selected driver | Portable build honesty fallback |
| Session feels empty | Empty launchers are intentionally quiet | Hover/click creates a sketch loop |

---

## 7. Next-work register

1. Keep the current release channel stable unless a verified defect requires a
   new version.
2. Expand visual review beyond the already inspected surfaces without claiming
   exhaustive pixel coverage.
3. Add deeper function coverage for Launch Pad, Mix ABCD, and plugin editor
   lifetime where it provides real defect detection.
4. Add real instrument work only when it includes DSP, UI route, preset, and
   tests; no placeholder instruments.
5. Do not add cloud accounts, liquid-glass styling, Mac/Linux packaging, or
   unrelated product categories.

---

## 8. Release handoff checklist

- [ ] `git status` clean or every dirty file intentionally documented.
- [ ] Version agrees across `Cargo.toml`, `Cargo.lock`, README, devlog, and
      readiness docs.
- [ ] `cargo check --workspace --all-targets` with `-D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] Windows function test passes with every new action included.
- [ ] Packaged/release UI visually inspected on the changed surfaces.
- [ ] Zip and `latest.json` downloaded from the remote release and hash-matched.
- [ ] Prior release/tag removed only after the new release is verified.
- [ ] `/engineering` updated with the exact commit, tag, URL, and artifact hash.

