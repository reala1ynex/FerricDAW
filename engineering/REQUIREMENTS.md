# Requirements Registry

**Project:** FerricDAW · **Version under test:** 1.1.5  
**Last updated:** 2026-11-15

Traceability: Requirement → Feature (`FEATURE_REGISTRY.md`) → Test (`TEST_REGISTRY.md`) → Issue (`ISSUE_REGISTRY.md`).

## Functional

| ID | Requirement | Priority | Status |
|----|-------------|----------|--------|
| R-01 | Hybrid timeline: audio + MIDI tracks, clips, takes | P0 | Verified |
| R-02 | Transport: play/stop/record, loop, metronome | P0 | Verified |
| R-03 | Project save/load (`.ferric` / JSON bundle) | P0 | Verified |
| R-04 | Autosave + recovery snapshot | P1 | Verified |
| R-05 | Dirty-session guard on destructive actions and window close | P0 | Verified |
| R-06 | Bounce master/stems (WAV/FLAC/MP3/OGG) with LUFS/TP | P0 | Verified |
| R-07 | Piano roll + MIDI edit (draw, quantize, delete Backspace) | P0 | Verified |
| R-08 | Import/export Standard MIDI File | P1 | Verified |
| R-09 | WASAPI Shared/Exclusive audio I/O with honest fallback messaging | P0 | Verified |
| R-10 | ASIO selection attempts host; portable build falls back with honesty note | P1 | Verified |
| R-11 | USB MIDI input + musical typing + MIDI learn | P1 | Verified |
| R-12 | CLAP/VST3 plugins via crash-isolated bridge | P1 | Verified |
| R-13 | Night Current tutorial + Afterglow demo + first-run | P1 | Verified |
| R-14 | Portable Windows zip + in-app updater (SHA-256 + GitHub HTTPS) | P0 | Verified |
| R-15 | Kaoss/XY Write → FX param automation | P2 | Verified |
| R-16 | Hardware Studio Rig presets | P2 | Verified |
| R-17 | Groove/Velocity DNA, HostDNA, Launch Pad, Mix A–D | P2 | Implemented |
| R-18 | Docs: README, BEGINNER, FEATURE, DEVLOG, AUTHORS match shipped version | P2 | Verified |
| R-19 | Automated regression: `cargo test --workspace` + Windows function-test | P0 | Verified |
| R-20 | Persistent `/engineering` registries and checkpoints | P1 | Verified |

## Nonfunctional

| ID | Requirement | Priority | Status |
|----|-------------|----------|--------|
| N-01 | Release build strips symbols; panic=abort (Windows subsystem) | P1 | Verified |
| N-02 | No UTF-8 BOM on `latest.json` / `update_feed.url` | P0 | Verified |
| N-03 | Updater refuses missing SHA-256 and non-GitHub HTTPS hosts | P0 | Verified |
| N-04 | Plugin crashes must not kill host (bridge process) | P0 | Verified (architecture) |
| N-05 | CI: Windows required; Mac/Linux advisory (`continue-on-error`) | P1 | Verified |
| N-06 | Mac/Linux portable packaging out of scope for current channel | — | Blocked / deferred |

## Explicitly out of scope (do not invent)

- Native macOS/Linux installers and signed notarization
- True ASIO in portable zip without ASIO SDK / cpal `asio` feature
- Cloud sync, accounts, DRM
- Liquid-glass / redesign of Pad A/B chrome
