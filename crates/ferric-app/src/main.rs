#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod arrangement;
mod mixer;
mod palette;
mod piano_roll;
mod theme;

use arrangement::{ArrangementAction, ArrangementState};
use eframe::egui;
use egui::{Color32, Context, Key, Stroke};
use ferric_engine::{
    audio_buffer_from_capture, import_wav_file, render_all_stems_offline, render_master_offline,
    BitDepth, BounceReport, CapturedTakeEvent, EngineCommand, EngineHandle,
};
use ferric_model::{
    build_demo_project, ClipId, FxKind, FxSlot, HistoryManager, InputSource, ModulatorSlot,
    Project, Take, TimelineClip, Track, TrackId,
};
use mixer::{render_mixer_console, render_routing_matrix};
use palette::{CommandPalette, PaletteCommand};
use piano_roll::{render_fx_and_modulators_panel, DockAction, PianoRollState};
use std::collections::HashSet;
use std::path::PathBuf;
use theme::{
    apply_studio_theme, pill_toggle, ACCENT_AMBER, ACCENT_FERRIC, ACCENT_RED, ACCENT_TEAL,
    BG_DEEP, BG_ELEVATED, BG_PANEL, BORDER_SUBTLE, TEXT_DIM, TEXT_MAIN,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BottomTab {
    EditorAndComp,
    FxAndModulators,
    MixingConsole,
    RoutingMatrix,
}

struct BounceModalState {
    open: bool,
    export_stems: bool,
    sample_rate: u32,
    bit_depth: BitDepth,
    start_beat: f64,
    end_beat: f64,
    last_report: Option<Result<BounceReport, String>>,
}

struct FerricDawApp {
    project: Project,
    project_path: Option<PathBuf>,
    history: HistoryManager,
    engine: EngineHandle,
    arrangement: ArrangementState,
    piano_roll: PianoRollState,
    palette: CommandPalette,
    bottom_tab: BottomTab,
    show_bottom_dock: bool,
    inspect_master_fx: bool,
    selected_track: Option<TrackId>,
    selected_clip: Option<(TrackId, ClipId)>,
    musical_typing_enabled: bool,
    musical_typing_octave: i8,
    held_typing_keys: HashSet<Key>,
    bounce_modal: BounceModalState,
    status_banner: String,
    capture_inbox: Vec<CapturedTakeEvent>,
}

impl FerricDawApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_studio_theme(&cc.egui_ctx);

        let initial_project = build_demo_project(48_000);
        let engine = EngineHandle::start(&initial_project);
        // Rebuild demo audio buffers at exact hardware sample rate if different from 48kHz
        let project = if engine.sample_rate != 48_000 {
            build_demo_project(engine.sample_rate)
        } else {
            initial_project
        };

        let mut app = Self {
            selected_track: project.tracks.get(3).or(project.tracks.first()).map(|t| t.id),
            selected_clip: project
                .tracks
                .get(3)
                .and_then(|t| t.clips.first().map(|c| (t.id, c.id))),
            bounce_modal: BounceModalState {
                open: false,
                export_stems: false,
                sample_rate: engine.sample_rate,
                bit_depth: BitDepth::Pcm24,
                start_beat: 0.0,
                end_beat: project.project_end_beat(),
                last_report: None,
            },
            status_banner: format!(
                "Ready • Audio: {} ({} Hz) • Space to Play • Ctrl+K Command Palette • Musical Typing Active",
                engine.audio_device_name, engine.sample_rate
            ),
            project,
            project_path: None,
            history: HistoryManager::new(64),
            engine,
            arrangement: ArrangementState::default(),
            piano_roll: PianoRollState::default(),
            palette: CommandPalette::default(),
            bottom_tab: BottomTab::FxAndModulators,
            show_bottom_dock: true,
            inspect_master_fx: false,
            musical_typing_enabled: true,
            musical_typing_octave: 4,
            held_typing_keys: HashSet::new(),
            capture_inbox: Vec::new(),
        };
        app.engine.sync_project(&app.project);
        app
    }

    fn snapshot_before(&mut self, label: &'static str) {
        self.history.record(label, self.project.clone());
    }

    fn sync_engine(&mut self) {
        self.engine.sync_project(&self.project);
    }

    fn perform_undo(&mut self) {
        if let Some((lbl, prev)) = self.history.undo(self.project.clone()) {
            self.project = prev;
            self.sync_engine();
            self.status_banner = format!("Undid: {}", lbl);
        }
    }

    fn perform_redo(&mut self) {
        if let Some((lbl, next)) = self.history.redo(self.project.clone()) {
            self.project = next;
            self.sync_engine();
            self.status_banner = format!("Redid: {}", lbl);
        }
    }

    fn toggle_play(&mut self) {
        if self.engine.latest_telemetry.is_playing {
            self.engine.send(EngineCommand::Stop);
        } else {
            self.engine.send(EngineCommand::Play);
        }
    }

    fn toggle_record(&mut self) {
        let next = !self.engine.latest_telemetry.is_recording;
        // Ensure at least selected track is armed when hitting Record!
        if next && !self.project.tracks.iter().any(|t| t.record_arm) {
            if let Some(tid) = self.selected_track {
                if let Some(t) = self.project.find_track_mut(tid) {
                    t.record_arm = true;
                    self.sync_engine();
                }
            }
        }
        self.engine.send(EngineCommand::SetRecord(next));
        if next && !self.engine.latest_telemetry.is_playing {
            self.engine.send(EngineCommand::Play);
        }
    }

    fn handle_captured_takes(&mut self) {
        if self.capture_inbox.is_empty() {
            return;
        }
        let events: Vec<CapturedTakeEvent> = self.capture_inbox.drain(..).collect();
        for ev in events {
            self.snapshot_before("Commit Captured Take");
            let target_tid = self
                .project
                .tracks
                .iter()
                .find(|t| t.record_arm)
                .map(|t| t.id)
                .or(self.selected_track)
                .unwrap_or_else(|| self.project.add_track("Captured Track"));

            match ev {
                CapturedTakeEvent::MidiTake {
                    start_beat,
                    length_beats,
                    notes,
                    is_retroactive,
                } => {
                    if let Some(trk) = self.project.find_track_mut(target_tid) {
                        // Append as a new REAPER Take Lane if an overlapping MIDI clip exists!
                        if let Some(existing) = trk.clips.iter_mut().find(|c| {
                            c.is_midi() && (c.start_beat - start_beat).abs() < 0.5
                        }) {
                            let num = existing.takes.len() + 1;
                            existing
                                .takes
                                .push(Take::new_midi(format!("Take {}", num), notes));
                            existing.active_take = existing.takes.len() - 1;
                            existing.show_take_lanes = true;
                        } else {
                            let title = if is_retroactive {
                                "Retroactive MIDI Idea"
                            } else {
                                "Recorded MIDI Take"
                            };
                            let clip = TimelineClip::new_midi(title, start_beat, length_beats, notes);
                            self.selected_clip = Some((target_tid, clip.id));
                            trk.clips.push(clip);
                        }
                    }
                    self.status_banner = if is_retroactive {
                        "⚡ Retroactive MIDI Capture dropped onto timeline!".into()
                    } else {
                        "Recorded new MIDI Take Lane.".into()
                    };
                }
                CapturedTakeEvent::AudioTake {
                    start_beat,
                    length_beats,
                    sample_rate,
                    left,
                    right,
                    is_retroactive,
                } => {
                    let buf = audio_buffer_from_capture(sample_rate, left, right);
                    if let Some(trk) = self.project.find_track_mut(target_tid) {
                        if let Some(existing) = trk.clips.iter_mut().find(|c| {
                            !c.is_midi() && (c.start_beat - start_beat).abs() < 0.5
                        }) {
                            let num = existing.takes.len() + 1;
                            existing
                                .takes
                                .push(Take::new_audio(format!("Take {}", num), buf));
                            existing.active_take = existing.takes.len() - 1;
                            existing.show_take_lanes = true;
                        } else {
                            let title = if is_retroactive {
                                "Retroactive Audio Riff"
                            } else {
                                "Recorded Audio Take"
                            };
                            let clip = TimelineClip::new_audio(title, start_beat, length_beats, buf);
                            self.selected_clip = Some((target_tid, clip.id));
                            trk.clips.push(clip);
                        }
                    }
                    self.status_banner = if is_retroactive {
                        "⚡ Retroactive Audio Capture dropped onto timeline!".into()
                    } else {
                        "Recorded new Audio Take Lane.".into()
                    };
                }
            }
            self.sync_engine();
        }
    }

    fn execute_palette_command(&mut self, cmd: PaletteCommand) {
        match cmd {
            PaletteCommand::TogglePlay => self.toggle_play(),
            PaletteCommand::ToggleRecord => self.toggle_record(),
            PaletteCommand::CaptureRetroactive => {
                self.engine
                    .send(EngineCommand::CaptureRetroactive { seconds_back: 15.0 });
                self.status_banner =
                    "Requested 15s Retroactive Capture (Play Musical Typing A..K or MIDI first!)"
                        .into();
            }
            PaletteCommand::AddUniversalTrack => {
                self.snapshot_before("Add Universal Track");
                let id = self
                    .project
                    .add_track(format!("{:02} Universal Track", self.project.tracks.len() + 1));
                self.selected_track = Some(id);
                self.sync_engine();
            }
            PaletteCommand::AddSynthTrack => {
                self.snapshot_before("Add PolySynth Track");
                let id = self
                    .project
                    .add_track(format!("{:02} PolySynth", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.fx_chain.push(FxSlot::new(FxKind::PolySynth));
                    t.fx_chain.push(FxSlot::new(FxKind::StereoDelay));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddDrumTrack => {
                self.snapshot_before("Add Drum Kit Track");
                let id = self
                    .project
                    .add_track(format!("{:02} Studio Drum Kit", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.fx_chain.push(FxSlot::new(FxKind::DrumKit));
                }
                self.selected_track = Some(id);
                self.sync_engine();
            }
            PaletteCommand::AddGuitarAmpTrack => {
                self.snapshot_before("Add Guitar Amp Track");
                let id = self
                    .project
                    .add_track(format!("{:02} Guitar Amp Rig", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.input_source = InputSource::AudioMono { channel: 0 };
                    t.fx_chain.push(FxSlot::new(FxKind::ChromaticTuner));
                    t.fx_chain.push(FxSlot::new(FxKind::NoiseGate));
                    t.fx_chain.push(FxSlot::new(FxKind::AmpCabSim));
                    t.fx_chain.push(FxSlot::new(FxKind::PlateHallReverb));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::InsertFxOnSelected(kind) => {
                if let Some(tid) = self.selected_track {
                    self.snapshot_before("Insert FX");
                    if let Some(t) = self.project.find_track_mut(tid) {
                        t.fx_chain.push(FxSlot::new(kind));
                    }
                    self.bottom_tab = BottomTab::FxAndModulators;
                    self.inspect_master_fx = false;
                    self.sync_engine();
                }
            }
            PaletteCommand::AddLfoModulator => {
                if let Some(tid) = self.selected_track {
                    self.snapshot_before("Add LFO Modulator");
                    if let Some(t) = self.project.find_track_mut(tid) {
                        t.modulators.push(ModulatorSlot::new_lfo("Tempo LFO"));
                    }
                    self.bottom_tab = BottomTab::FxAndModulators;
                    self.sync_engine();
                }
            }
            PaletteCommand::SplitSelectedClipAtPlayhead => {
                if let Some((tid, cid)) = self.selected_clip {
                    let ph = self.engine.latest_telemetry.playhead_beat;
                    self.snapshot_before("Split Clip");
                    if let Some(new_id) = self.project.split_clip(tid, cid, ph) {
                        self.selected_clip = Some((tid, new_id));
                        self.sync_engine();
                    }
                }
            }
            PaletteCommand::QuantizeSelectedClip => {
                if let Some((tid, cid)) = self.selected_clip {
                    self.snapshot_before("Quantize MIDI");
                    self.project.quantize_midi_clip(tid, cid, 0.25);
                    self.sync_engine();
                }
            }
            PaletteCommand::ToggleLoop => {
                self.project.loop_enabled = !self.project.loop_enabled;
                self.sync_engine();
            }
            PaletteCommand::ToggleMetronome => {
                self.project.metronome_enabled = !self.project.metronome_enabled;
                self.sync_engine();
            }
            PaletteCommand::OpenBounceModal => {
                self.bounce_modal.end_beat = self.project.project_end_beat();
                self.bounce_modal.open = true;
            }
            PaletteCommand::SaveProject => self.save_project_dialog(),
            PaletteCommand::LoadDemoProject => {
                self.snapshot_before("Load Demo Session");
                self.project = build_demo_project(self.engine.sample_rate);
                self.selected_track = self.project.tracks.first().map(|t| t.id);
                self.sync_engine();
            }
            PaletteCommand::Undo => self.perform_undo(),
            PaletteCommand::Redo => self.perform_redo(),
        }
    }

    fn save_project_dialog(&mut self) {
        let path = self.project_path.clone().or_else(|| {
            rfd::FileDialog::new()
                .set_title("Save FerricDAW Session (Git-Friendly RON or JSON)")
                .add_filter("FerricDAW Project (.ferric)", &["ferric"])
                .add_filter("JSON Project (.json)", &["json"])
                .set_file_name("session.ferric")
                .save_file()
        });
        if let Some(p) = path {
            match self.project.save_to_file(&p) {
                Ok(()) => {
                    self.project_path = Some(p.clone());
                    self.status_banner = format!("Saved session to {}", p.display());
                }
                Err(e) => {
                    self.status_banner = format!("Save error: {}", e);
                }
            }
        }
    }

    fn open_project_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .set_title("Open FerricDAW Session")
            .add_filter("FerricDAW / JSON", &["ferric", "json"])
            .pick_file()
        {
            match Project::load_from_file(&p) {
                Ok(loaded) => {
                    self.snapshot_before("Open Project");
                    self.project = loaded;
                    self.project_path = Some(p.clone());
                    self.selected_track = self.project.tracks.first().map(|t| t.id);
                    self.selected_clip = None;
                    self.sync_engine();
                    self.status_banner = format!("Opened {}", p.display());
                }
                Err(e) => {
                    self.status_banner = format!("Open error: {}", e);
                }
            }
        }
    }

    fn import_wav_to_track(&mut self, track_id: TrackId) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Import WAV Audio File")
            .add_filter("WAV Audio", &["wav"])
            .pick_file()
        {
            match import_wav_file(&path, self.engine.sample_rate) {
                Ok(buf) => {
                    self.snapshot_before("Import WAV Clip");
                    let beats = self.project.seconds_to_beats(buf.duration_secs()).max(1.0);
                    let at = self
                        .project
                        .snap_grid
                        .snap(self.engine.latest_telemetry.playhead_beat, self.project.time_signature);
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Imported Audio")
                        .to_string();
                    if let Some(trk) = self.project.find_track_mut(track_id) {
                        let clip = TimelineClip::new_audio(name.clone(), at, beats, buf);
                        self.selected_clip = Some((track_id, clip.id));
                        trk.clips.push(clip);
                    }
                    self.sync_engine();
                    self.status_banner = format!("Imported '{}'", name);
                }
                Err(e) => {
                    self.status_banner = format!("WAV import failed: {}", e);
                }
            }
        }
    }

    fn handle_shortcuts_and_musical_typing(&mut self, ctx: &Context) {
        if ctx.wants_keyboard_input() {
            return;
        }

        let (ctrl, shift) = ctx.input(|i| (i.modifiers.command || i.modifiers.ctrl, i.modifiers.shift));

        if ctrl && ctx.input(|i| i.key_pressed(Key::K)) {
            self.palette.toggle();
            return;
        }
        if self.palette.open {
            return;
        }

        if ctrl && ctx.input(|i| i.key_pressed(Key::Z)) {
            if shift {
                self.perform_redo();
            } else {
                self.perform_undo();
            }
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::Y)) {
            self.perform_redo();
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::S)) {
            self.save_project_dialog();
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::O)) {
            self.open_project_dialog();
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::B)) {
            self.execute_palette_command(PaletteCommand::OpenBounceModal);
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::T)) {
            self.execute_palette_command(PaletteCommand::AddUniversalTrack);
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::D)) {
            if let Some((tid, cid)) = self.selected_clip {
                self.snapshot_before("Duplicate Clip");
                if let Some(nid) = self.project.duplicate_clip(tid, cid) {
                    self.selected_clip = Some((tid, nid));
                    self.sync_engine();
                }
            }
            return;
        }

        if ctx.input(|i| i.key_pressed(Key::Space)) {
            self.toggle_play();
        }
        if shift && ctx.input(|i| i.key_pressed(Key::R)) {
            self.execute_palette_command(PaletteCommand::CaptureRetroactive);
            return;
        }
        if !ctrl && !shift && ctx.input(|i| i.key_pressed(Key::Delete)) {
            if let Some((tid, cid)) = self.selected_clip.take() {
                self.snapshot_before("Delete Clip");
                if let Some(t) = self.project.find_track_mut(tid) {
                    t.clips.retain(|c| c.id != cid);
                }
                self.sync_engine();
            }
        }

        // Computer Keyboard Musical Typing (ASDFGHJK = White Keys C..C, WETYU = Black Keys, Z/X = Octave!)
        if self.musical_typing_enabled && !ctrl {
            if ctx.input(|i| i.key_pressed(Key::Z)) {
                self.musical_typing_octave = (self.musical_typing_octave - 1).max(1);
            }
            if ctx.input(|i| i.key_pressed(Key::X)) {
                self.musical_typing_octave = (self.musical_typing_octave + 1).min(7);
            }

            let key_map: &[(Key, u8)] = &[
                (Key::A, 0),
                (Key::W, 1),
                (Key::S, 2),
                (Key::E, 3),
                (Key::D, 4),
                (Key::F, 5),
                (Key::T, 6),
                (Key::G, 7),
                (Key::Y, 8),
                (Key::H, 9),
                (Key::U, 10),
                (Key::J, 11),
                (Key::K, 12),
            ];
            let base = ((self.musical_typing_octave as i32 + 1) * 12).clamp(12, 96) as u8;
            let target = self.selected_track;

            for &(k, semi) in key_map {
                let note = (base + semi).min(127);
                let down = ctx.input(|i| i.key_down(k));
                let was_down = self.held_typing_keys.contains(&k);
                if down && !was_down {
                    self.held_typing_keys.insert(k);
                    self.engine.send(EngineCommand::LiveMidi {
                        target_track: target,
                        note,
                        velocity: 108,
                        is_note_on: true,
                    });
                } else if !down && was_down {
                    self.held_typing_keys.remove(&k);
                    self.engine.send(EngineCommand::LiveMidi {
                        target_track: target,
                        note,
                        velocity: 0,
                        is_note_on: false,
                    });
                }
            }
        }
    }

    fn render_top_transport_bar(&mut self, ui: &mut egui::Ui) {
        let telem = self.engine.latest_telemetry;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 7.0;

            // Brand + File Menu
            ui.menu_button(
                egui::RichText::new("🧲 FERRIC DAW")
                    .size(13.5)
                    .strong()
                    .color(ACCENT_FERRIC),
                |ui| {
                    if ui.button("📄 New Empty Session").clicked() {
                        self.snapshot_before("New Empty Session");
                        self.project = Project::new_empty("Untitled Session");
                        let tid = self.project.add_track("01 Audio / MIDI");
                        self.selected_track = Some(tid);
                        self.selected_clip = None;
                        self.sync_engine();
                        ui.close_menu();
                    }
                    if ui.button("🎵 Reload 'Neon Oxide' Demo Session").clicked() {
                        self.execute_palette_command(PaletteCommand::LoadDemoProject);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("📂 Open Session (.ferric / .json)...  Ctrl+O").clicked() {
                        self.open_project_dialog();
                        ui.close_menu();
                    }
                    if ui.button("💾 Save Session (.ferric)...  Ctrl+S").clicked() {
                        self.save_project_dialog();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("💿 Bounce Master / Stems to WAV...  Ctrl+B").clicked() {
                        self.execute_palette_command(PaletteCommand::OpenBounceModal);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("↩ Undo  Ctrl+Z").clicked() {
                        self.perform_undo();
                        ui.close_menu();
                    }
                    if ui.button("↪ Redo  Ctrl+Y").clicked() {
                        self.perform_redo();
                        ui.close_menu();
                    }
                },
            );

            ui.separator();

            // Transport Controls
            if ui
                .button("⏮")
                .on_hover_text("Return to Start (001.1.000)")
                .clicked()
            {
                self.engine.send(EngineCommand::SeekBeat(0.0));
            }
            let play_lbl = if telem.is_playing { "⏸ PAUSE" } else { "▶ PLAY" };
            if pill_toggle(ui, play_lbl, telem.is_playing, ACCENT_TEAL)
                .on_hover_text("Play / Stop (Space)")
                .clicked()
            {
                self.toggle_play();
            }
            if pill_toggle(ui, "● REC", telem.is_recording, ACCENT_RED)
                .on_hover_text("Record into Armed / Selected Track")
                .clicked()
            {
                self.toggle_record();
            }
            if pill_toggle(ui, "⟳ LOOP", self.project.loop_enabled, ACCENT_FERRIC).clicked() {
                self.project.loop_enabled = !self.project.loop_enabled;
                self.sync_engine();
            }
            if pill_toggle(ui, "⏱ CLICK", self.project.metronome_enabled, ACCENT_AMBER).clicked() {
                self.project.metronome_enabled = !self.project.metronome_enabled;
                self.sync_engine();
            }

            // Always-Listening Retroactive Capture Button!
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("⚡ Capture Idea")
                            .size(11.0)
                            .strong()
                            .color(ACCENT_AMBER),
                    )
                    .fill(Color32::from_rgb(44, 36, 22)),
                )
                .on_hover_text(
                    "Retroactive Capture (Shift+R): Drops the last 15s of played MIDI or Audio onto the timeline even when NOT recording!",
                )
                .clicked()
            {
                self.execute_palette_command(PaletteCommand::CaptureRetroactive);
            }

            ui.separator();

            // High-Contrast Studio LCD Position & Tempo Readout
            egui::Frame::new()
                .fill(BG_DEEP)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .corner_radius(4)
                .inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let bbt = self.project.format_bar_beat_tick(telem.playhead_beat);
                        let secs = self.project.beats_to_seconds(telem.playhead_beat);
                        ui.label(
                            egui::RichText::new(bbt)
                                .size(14.5)
                                .monospace()
                                .strong()
                                .color(ACCENT_TEAL),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:05.2}s", secs))
                                .size(11.5)
                                .monospace()
                                .color(TEXT_DIM),
                        );
                    });
                });

            let r_bpm = ui.add(
                egui::DragValue::new(&mut self.project.bpm)
                    .range(30.0..=300.0)
                    .speed(0.25)
                    .suffix(" BPM"),
            );
            if r_bpm.changed() {
                self.sync_engine();
            }

            // Musical Typing Toggle
            let mt_lbl = format!("🎹 Keys C{}", self.musical_typing_octave);
            if pill_toggle(ui, &mt_lbl, self.musical_typing_enabled, Color32::from_rgb(186, 108, 226))
                .on_hover_text("Computer Keyboard Musical Typing (A..K = Notes, Z/X = Octave)")
                .clicked()
            {
                self.musical_typing_enabled = !self.musical_typing_enabled;
            }

            // Right-Aligned Command Palette + Chromatic Tuner Pill + LUFS + CPU Load
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(egui::RichText::new("⌘ Ctrl+K").size(11.0).color(ACCENT_FERRIC))
                    .on_hover_text("Open Instant Command & FX Palette")
                    .clicked()
                {
                    self.palette.toggle();
                }

                ui.label(
                    egui::RichText::new(format!(
                        "DSP {:.1}% | {:.1} LUFS",
                        telem.cpu_load_percent, telem.master.momentary_lufs
                    ))
                    .size(11.0)
                    .monospace()
                    .color(TEXT_DIM),
                );

                if telem.tuner.active {
                    ui.label(
                        egui::RichText::new(format!(
                            "🎯 {} {:+.0}ct",
                            telem.tuner.note_label(),
                            telem.tuner.cents
                        ))
                        .size(11.5)
                        .monospace()
                        .color(ACCENT_TEAL),
                    );
                }
            });
        });
    }

    fn render_bounce_modal(&mut self, ctx: &Context) {
        if !self.bounce_modal.open {
            return;
        }
        let mut keep_open = true;
        egui::Window::new("💿 Offline Bounce & Stem Export")
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([430.0, 260.0])
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Faster-than-realtime deterministic render with full FX, Modulators & Swipe-Comp crossfades.",
                    )
                    .size(11.0)
                    .color(TEXT_DIM),
                );
                ui.separator();

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.bounce_modal.export_stems,
                        false,
                        "Stereo Master Mix (.wav)",
                    );
                    ui.radio_value(
                        &mut self.bounce_modal.export_stems,
                        true,
                        "All Individual Track Stems (Folder)",
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Sample Rate:");
                    for sr in [44_100_u32, 48_000, 96_000] {
                        ui.selectable_value(
                            &mut self.bounce_modal.sample_rate,
                            sr,
                            format!("{} Hz", sr),
                        );
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Bit Depth:");
                    egui::ComboBox::from_id_salt("bounce_bit_depth")
                        .selected_text(self.bounce_modal.bit_depth.label())
                        .show_ui(ui, |ui| {
                            for &bd in BitDepth::ALL {
                                ui.selectable_value(
                                    &mut self.bounce_modal.bit_depth,
                                    bd,
                                    bd.label(),
                                );
                            }
                        });
                });

                ui.horizontal(|ui| {
                    ui.label("Beat Range:");
                    ui.add(
                        egui::DragValue::new(&mut self.bounce_modal.start_beat)
                            .range(0.0..=512.0)
                            .prefix("Start "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.bounce_modal.end_beat)
                            .range(1.0..=1024.0)
                            .prefix("End "),
                    );
                });

                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("⚡ Render Offline Now")
                                .strong()
                                .color(Color32::BLACK),
                        )
                        .fill(ACCENT_TEAL),
                    )
                    .clicked()
                {
                    if self.bounce_modal.export_stems {
                        if let Some(dir) = rfd::FileDialog::new()
                            .set_title("Choose Output Directory for Stems")
                            .pick_folder()
                        {
                            self.bounce_modal.last_report = Some(
                                render_all_stems_offline(
                                    &self.project,
                                    &dir,
                                    self.bounce_modal.sample_rate,
                                    self.bounce_modal.bit_depth,
                                    self.bounce_modal.start_beat,
                                    self.bounce_modal.end_beat,
                                )
                                .map_err(|e| e.to_string()),
                            );
                        }
                    } else if let Some(file) = rfd::FileDialog::new()
                        .set_title("Export Master WAV")
                        .add_filter("WAV Audio", &["wav"])
                        .set_file_name("master_mix.wav")
                        .save_file()
                    {
                        self.bounce_modal.last_report = Some(
                            render_master_offline(
                                &self.project,
                                &file,
                                self.bounce_modal.sample_rate,
                                self.bounce_modal.bit_depth,
                                self.bounce_modal.start_beat,
                                self.bounce_modal.end_beat,
                            )
                            .map_err(|e| e.to_string()),
                        );
                    }
                }

                if let Some(ref res) = self.bounce_modal.last_report {
                    ui.separator();
                    match res {
                        Ok(rep) => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "✓ Exported {} file(s) ({:.1}s) • Peak {:+.1} dB • {:.1} LUFS",
                                    rep.output_files.len(),
                                    rep.duration_secs,
                                    rep.peak_db,
                                    rep.short_term_lufs
                                ))
                                .color(ACCENT_TEAL),
                            );
                        }
                        Err(err) => {
                            ui.label(egui::RichText::new(err).color(ACCENT_RED));
                        }
                    }
                }
            });
        self.bounce_modal.open = keep_open;
    }
}

