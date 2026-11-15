# FerricDAW Devlog

Notes on what shipped and when. Newest first.

**Owner:** Patrick Rudolph ([@reala1ynex](https://github.com/reala1ynex))  
**Repo:** [github.com/reala1ynex/FerricDAW](https://github.com/reala1ynex/FerricDAW)  
**License:** MIT OR Apache-2.0

---

## 2026-11-15 — v1.1.2

Current public release.

- Portable Windows zip with in-app **Check for Updates** (`latest.json` feed on GitHub Releases)
- Clearer Night Current tutorial (numbered steps, “Do this step for me”)
- Docs rewrite — README, beginner guide, feature guide, F1 help (less jargon)
- Track types (Vocal / MIDI / Instrument / Audio), playhead scrub, piano-roll edit polish
- Kaoss / XY as live instrument + FX control
- Packaging scripts for zip + GitHub release upload

Grab it: [Releases · v1.1.2](https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.2)

---

## 2026-11-14 — v1.1.1

- Night Current in-app tutorial + `demo/NightCurrent-Demo.wav`
- Plugin bridge sandbox as the default crash path
- Hardware Rig, dual keyboard pads, Line-In helpers
- First-run chooser (tutorial / Afterglow / empty)

---

## 2026-11 — v1.1.0

- FerricWave (wavetable + granular)
- Pitch editing blobs, convolution IR, OTT, vectorscope
- Groove / Velocity DNA, HostDNA, Chord Storyboard
- Golden Bounce, Scene Launch Pad, Plugin Playground

---

## 2026-11 — v1.0.0

- First “ship it” cut: icon, version info, F1 quickstart
- Clip glue, recent sessions, CLI open
- CI packaging workflow

---

## Earlier milestones (v0.2 → v0.6)

Built up in order:

- Core hybrid tracks, WSOLA warp, vocal auto-pitch, sampler, automation
- Multitrack MIDI + audio record, drum grid, arp, autosave
- CLAP + VST3 hosting with real params, state, floating editors
- FLAC bounce, −14 LUFS, true-peak limiter, latency ping
- Tempo/meter conductor, marquee edit, auto-crossfades

---

## What’s next (loose)

Whatever gets used: tighter tutorial feedback, more honest I/O status, and keeping the portable updater boring and reliable.
