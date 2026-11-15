# FerricDAW

A small hybrid DAW written in Rust. Tracks hold audio, MIDI, instruments, and FX. Session clips, CLAP/VST3 plugins (crash-isolated), and a portable Windows build you can unzip and run.

**Current version:** [1.1.7](https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.7)
**Owner:** Patrick Rudolph ([@reala1ynex](https://github.com/reala1ynex))  
**Devlog:** [`docs/DEVLOG.md`](docs/DEVLOG.md) · **Authors:** [`AUTHORS.md`](AUTHORS.md)

---

## Get the app (Windows)

1. Grab **v1.1.7** from [Releases](https://github.com/reala1ynex/FerricDAW/releases/latest) (current only).
2. Unzip somewhere permanent.
3. Run `ferricdaw.exe`.
4. Keep `ferric-plugin-bridge.exe` next to it — that’s the plugin sandbox.

On first launch you’ll get three choices: **Night Current** tutorial, **Afterglow Copper** playground, or an empty session.

Updates: in the app, **Ferric → Check for Updates**. It downloads the portable zip and applies it on the next restart.

---

## Try the tutorial first

**Night Current** is a short dark-techno track (128 BPM, Cm–Ab–Eb–Bb).

- In the app: **🎓 Tutorial** (or Ctrl+K → Night Current)
- Written steps: [`docs/BEGINNER_GUIDE.md`](docs/BEGINNER_GUIDE.md)
- Target bounce: `demo/NightCurrent-Demo.wav`

Play the demo WAV once so you know what you’re aiming for.

Everything else (DNA groove, HostDNA, Launch Pad, plugin audition, hardware rig, etc.) is in [`docs/FEATURE_GUIDE.md`](docs/FEATURE_GUIDE.md) and **F1** inside the app.

---

## What you can do

- **Tracks** — audio, MIDI, instruments, folders, session slots on the same track
- **Plugins** — CLAP and VST3 with floating UIs; bad plugins die in the bridge, not the DAW
- **Editing** — piano roll, drum grid, warp/stretch, chord lane, tempo/meter map
- **Mixing** — mixer, VCA groups, Mix A/B/C/D, sidechain, freeze, PDC
- **Bounce** — WAV / FLAC / MP3 / OGG with −14 LUFS and true-peak limiting
- **Hardware** — USB MIDI + Line-In, Kaoss/XY pad, dual keyboard pads
- **Extras** — Groove/Velocity DNA, HostDNA synth, scene Launch Pad, Golden Bounce A/B

---

## Line-In tips

Speakers out, External Mic / Line-In in. Set Windows **Mic Boost to 0%** and use Ferric’s input trim instead — Mic Boost adds hiss.

```powershell
powershell -ExecutionPolicy Bypass -File tools\SetLineInCapture.ps1
```

---

## Build from source

```powershell
cargo run --release
cargo build --release -p ferric-engine --bin ferric-plugin-bridge

# Portable zip + latest.json for the updater
powershell -ExecutionPolicy Bypass -File tools\Package-WindowsRelease.ps1

# Optional: GitHub release upload
powershell -ExecutionPolicy Bypass -File tools\Publish-GitHubRelease.ps1
```

```powershell
cargo test --workspace
```

---

## Docs

| File | What’s in it |
|------|----------------|
| [`docs/BEGINNER_GUIDE.md`](docs/BEGINNER_GUIDE.md) | Build Night Current step by step |
| [`docs/FEATURE_GUIDE.md`](docs/FEATURE_GUIDE.md) | Full feature map |
| [`docs/NIGHT_CURRENT_DEMO.md`](docs/NIGHT_CURRENT_DEMO.md) | Notes on the demo WAV |
| [`docs/DEVLOG.md`](docs/DEVLOG.md) | What shipped, by version |
| [`AUTHORS.md`](AUTHORS.md) | Owner / contact |
| In-app **F1** | Quickstart, feature guide, shortcuts |

---

## Owner

FerricDAW is made by **Patrick Rudolph** ([@reala1ynex](https://github.com/reala1ynex)).  
Bugs and ideas: [GitHub Issues](https://github.com/reala1ynex/FerricDAW/issues).

---

## License

MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache-2.0 ([LICENSE-APACHE](LICENSE-APACHE)), your choice.  
Copyright © 2026 Patrick Rudolph.