impl eframe::App for FerricDawApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.engine.poll_ui(&mut self.capture_inbox);
        self.handle_captured_takes();
        self.handle_shortcuts_and_musical_typing(ctx);

        // Request smooth 60 FPS repaint during active playback or live metering
        if self.engine.latest_telemetry.is_playing
            || self.engine.latest_telemetry.master.peak_l_db > -65.0
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        if let Some(cmd) = self.palette.show(ctx) {
            self.execute_palette_command(cmd);
        }
        self.render_bounce_modal(ctx);

        // 1. Top Studio Transport Bar
        egui::TopBottomPanel::top("ferric_transport_top")
            .exact_height(42.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ctx, |ui| {
                self.render_top_transport_bar(ui);
            });

        // 2. Thin Status Bar at very bottom
        egui::TopBottomPanel::bottom("ferric_status_footer")
            .exact_height(24.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_DEEP)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .small_button(if self.show_bottom_dock {
                            "▼ Hide Lower Studio Dock"
                        } else {
                            "▲ Show Lower Studio Dock"
                        })
                        .clicked()
                    {
                        self.show_bottom_dock = !self.show_bottom_dock;
                    }
                    ui.separator();
                    ui.label(
                        egui::RichText::new(&self.status_banner)
                            .size(11.0)
                            .color(TEXT_DIM),
                    );
                });
            });

        // 3. Resizable Lower Studio Dock (Piano Roll / Audio Comp, FX + Modulators, Mixer, Routing Matrix)
        if self.show_bottom_dock {
            egui::TopBottomPanel::bottom("ferric_lower_dock")
                .resizable(true)
                .default_height(265.0)
                .min_height(175.0)
                .max_height(480.0)
                .frame(
                    egui::Frame::new()
                        .fill(BG_PANEL)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .inner_margin(8.0),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for (tab, label) in [
                            (BottomTab::EditorAndComp, "🎹 Clip Editor / Swipe Comp"),
                            (BottomTab::FxAndModulators, "🎛 FX Chain & Modulators"),
                            (BottomTab::MixingConsole, "🎚 Mixing Console"),
                            (BottomTab::RoutingMatrix, "🔀 Routing Matrix"),
                        ] {
                            if pill_toggle(ui, label, self.bottom_tab == tab, ACCENT_FERRIC)
                                .clicked()
                            {
                                self.bottom_tab = tab;
                            }
                        }
                        if let Some(tid) = self.selected_track {
                            if let Some(trk) = self.project.find_track(tid) {
                                ui.separator();
                                ui.label(
                                    egui::RichText::new(format!("Selected: {}", trk.name))
                                        .size(11.5)
                                        .color(TEXT_MAIN),
                                );
                            }
                        }
                    });
                    ui.separator();

                    let mut dock_actions = Vec::new();
                    let telem = self.engine.latest_telemetry;
                    match self.bottom_tab {
                        BottomTab::EditorAndComp => {
                            self.piano_roll.ui(
                                ui,
                                &mut self.project,
                                self.selected_clip,
                                &telem,
                                &mut dock_actions,
                            );
                        }
                        BottomTab::FxAndModulators => {
                            render_fx_and_modulators_panel(
                                ui,
                                &mut self.project,
                                self.selected_track,
                                &mut self.inspect_master_fx,
                                &telem,
                                &mut dock_actions,
                            );
                        }
                        BottomTab::MixingConsole => {
                            render_mixer_console(
                                ui,
                                &mut self.project,
                                &mut self.selected_track,
                                &mut self.inspect_master_fx,
                                &telem,
                                &mut dock_actions,
                            );
                        }
                        BottomTab::RoutingMatrix => {
                            render_routing_matrix(ui, &mut self.project, &mut dock_actions);
                        }
                    }

                    let mut need_sync = false;
                    for act in dock_actions {
                        match act {
                            DockAction::ProjectMutated(_lbl) => {
                                need_sync = true;
                            }
                            DockAction::AuditionMidi {
                                track_id,
                                note,
                                is_note_on,
                            } => {
                                self.engine.send(EngineCommand::LiveMidi {
                                    target_track: Some(track_id),
                                    note,
                                    velocity: if is_note_on { 104 } else { 0 },
                                    is_note_on,
                                });
                            }
                        }
                    }
                    if need_sync {
                        self.sync_engine();
                    }
                });
        }

        // 4. Central Hybrid Session Clip Launcher + Linear Multitrack Arrangement
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG_PANEL).inner_margin(6.0))
            .show(ctx, |ui| {
                let mut arr_actions = Vec::new();
                let telem = self.engine.latest_telemetry;
                self.arrangement.ui(
                    ui,
                    &mut self.project,
                    self.selected_track,
                    self.selected_clip,
                    &telem,
                    &mut arr_actions,
                );

                let mut need_sync = false;
                for act in arr_actions {
                    match act {
                        ArrangementAction::Seek(beat) => {
                            self.engine.send(EngineCommand::SeekBeat(beat));
                        }
                        ArrangementAction::SelectTrack(tid) => {
                            self.selected_track = Some(tid);
                            self.inspect_master_fx = false;
                        }
                        ArrangementAction::SelectClip { track_id, clip_id } => {
                            self.selected_track = Some(track_id);
                            self.selected_clip = Some((track_id, clip_id));
                        }
                        ArrangementAction::OpenPianoRoll { track_id, clip_id } => {
                            self.selected_track = Some(track_id);
                            self.selected_clip = Some((track_id, clip_id));
                            self.bottom_tab = BottomTab::EditorAndComp;
                            self.show_bottom_dock = true;
                        }
                        ArrangementAction::ProjectMutated(_lbl) => {
                            need_sync = true;
                        }
                        ArrangementAction::SplitClip {
                            track_id,
                            clip_id,
                            beat,
                        } => {
                            self.snapshot_before("Split Clip");
                            if let Some(new_id) = self.project.split_clip(track_id, clip_id, beat) {
                                self.selected_clip = Some((track_id, new_id));
                                need_sync = true;
                            }
                        }
                        ArrangementAction::DuplicateClip { track_id, clip_id } => {
                            self.snapshot_before("Duplicate Clip");
                            if let Some(new_id) = self.project.duplicate_clip(track_id, clip_id) {
                                self.selected_clip = Some((track_id, new_id));
                                need_sync = true;
                            }
                        }
                        ArrangementAction::DeleteClip { track_id, clip_id } => {
                            self.snapshot_before("Delete Clip");
                            if let Some(t) = self.project.find_track_mut(track_id) {
                                t.clips.retain(|c| c.id != clip_id);
                                if self.selected_clip == Some((track_id, clip_id)) {
                                    self.selected_clip = None;
                                }
                                need_sync = true;
                            }
                        }
                        ArrangementAction::ImportAudioToTrack(tid) => {
                            self.import_wav_to_track(tid);
                        }
                        ArrangementAction::AddTrack => {
                            self.snapshot_before("Add Track");
                            let idx = self.project.tracks.len() + 1;
                            let track = Track::new(
                                format!("{:02} Universal Track", idx),
                                idx - 1,
                                self.project.scenes.len(),
                            );
                            let id = track.id;
                            self.project.tracks.push(track);
                            self.selected_track = Some(id);
                            need_sync = true;
                        }
                    }
                }
                if need_sync {
                    self.sync_engine();
                }
            });
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FerricDAW — Open-Source Rust Hybrid Digital Audio Workstation")
            .with_inner_size([1440.0, 860.0])
            .with_min_inner_size([980.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "FerricDAW",
        native_options,
        Box::new(|cc| Ok(Box::new(FerricDawApp::new(cc)))),
    )
}
