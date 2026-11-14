# FerricDAW v0.2

**Full-Fledged, Ultra-Lightweight Open-Source Hybrid Digital Audio Workstation in 100% Pure Rust.**

FerricDAW combines **REAPER's** universal "Track = Anything" philosophy, multi-take swipe comping, and tiny native footprint with **Bitwig/Ableton** hybrid session launching, modulators, **WSOLA time-stretching**, **Scale-Aware Vocal Auto-Pitch**, **Bezier automation**, **CLAP plugin hosting + Automatic PDC**, and dedicated **Akai LPK25 + Nintendo DS (KORG DS-10)** hardware integration.

---

## Studio Feature Set

- **REAPER Universal Track Graph + Pro Mixing (`ferric-model` & `ferric-engine`):**
  - Universal tracks simultaneously hold Audio, MIDI, Folder Submix children, Session Clips, Instruments, Audio FX, Modulators, and Pre-FX / Pre-Fader / Post-Fader Aux Sends.
  - **True External Sidechain Routing:** Key any `Compressor` or `Noise Gate` off any other track (e.g. Kick ducking Sub Bass) with topological DAG cycle protection.
  - **1-Click Reversible Track Freeze (`❄` / `Ctrl+F`):** Renders heavy Instrument + FX chains faster-than-realtime to a 32-bit float stem, dropping live DSP CPU to `0.0%` while remaining 1-click reversible.
  - **Automatic Plugin Delay Compensation (PDC):** Sample-accurate delay alignment across parallel tracks for lookahead/pitch-shifting processors and external plugins.
- **WSOLA Time-Stretching, Warp Markers & Vocal Auto-Pitch (`ferric-dsp`):**
  - **Decoupled WSOLA Time-Stretch & Pitch-Shift:** Stretch audio loops (`0.25x`–`4.0x`) or transpose (`±24 st`) with zero chipmunk speed coupling, plus automatic transient onset detection and draggable **Stretch/Warp Markers** directly on the waveform.
  - **Scale-Aware Vocal Auto-Pitch Corrector:** Real-time sub-cent YIN pitch tracking + dual-grain Hann pitch shifting (`0 ms` hard snap to `160 ms` natural polish + vibrato humanize).
  - **16-Voice Chromatic & Transient-Slice Sampler:** Play any `WAV`/`FLAC`/`MP3`/`OGG`/`AIFF` chromatically across 128 keys or auto-slice drum breaks across pads (`C2+`).
- **Curved Bezier Automation, SMF `.MID` & 1-Click MIDI Learn:**
  - Foldable per-track Bezier automation lanes (`Shift+Drag` adjusts exponential/logarithmic curve tension) with `Read`, `Touch`, `Latch`, and `Write` modes.
  - Full **Standard MIDI File (`.mid`) Import & Export**, Piano Roll **Scale Lock**, **1-Click Chord Stamps** (`Maj7`, `Min9`, `Dom7`, `Sus4`, `Power5`), **Humanize**, and **MIDI CC / Pitch-Bend Lanes**.
- **Open-Standard `.clap` Plugin Hosting & Multi-Codec Audio:**
  - Built-in C-ABI `.clap` bundle scanner & stereo/MIDI host (`CommonProgramFiles\CLAP` + custom folders).
  - Native drag-and-drop decoding for **`WAV`, `FLAC`, `MP3`, `OGG/Vorbis`, `AIFF`, and `.MID`** via pure-Rust `symphonia` & `midly`.
- **Akai LPK25 + Nintendo DS (KORG DS-10) Hardware Studio Integration:**
  - Multi-port USB MIDI hot-plugging auto-detects the **Akai LPK25** (including `CC64` Sustain button & MIDI Learn).
  - Universal stereo `F32`/`I16` Line-In stream with real-time sample-rate conversion, Input Gain Trim, **Tap Tempo**, **1-Click LPK25 + DS-10 Hybrid Rig Spawner**, and a **KORG DS-10 inspired Touch KAOSS X-Y Pad**!

---

## Build & Run

```powershell
cargo run --release
```

### Run Workspace Tests

```powershell
cargo test --workspace
```

---

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Space` | Play / Stop Transport |
| `Ctrl+K` | Open Instant Action, Track & FX Command Palette |
| `Shift+R` | Capture Last 15s from Always-Listening Retroactive Audio/MIDI Buffer |
| `Ctrl+F` | Freeze / Unfreeze Selected Track (`0.0%` DSP CPU 32-bit Stem) |
| `A W S E D F T G Y H U J K` | Computer Keyboard Musical Typing (`Z` / `X` shifts octave) |
| `Ctrl+T` | Add New Universal Track |
| `Ctrl+D` | Duplicate Selected Clip |
| `Ctrl+B` | Offline Bounce Master WAV / Multi-Track Stems |
| `Ctrl+S` / `Ctrl+O` | Save / Open `.ferric` (Git-friendly RON) or `.json` Session |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |

---

## License

Dual-licensed under either **MIT** ([LICENSE-MIT](LICENSE-MIT)) or **Apache-2.0** ([LICENSE-APACHE](LICENSE-APACHE)) at your option.
