# Feature Registry

**Last updated:** 2026-11-15 · **App version:** 1.1.12

Status legend: **Complete+Verified** | **Implemented** | **Partial** | **Broken** | **Missing** | **Blocked**

| ID | Feature | Req | Status | Evidence |
|----|---------|-----|--------|----------|
| F-01 | Arrangement timeline / clips / tracks | R-01 | Complete+Verified | `arrangement.rs`, model tests, function-test |
| F-02 | Transport + recording | R-02 | Complete+Verified | engine + function-test transport cases |
| F-03 | Project I/O | R-03 | Complete+Verified | model roundtrip + File menu handlers |
| F-04 | Autosave / recover | R-04 | Complete+Verified | `maybe_autosave` banners on failure (I-08) |
| F-05 | Dirty close / destructive confirm | R-05 | Complete+Verified | `PendingDestructive::QuitApp` + CancelClose (I-01) |
| F-06 | Bounce modal | R-06 | Complete+Verified | function-test bounce path; mastering tests |
| F-07 | Piano roll / MIDI notes | R-07 | Complete+Verified | function-test MIDI quantize/delete; Backspace path |
| F-08 | SMF import/export | R-08 | Complete+Verified | import dialog; export banners (I-09); model SMF test |
| F-09 | WASAPI Shared/Exclusive | R-09 | Complete+Verified | `audio_backend.rs` + honesty note UI |
| F-10 | ASIO picker | R-10 | Complete+Verified | labels + fallback note (I-03) |
| F-11 | USB MIDI / typing / learn | R-11 | Complete+Verified | function-test MIDI rescans; learn arm |
| F-12 | Plugin bridge CLAP/VST3 | R-12 | Implemented | integration test downloads real plugs when net available |
| F-13 | Tutorial / demo / first-run | R-13 | Complete+Verified | function-test tutorial + demo WAV |
| F-14 | Portable updater | R-14 | Complete+Verified | `update.rs` validate_feed + unit tests (I-02) |
| F-15 | Kaoss XY Write automation | R-15 | Complete+Verified | browser Kaoss Write → FxParam |
| F-16 | Hardware Rig presets | R-16 | Complete+Verified | function-test spawn cases |
| F-17 | DNA / HostDNA / Launch / Mix ABCD | R-17 | Implemented | UI wired; not every sub-control in function-test |
| F-18 | Versioned docs | R-18 | Complete+Verified | README 1.1.5 |
| F-19 | Function-test harness | R-19 | Complete+Verified | `function_test.rs` + CI Windows step (I-07) |
| F-20 | Engineering registries | R-20 | Complete+Verified | this directory |

## Incomplete / blocked

| ID | Notes |
|----|-------|
| F-12 deeper | Full interactive plugin UI soak not automated |
| F-17 depth | Scene Launch Pad edge cases not exhaustively function-tested |
| Mac/Linux ship | N-06 — advisory CI only |

## Current local development build: 1.1.13

F-06 now uses a bundled Ogg Vorbis encoder and an external FFmpeg with
libmp3lame for MP3. On a system without that dependency, MP3 export reports a
failure; WAV, FLAC and Ogg do not depend on FFmpeg. End-user acceptance is
partial. See `CANDIDATE_VERIFICATION.md` and I-26 for current evidence.
