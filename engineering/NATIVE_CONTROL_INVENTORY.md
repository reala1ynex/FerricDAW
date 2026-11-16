# Native control and evidence inventory

The existing `MASTER_FEATURE_REFERENCE.md` maps capabilities to code and prior
tests. This companion groups the controls that are replayed on the workstation.
A handler result does not prove that the corresponding rendered widget is
reachable or that external hardware or software behaves as expected.

| Surface / controls | Handler and state | Engine / persistence | Evidence route |
|---|---|---|---|
| Transport play, stop, record | `render_top_transport_bar`, `toggle_play`, `toggle_record` | Transport EngineCommands, armed-track project state, captured takes | Debug function cases + real transport and capture run |
| Loop, click, count-in, auto punch, tempo, meter, tap | `render_top_transport_bar` / Project | Project sync, .ferric and recovery | Function cases; real ruler/meter and recording boundaries |
| Seek to start, bar nudges, BBT scrub and arrangement ruler/lanes | `render_top_transport_bar`, `ArrangementAction::Seek` | Engine seek, no file write | Visible playhead delta and audio position |
| Retroactive capture | Palette CaptureRetroactive | Engine capture inbox, project takes | MIDI/line input and clip result |
| DNA Feel, groove, velocity, accent, Apply drums/MIDI | `render_dna_strip` / Project | Live sync; apply snapshots undo and persists note changes | Function cases, audition and undo |
| New/Open, Save, Save As, recovery, close and confirmation | `PendingDestructive`; project dialogs, autosave and recovery | Bundle + autosave; sync after load | Rust save guard tests, all three confirm decisions via UI |
| Track add, input, monitor, M/S/R, rename, gain/pan, reorder, folders, freeze, importer | Arrangement and track-header action match, project updates | Engine graph/bus/track sync, frozen render, bundle | Function cases, narrow/deep track visual and audio run |
| Arrangement pointer/razor/draw, snap, ripple, zoom, scroll, selection, takes, glue | `ArrangementState` / clip edit actions | Project automation/takes/clips; display-only tool and view state | Model and editor tests, live geometry/hit testing |
| Session slot and scene launch, print scene, stop clips | `render_session_cells`, `launch_scene`, clip actions | Project session clips and engine schedule | Session and scene UI; quantization with transport running |
| Conductor, chord lane, circle of fifths, progression, storyboard, generate/undo | Arrangement lane handlers and theory; project conductor and chord tracks | Sync, bundle, history on Generate | Model and integration cases; demo click-through |
| Hardware I/O driver, device, input channels, buffer, monitor, loopback ping, USB rescan/learn and rig | `BrowserAction` match; engine live devices and Project | Backend resolves actual driver; MIDI mapping and input configuration | Function paths, physical device walk; ASIO fallback explicit |
| Pad A/B enable, assign, source/layout, octave, velocity | `KeyboardPadRig` and transport handler | LiveMidi messages; performance state | Handler coverage plus live key/audio walk |
| Samples filter, browse, preview, drop; factory presets | Browser handlers | Engine preview, inserts/clips, project bundle | Handler paths + visible result |
| CLAP/VST3 scan, audition, quarantine, insert and editor | Browser and DockAction matches | Bridge process, plugin state and quarantine; project chain | Workspace plugin integration test; real plugin UI soak is separate |
| MIDI editor, quantize, delete, drum grid, automation, arp, import/export | `piano_roll.rs`, shortcut handler, DockAction and SMF paths | Project notes / automation and engine sync; .mid and .ferric | Workspace/model/function suites and user edit cycle |
| FX and modulator controls, sidechain, PDC, instrument activation | `render_fx_and_modulators_panel`, DockAction | DSP/device graph, engine sync, project chain | DSP/model + factory coverage and real control/audio walk |
| Kaoss, Write XY | `KaossPadState`, `process_browser_actions` | Live MIDI + FxParam automation in project | Function case + recorded path in actual editor |
| Launch Pad scenes, quantization and morph | `render_launch_pad`, `queue_scene_launch`, `tick_launch_and_morph` | Pending launch, live mix interpolation, project snapshots | Full scene/quantization/morph UI trial still needed |
| Mix A–D store/recall, faders, pan, routing, sends, master | Mixer / routing panels and DockAction | Project mixer/recall graph and realtime sync | Model tests + real meter/routing walk |
| Bounce file/range/format/mastering/stems and Golden Bounce | `render_bounce_modal`, offline engine render and analysis | Output files, loudness/peak data, matched reference preview | Workspace bounce tests + packaged export/compare |
| Tutorial, Afterglow, Help/F1, command palette | Tutorial action/coach, `PendingDestructive`, Help, PaletteCommand | Project rebuild/demo load and display state | Handler tests, native modal review and tutorial progression |
| Updater, banners and retry | `UpdateService`, modal, status banner and fallback paths | Validation, zip/apply only when requested | Feed and upgrade error tests; in-place apply is separate |
