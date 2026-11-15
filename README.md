# FerricDAW v1.1.1

**Full-Fledged, Ultra-Lightweight Open-Source Hybrid Digital Audio Workstation in 100% Pure Rust.**

FerricDAW combines **REAPER's** universal *"Track = Anything"* architecture, multi-take swipe comping, and a tiny native standalone binary with **Bitwig/Ableton** hybrid session clip launching, modulators, **WSOLA time-stretching**, **Scale-Aware Vocal Auto-Pitch**, **Bezier automation**, **full `.CLAP` + `.VST3` plugin hosting** (in-process editors + **out-of-process sandbox**), **Lossless `FLAC` / Lossy `MP3`/`OGG` + `-14 LUFS` Mastering**, **VCA console groups**, **Chord Lane songwriter**, **Dynamic Tempo/Meter Conductor Maps**, and dedicated **Akai LPK25 + Nintendo DS (KORG DS-10)** hardware integration.

**Tutorial song:** **Night Current** — in-app **🎓 Tutorial** coach (also Ferric menu / Ctrl+K). Written twin: `docs/BEGINNER_GUIDE.md`. Finish line: `demo/NightCurrent-Demo.wav` (128 BPM dark techno, Cm–Ab–Eb–Bb). Factory session **Afterglow Copper** is a separate playground.

---

## Windows portable (send this out)

Unzip `FerricDAW-v1.1.1-win-x64.zip` and run **`ferricdaw.exe`**. Keep `ferric-plugin-bridge.exe` next to it — **plugins that crash run in a worker, not the DAW**. First launch offers **Night Current Tutorial / Afterglow playground / Empty**. Play **`demo/NightCurrent-Demo.wav`** (or **A/B vs demo** after bounce) — that’s the tutorial finish line.

Optional DS Line-In helper (Realtek External Mic, Mic Boost off):

```powershell
powershell -ExecutionPolicy Bypass -File tools\SetDs10Capture.ps1
```

**DS tip:** Speakers out + External Microphone in. Windows **Mic Boost = 0%**. Turn up the DS headphone volume; use Ferric **+6 dB** trim — Mic Boost causes hiss.

---

## Complete v1.1 Studio Feature Set

1. **REAPER Universal Track Graph & Pro Mixing (`ferric-model` & `ferric-engine`):**
   - Every track simultaneously supports Audio clips, MIDI clips, Folder Submix children, Session Launcher loops, Instruments, Audio FX, Modulators, and Pre-FX / Pre-Fader / Post-Fader Aux Sends.
   - **True External Sidechain Routing** (`Compressor` & `Noise Gate` keyed off any track with topological DAG cycle protection).
   - **1-Click Reversible Track Freeze (`❄` / `Ctrl+F`)** & **Automatic Plugin Delay Compensation (PDC)**.
   - **Equal-Power Auto-Crossfades** whenever audio clips overlap + **Rubber-Band Marquee Multi-Clip Selection** (`Shift+Click` or drag box to group-move, duplicate, or delete).
2. **Dynamic Song Conductor (`🎼 Tempo / Meter Map`):**
   - Foldable Master Conductor Lane supporting stepped & linear **Accelerando / Ritardando Tempo Ramps** (with exact closed-form logarithmic beat↔second integration) and mid-song **Time Signature Changes** (`4/4`, `3/4`, `6/8`, `7/8`, `5/4`, `12/8`).
3. **Full `.CLAP` & `.VST3` Plugin Ecosystem (`clap_host.rs`, `vst3_host.rs`, `plugin_window.rs`):**
   - Battle-tested pure-Rust C-ABI `.clap` and COM `.vst3` scanners & multi-bus real-time hosts (verified end-to-end against **Dexed** and **Six Sines** in both `.clap` and `.vst3`).
   - Real plugin parameter discovery (`clap.params` / `IEditController`), Base64 binary patch state save/reload (`clap.state` / `IBStream`), floating native Win32 **`HWND` Graphical Editors (`🗔 Open UI`)**, and automatic **Crash Quarantine**.
