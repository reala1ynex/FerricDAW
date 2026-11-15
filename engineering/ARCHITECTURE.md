# Architecture

**Last updated:** 2026-11-15

```
ferricdaw.exe (ferric-app)
  ├─ egui/eframe UI (main, browser, arrangement, piano_roll, theme, update)
  ├─ ferric-model   — Project, clips, FX graph, SMF, theory, serialization
  ├─ ferric-dsp     — Processors (EQ, synth, dynamics, HostDNA, etc.)
  ├─ ferric-engine  — Realtime audio/MIDI, WASAPI/ASIO host resolve, bounce, plugin host client
  └─ ferric-plugin-bridge.exe — Out-of-process CLAP/VST3 sandbox
```

## Data / control flow

- UI mutates `Project` on the main thread; snapshots for undo.
- Engine receives sync/commands via lock-free rings; telemetry polled each frame.
- Capture inbox delivers recorded MIDI/audio takes back to UI.
- Bounce runs offline render path in engine with mastering options.
- Updater: HTTPS fetch `latest.json` → validate host+sha256 → download zip → PowerShell apply after quit.

## Persistence

- Project bundles (`.ferric` / JSON+assets).
- Autosave recovery under app data / install-relative path (see `maybe_autosave`).
- `update_feed.url` beside exe; preferences via eframe persistence where enabled.

## Packaging

- `tools/Package-WindowsRelease.ps1` → zip + BOM-free `latest.json`.
- `tools/Publish-GitHubRelease.ps1` / tag CI job uploads assets.
- Single current Windows x64 channel (v1.1.5).
