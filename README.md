# FerricDAW

**Ultra-lightweight, open-source Hybrid Digital Audio Workstation written in 100% safe Rust.**

FerricDAW takes **REAPER's** legendary universal-track philosophy, featherweight footprint, and home-recording take-comping workflow — and upgrades it with a modern 60 FPS GPU vector UI, **Ableton/Bitwig-style Session Clip Launching**, **track-level LFO/Envelope-Follower Modulators**, **Always-Listening Retroactive Capture**, and a **zero-dependency pure-Rust studio instrument & FX suite**.

---

## Highlights

- **REAPER "Track = Anything" Universal Graph (`ferric-model`):**
  - Every track simultaneously supports stereo/mono audio clips, MIDI clips, multi-take comping lanes, folder bus submixing, and arbitrary cycle-checked Pre-FX / Pre-Fader / Post-Fader sends.
  - **Swipe Take Comping:** Loop-record or stack multiple takes on any clip and drag across lanes to build seamless crossfaded vocal or guitar comps.
  - **Per-Clip FX & Envelopes:** Non-destructive slip editing, clip gain, varispeed transpose, shaped fades, and per-clip FX inserts.
  - **Git-Friendly Project Persistence:** Clean, human-readable `.ferric` (RON) and `.json` project files with `Arc`-shared audio buffers for instant `< 1 ms` Undo/Redo snapshots.
- **Zero-Allocation Real-Time Audio Engine (`ferric-engine`):**
  - Native low-latency audio/MIDI I/O via `cpal` & `midir` paired with wait-free SPSC ring buffers (`rtrb`).
  - **Always-Listening Retroactive Capture (`Shift+R`):** Continuously buffers incoming MIDI & audio so you never lose an unrecorded riff or jam.
  - **Faster-Than-Realtime Offline Bounce:** Export 16-bit, 24-bit, or 32-bit float Master WAV mixes or individual multi-track stems.
- **Built-In Pure-Rust Studio Suite (`ferric-dsp`):**
  - **Instruments:** 16-Voice PolyBLEP VA / 2-Op FM `PolySynth` (Cytomic ZDF SVF filter) & 8-Pad Acoustic/Analog `Studio Drum Kit`.
  - **Home-Recording Essentials:** Sub-cent YIN `Chromatic Strobe Tuner`, Hysteresis `Noise Gate`, `Tube Amp & Speaker Cab Modeler` (1x12 Tweed, 2x12 Brit, 4x12 Modern, Bass DI), `4-Band Parametric EQ + 64-Band FFT Spectrum`, Soft-Knee `Compressor`, `Oxide Tape Saturator`, Tempo-Synced `Ping-Pong Delay`, and Lush Stereo `Plate/Hall Reverb`.
  - **Metering:** ITU-R BS.1770 K-weighted Momentary & Short-Term **LUFS**, Stereo Phase Correlation, and Peak/RMS meters.
- **Single-Window GPU Vector Interface (`ferric-app`):**
  - Instant **`Ctrl+K` Command & FX Palette**, hybrid Session Launcher + Linear Arrangement, inline Piano Roll + Waveform Comp Inspector, Routing Matrix crosspoint grid, and built-in Computer Keyboard Musical Typing (`A..K` notes, `Z/X` octave).

---

## Quick Start

```bash
cargo run --release
```

### Run Test Suite

```bash
cargo test --workspace
```

---

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Space` | Play / Stop Transport |
| `Ctrl+K` | Open Instant Action & FX Command Palette |
| `Shift+R` | Capture Last 15s from Retroactive Audio/MIDI Buffer |
| `A W S E D F T G Y H U J K` | Musical Typing Keyboard (`Z` / `X` shifts octave) |
| `Ctrl+T` | Add New Universal Track |
| `Ctrl+D` | Duplicate Selected Clip |
| `Ctrl+B` | Offline Bounce Master WAV / Multi-Track Stems |
| `Ctrl+S` / `Ctrl+O` | Save / Open `.ferric` or `.json` Session |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |

---

## License

Dual-licensed under either **MIT** ([LICENSE-MIT](LICENSE-MIT)) or **Apache-2.0** ([LICENSE-APACHE](LICENSE-APACHE)) at your option.