4. **WSOLA Warp, Vocal Auto-Pitch & 16-Voice Slice Sampler (`ferric-dsp`):**
   - Decoupled WSOLA time-stretch (`0.25x`–`4.0x`) & pitch-shift (`±24 st`), automatic transient detection, and draggable **Stretch/Warp Markers**.
   - **Scale-Aware Vocal Auto-Pitch Corrector**, **Chromatic & Transient-Slice Sampler**, `PolySynth`, `8-Pad Studio Drum Kit`, `Chromatic Strobe Tuner`, `Tube Amp & Cab Modeler`, `4-Band Parametric EQ + 64-Band FFT`, `Compressor`, `Noise Gate`, `Oxide Tape Saturator`, `Ping-Pong Delay`, and `Lush Plate Reverb`.
5. **Curved Bezier Automation, 16-Step Drum Grid, Arpeggiator & SMF `.MID`:**
   - Foldable Bezier automation lanes (`Read / Touch / Latch / Write`), 1-Click **MIDI CC Learn**, **Track MIDI Arpeggiator (`🎹 ARP`)**, **TR-808 / DS-10 16-Step Drum Grid**, Piano Roll **Scale Lock & Chord Stamps**, and `.MID` Import/Export.
6. **Low-Latency Calibration & Mastering Suite (`flac_enc.rs`, `wav_io.rs`):**
   - Selectable hardware buffer sizes (`64`–`1024` samples), **1-Click `📡 Loopback Ping` Round-Trip Latency Calibrator** + automatic overdub trim.
   - Pure-Rust RFC 9639 **Lossless Stereo `FLAC` (16/24-bit + MD5)** & **`WAV` (16/24/32f)** export with **ITU-R BS.1770-4 Integrated `-14 LUFS` Normalization**, **4x True-Peak (`dBTP`) Lookahead Limiter**, and **Triangular `TPDF` Dither**.
7. **Akai LPK25 + Nintendo DS (KORG DS-10) Hardware Rig:**
   - Multi-port USB MIDI hot-plugging (`CC64` Sustain, Pitch-Bend, Mod Wheel), simultaneous LPK25 MIDI + DS-10 Stereo Line-In multitrack recording, automatic Line-In BPM detector, **Tap Tempo**, and an interactive **KORG DS-10 KAOSS X-Y Pad**.
8. **Milestone 9 — Studio Console & Harmony Arranger:**
   - **VCA Groups** (engine-wired gain/mute/solo) + **Mix A/B/C/D** recall, interactive **Chord Lane** + Circle of Fifths / Progression presets, **Generate Arrangement**, **Chord-Follow** MIDI snap, and offline **4-stem HPSS split**.
9. **Milestone 10 — Pro I/O & Crash-Safe Plugins:**
   - **WASAPI Shared / Exclusive / ASIO** driver selector, **16-channel hardware I/O matrix**, default **out-of-process `ferric-plugin-bridge` sandbox** for CLAP/VST3, and **MP3 / OGG** mastering bounce.
10. **v1.1.1 polish:** Stable maximized window, sticky resizable lower dock, DS capture gain staging (Mic Boost off / +6 dB), and UI FPS fix (no per-frame WASAPI device scans).

---

## Build & Run

```powershell
cargo run --release
# Optional: place the sandboxed plugin worker next to the DAW binary
cargo build --release -p ferric-engine --bin ferric-plugin-bridge

# Package portable zip
powershell -ExecutionPolicy Bypass -File tools\Package-WindowsRelease.ps1
```

### Run Full Workspace & Real Plugin Verification Suite

```powershell
cargo test --workspace
```

---

## License

Dual-licensed under either **MIT** ([LICENSE-MIT](LICENSE-MIT)) or **Apache-2.0** ([LICENSE-APACHE](LICENSE-APACHE)) at your option.
