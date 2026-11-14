#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod arrangement;
mod browser;
mod help_modal;
mod mixer;
mod palette;
mod piano_roll;
mod theme;

use arrangement::{ArrangementAction, ArrangementState};
use browser::{BrowserAction, KaossPadState, StudioBrowserState, TapTempoTracker};
use help_modal::{build_app_icon, render_quickstart_help_modal};
use eframe::egui;
use egui::{Color32, Context, Key, Stroke};
use ferric_engine::{
    audio_buffer_from_capture, import_audio_file, load_project_bundle,
    render_all_stems_with_options, render_master_with_options, render_track_freeze_buffer,
    save_project_bundle, BitDepth, BounceReport, CapturedTakeEvent, EngineCommand, EngineHandle,
    ExportFormat, MasteringOptions,
};
use ferric_model::{
    build_demo_project, import_smf_file, AutomationTarget, ClipId, FxKind, FxSlot, HistoryManager,
    InputSource, MidiLearnMapping, ModulatorSlot, Project, Take, TakeContent, TimelineClip, Track,
    TrackId,
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
    KaossPad,
    MixingConsole,
    RoutingMatrix,
}

struct BounceModalState {
    open: bool,
    export_stems: bool,
    format: ExportFormat,
    sample_rate: u32,
    bit_depth: BitDepth,
    start_beat: f64,
    end_beat: f64,
    enable_lufs_norm: bool,
    target_lufs: f32,
    true_peak_ceiling_dbtp: f32,
    tpdf_dither: bool,
    last_report: Option<Result<BounceReport, String>>,
}

struct PendingMidiLearn {
    track_id: TrackId,
    target: AutomationTarget,
    label: String,
}

struct FerricDawApp {
    project: Project,
    project_path: Option<PathBuf>,
    history: HistoryManager,
    engine: EngineHandle,
    browser: StudioBrowserState,
    kaoss_pad: KaossPadState,
    tap_tempo: TapTempoTracker,
    arrangement: ArrangementState,
    piano_roll: PianoRollState,
    palette: CommandPalette,
    bottom_tab: BottomTab,
    show_bottom_dock: bool,
    inspect_master_fx: bool,
    selected_track: Option<TrackId>,
    selected_clip: Option<(TrackId, ClipId)>,
    clipboard_clip: Option<TimelineClip>,
    recent_projects: Vec<PathBuf>,
    show_help_modal: bool,
    dirty_since_autosave: bool,
    last_autosave_inst: std::time::Instant,
    pending_midi_learn: Option<PendingMidiLearn>,
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
        let project = if engine.sample_rate != 48_000 {
            build_demo_project(engine.sample_rate)
        } else {
            initial_project
        };

        let lpk_badge = if engine.has_akai_lpk25() {
            " • Akai LPK25 Connected!"
        } else if !engine.midi_port_names.is_empty() {
            " • USB MIDI Connected!"
        } else {
            ""
        };

        let mut app = Self {
            selected_track: project.tracks.get(4).or(project.tracks.first()).map(|t| t.id),
            selected_clip: project
                .tracks
                .get(3)
                .and_then(|t| t.clips.first().map(|c| (t.id, c.id))),
            bounce_modal: BounceModalState {
                open: false,
                export_stems: false,
                format: ExportFormat::Wav,
                sample_rate: engine.sample_rate,
                bit_depth: BitDepth::Pcm24,
                start_beat: 0.0,
                end_beat: project.project_end_beat(),
                enable_lufs_norm: true,
                target_lufs: -14.0,
                true_peak_ceiling_dbtp: -1.0,
                tpdf_dither: true,
                last_report: None,
            },
            status_banner: format!(
                "v1.0 Studio Ready • Out: {} ({} Hz) • In: {}{} • Space: Play • Ctrl+K: Palette",
                engine.audio_device_name, engine.sample_rate, engine.audio_input_name, lpk_badge
            ),
            project,
            project_path: None,
            history: HistoryManager::new(64),
            engine,
            browser: StudioBrowserState::default(),
            kaoss_pad: KaossPadState::default(),
            tap_tempo: TapTempoTracker::default(),
            arrangement: ArrangementState::default(),
            piano_roll: PianoRollState::default(),
            palette: CommandPalette::default(),
            bottom_tab: BottomTab::FxAndModulators,
            show_bottom_dock: true,
            inspect_master_fx: false,
            clipboard_clip: None,
            recent_projects: Self::load_recent_projects_list(),
            show_help_modal: false,
            dirty_since_autosave: false,
            last_autosave_inst: std::time::Instant::now(),
            pending_midi_learn: None,
            musical_typing_enabled: true,
            musical_typing_octave: 4,
            held_typing_keys: HashSet::new(),
            capture_inbox: Vec::new(),
        };
        app.engine.sync_project(&app.project);

        // Support launching `ferricdaw.exe my_song.ferric` (or `.json` / `.wav` / `.flac` / `.mid`) from CLI / File Association!
        if let Some(arg_path) = std::env::args_os().nth(1).map(PathBuf::from) {
            if arg_path.exists() {
                let ext = arg_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if matches!(ext.as_str(), "ferric" | "json") {
                    app.open_project_from_path(&arg_path);
                } else if matches!(ext.as_str(), "mid" | "midi") {
                    app.import_smf_midi_dialog(Some(arg_path));
                } else if let Ok(buf) = import_audio_file(&arg_path, app.engine.sample_rate) {
                    let tid = app.project.add_track("Imported CLI Audio");
                    let beats = app.project.seconds_to_beats(buf.duration_secs()).max(1.0);
                    let nm = arg_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Audio")
                        .to_string();
                    if let Some(trk) = app.project.find_track_mut(tid) {
                        trk.clips.push(TimelineClip::new_audio(nm, 0.0, beats, buf));
                    }
                    app.selected_track = Some(tid);
                    app.sync_engine();
                }
            }
        }
        app
    }

    fn recent_projects_file() -> PathBuf {
        let base = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("FerricDAW").join("recent_projects.txt")
    }

    fn load_recent_projects_list() -> Vec<PathBuf> {
        let p = Self::recent_projects_file();
        std::fs::read_to_string(p)
            .map(|txt| {
                txt.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(PathBuf::from)
                    .filter(|pb| pb.exists())
                    .take(8)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn record_recent_project(&mut self, path: &PathBuf) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.clone());
        self.recent_projects.truncate(8);
        let p = Self::recent_projects_file();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body: String = self
            .recent_projects
            .iter()
            .map(|pb| format!("{}\n", pb.to_string_lossy()))
            .collect();
        let _ = std::fs::write(p, body);
    }

    fn snapshot_before(&mut self, label: &'static str) {
        self.history.record(label, self.project.clone());
        self.dirty_since_autosave = true;
    }

    fn sync_engine(&mut self) {
        self.engine.sync_project(&self.project);
        self.dirty_since_autosave = true;
    }

    fn autosave_recovery_path() -> PathBuf {
        let base = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("FerricDAW")
            .join("autosave")
            .join("recovery.ferric")
    }

    fn tick_autosave(&mut self) {
        if self.dirty_since_autosave
            && self.last_autosave_inst.elapsed().as_secs() >= 60
            && !self.engine.latest_telemetry.is_recording
        {
            let path = Self::autosave_recovery_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            self.engine
                .harvest_plugin_states_into_project(&mut self.project);
            if save_project_bundle(&mut self.project, &path).is_ok() {
                self.dirty_since_autosave = false;
                self.last_autosave_inst = std::time::Instant::now();
            }
        }
    }

    fn recover_autosave(&mut self) {
        let path = Self::autosave_recovery_path();
        if !path.exists() {
            self.status_banner = "No autosave recovery snapshot found yet.".into();
            return;
        }
        match load_project_bundle(&path, self.engine.sample_rate) {
            Ok(loaded) => {
                self.snapshot_before("Recover Autosave");
                self.project = loaded;
                self.selected_track = self.project.tracks.first().map(|t| t.id);
                self.selected_clip = None;
                self.sync_engine();
                self.status_banner = format!("✓ Recovered session from {}", path.display());
            }
            Err(e) => {
                self.status_banner = format!("Autosave recovery failed: {}", e);
            }
        }
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

    fn toggle_freeze_track(&mut self, track_id: TrackId) {
        let currently_frozen = self
            .project
            .find_track(track_id)
            .map(|t| t.frozen)
            .unwrap_or(false);
        self.snapshot_before("Toggle Track Freeze");

        if currently_frozen {
            if let Some(t) = self.project.find_track_mut(track_id) {
                t.frozen = false;
                t.frozen_buffer = None;
                self.status_banner = format!("Unfroze '{}' — live Instrument & FX restored.", t.name);
            }
            self.sync_engine();
        } else if let Some(buf) =
            render_track_freeze_buffer(&self.project, track_id, self.engine.sample_rate)
        {
            if let Some(t) = self.project.find_track_mut(track_id) {
                t.frozen = true;
                t.frozen_buffer = Some(buf);
                self.status_banner = format!(
                    "❄ Froze '{}' to 32-bit stem — live DSP CPU reduced to 0.0%!",
                    t.name
                );
            }
            self.sync_engine();
        }
    }

    /// 1-Click Hardware Studio Rig tailored specifically for Akai LPK25 + Nintendo DS KORG DS-10!
    fn spawn_akai_and_ds10_rig(&mut self) {
        self.snapshot_before("Spawn Akai LPK25 + KORG DS-10 Rig");
        self.engine.rescan_midi_devices();

        // 1. Akai LPK25 USB MIDI Lead/Arp Synth Track (Armed for MIDI!)
        let lpk_id = self.project.add_track("LPK25 Analog Synth");
        if let Some(t) = self.project.find_track_mut(lpk_id) {
            t.color = [232, 114, 62];
            t.input_source = InputSource::MidiAll;
            t.record_arm = true;
            let mut poly = FxSlot::new(FxKind::PolySynth);
            poly.set_param(0, 0.0); // Fat Saw
            poly.set_param(1, 42.0); // Unison Sub/Detune
            poly.set_param(2, 2400.0);
            poly.set_param(3, 0.38);
            t.fx_chain.push(poly);
            t.fx_chain.push(FxSlot::new(FxKind::StereoDelay));
        }

        // 2. Nintendo DS (KORG DS-10) Stereo 3.5mm Line-In Console Channel!
        let ds_id = self.project.add_track("Nintendo DS — KORG DS-10 Line-In");
        if let Some(t) = self.project.find_track_mut(ds_id) {
            t.color = [66, 179, 165];
            t.input_source = InputSource::AudioAndMidi { stereo_pair: 0 };
            t.record_arm = true;
            t.input_monitoring = true;

            // Clean up portable headphone-out hiss with gentle Gate, sculpt MS-10 punch with EQ, add Oxide Tape & Space!
            let mut gate = FxSlot::new(FxKind::NoiseGate);
            gate.set_param(0, -54.0);
            gate.set_param(5, -78.0);
            let mut eq = FxSlot::new(FxKind::ParametricEq);
            eq.set_param(0, 32.0); // Keep deep MS-10 bass!
            eq.set_param(2, 2.0);  // +2 dB low punch
            eq.set_param(10, 2.5); // +2.5 dB top air for DS DAC roll-off
            let mut tape = FxSlot::new(FxKind::TapeSaturator);
            tape.set_param(0, 4.0);
            tape.set_param(1, 48.0);

            t.fx_chain.push(gate);
            t.fx_chain.push(eq);
            t.fx_chain.push(tape);
        }

        self.selected_track = Some(lpk_id);
        self.bottom_tab = BottomTab::KaossPad;
        self.show_bottom_dock = true;
        self.sync_engine();
        self.status_banner =
            "⚡ Spawned 'LPK25 Analog Synth' + 'Nintendo DS — KORG DS-10 Line-In' & opened KAOSS X-Y Pad!"
                .into();
    }

    fn handle_midi_learn_poll(&mut self) {
        let Some((ch, cc, val)) = self.engine.latest_telemetry.last_midi_cc else {
            return;
        };
        if let Some(pending) = self.pending_midi_learn.take() {
            self.project
                .midi_learn_mappings
                .retain(|m| !(m.track_id == pending.track_id && m.target == pending.target));
            self.project.midi_learn_mappings.push(MidiLearnMapping {
                id: uuid::Uuid::new_v4(),
                cc,
                channel: Some(ch),
                track_id: pending.track_id,
                target: pending.target.clone(),
            });
            self.project.set_target_normalized(
                pending.track_id,
                &pending.target,
                val as f32 / 127.0,
                self.engine.latest_telemetry.playhead_beat,
                self.engine.latest_telemetry.is_playing,
            );
            self.sync_engine();
            self.status_banner = format!(
                "✓ Bound MIDI Ch {} CC#{} → {}",
                ch + 1,
                cc,
                pending.label
            );
        } else if self.project.apply_midi_cc(
            ch,
            cc,
            val,
            self.engine.latest_telemetry.playhead_beat,
            self.engine.latest_telemetry.is_playing,
        ) {
            self.sync_engine();
        }
    }

    fn handle_captured_takes(&mut self) {
        if self.capture_inbox.is_empty() {
            return;
        }
        let events: Vec<CapturedTakeEvent> = self.capture_inbox.drain(..).collect();
        self.snapshot_before("Commit Captured Take(s)");
        for ev in events {
            let fallback_tid = self
                .project
                .tracks
                .iter()
                .find(|t| t.record_arm)
                .map(|t| t.id)
                .or(self.selected_track)
                .unwrap_or_else(|| self.project.add_track("Captured Track"));

            match ev {
                CapturedTakeEvent::MidiTake {
                    target_track,
                    start_beat,
                    length_beats,
                    notes,
                    is_retroactive,
                } => {
                    let target_tid = target_track.unwrap_or(fallback_tid);
                    if let Some(trk) = self.project.find_track_mut(target_tid) {
                        if let Some(existing) = trk
                            .clips
                            .iter_mut()
                            .find(|c| c.is_midi() && (c.start_beat - start_beat).abs() < 0.5)
                        {
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
                            let clip =
                                TimelineClip::new_midi(title, start_beat, length_beats, notes);
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
                    target_track,
                    start_beat,
                    length_beats,
                    sample_rate,
                    left,
                    right,
                    is_retroactive,
                } => {
                    let target_tid = target_track.unwrap_or(fallback_tid);
                    let buf = audio_buffer_from_capture(sample_rate, left, right);
                    if let Some(trk) = self.project.find_track_mut(target_tid) {
                        if let Some(existing) = trk
                            .clips
                            .iter_mut()
                            .find(|c| !c.is_midi() && (c.start_beat - start_beat).abs() < 0.5)
                        {
                            let num = existing.takes.len() + 1;
                            existing
                                .takes
                                .push(Take::new_audio(format!("Take {}", num), buf));
                            existing.active_take = existing.takes.len() - 1;
                            existing.show_take_lanes = true;
                        } else {
                            let title = if is_retroactive {
                                "Retroactive DS-10 / Audio Riff"
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
        }
        self.sync_engine();
    }

    fn execute_palette_command(&mut self, cmd: PaletteCommand) {
        match cmd {
            PaletteCommand::TogglePlay => self.toggle_play(),
            PaletteCommand::ToggleRecord => self.toggle_record(),
            PaletteCommand::SetupAkaiDs10Rig => self.spawn_akai_and_ds10_rig(),
            PaletteCommand::ToggleFreezeSelectedTrack => {
                if let Some(tid) = self.selected_track {
                    self.toggle_freeze_track(tid);
                }
            }
            PaletteCommand::CaptureRetroactive => {
                self.engine
                    .send(EngineCommand::CaptureRetroactive { seconds_back: 15.0 });
                self.status_banner =
                    "Requested 15s Retroactive Capture (LPK25 MIDI, Musical Typing, or DS-10 Line-In!)"
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
            PaletteCommand::AddSamplerTrack => {
                self.snapshot_before("Add Sampler Track");
                let id = self
                    .project
                    .add_track(format!("{:02} Slice Sampler", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.fx_chain.push(FxSlot::new(FxKind::SamplePlayer));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddVocalPitchTrack => {
                self.snapshot_before("Add Vocal Auto-Pitch Track");
                let id = self
                    .project
                    .add_track(format!("{:02} Vocal Auto-Pitch", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.input_source = InputSource::AudioMono { channel: 0 };
                    t.fx_chain.push(FxSlot::new(FxKind::NoiseGate));
                    t.fx_chain.push(FxSlot::new(FxKind::VocalPitch));
                    t.fx_chain.push(FxSlot::new(FxKind::Compressor));
                    t.fx_chain.push(FxSlot::new(FxKind::PlateHallReverb));
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
            PaletteCommand::GlueSelectedClips => self.glue_selected_clips(),
            PaletteCommand::ToggleConductorLane => {
                self.project.show_conductor_lane = !self.project.show_conductor_lane;
                if self.project.show_conductor_lane && self.project.tempo_map.is_empty() {
                    self.project.upsert_tempo_point(0.0, self.project.bpm, true);
                }
                self.sync_engine();
            }
            PaletteCommand::OpenQuickstartHelp => {
                self.show_help_modal = true;
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

    fn import_smf_midi_dialog(&mut self, path_opt: Option<PathBuf>) {
        let path = path_opt.or_else(|| {
            rfd::FileDialog::new()
                .set_title("Import Standard MIDI File (.mid)")
                .add_filter("MIDI Files", &["mid", "midi"])
                .pick_file()
        });
        let Some(p) = path else {
            return;
        };
        match import_smf_file(&p) {
            Ok(imported_tracks) => {
                self.snapshot_before("Import Standard MIDI File");
                for mtrk in &imported_tracks {
                    if let Some(bpm) = mtrk.detected_bpm {
                        self.project.bpm = bpm;
                    }
                    let tid = self.project.add_track(mtrk.name.clone());
                    let max_end = mtrk
                        .notes
                        .iter()
                        .map(|n| n.end_beat())
                        .fold(4.0_f64, f64::max)
                        .ceil();
                    if let Some(t) = self.project.find_track_mut(tid) {
                        t.fx_chain.push(FxSlot::new(FxKind::PolySynth));
                        let mut clip =
                            TimelineClip::new_midi(mtrk.name.clone(), 0.0, max_end, mtrk.notes.clone());
                        clip.loop_enabled = false;
                        if let Some(Take {
                            content: TakeContent::Midi { cc_points, .. },
                            ..
                        }) = clip.takes.first_mut()
                        {
                            *cc_points = mtrk.cc_points.clone();
                        }
                        self.selected_clip = Some((tid, clip.id));
                        t.clips.push(clip);
                    }
                    self.selected_track = Some(tid);
                }
                self.sync_engine();
                self.status_banner = format!(
                    "Imported {} MIDI track(s) from {}",
                    imported_tracks.len(),
                    p.display()
                );
            }
            Err(e) => {
                self.status_banner = format!("MIDI import error: {}", e);
            }
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
            self.save_project_to_path(&p);
        }
    }

    fn save_project_as_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .set_title("Save FerricDAW Session As...")
            .add_filter("FerricDAW Project (.ferric)", &["ferric"])
            .add_filter("JSON Project (.json)", &["json"])
            .set_file_name("session.ferric")
            .save_file()
        {
            self.save_project_to_path(&p);
        }
    }

    fn save_project_to_path(&mut self, p: &PathBuf) {
        self.engine
            .harvest_plugin_states_into_project(&mut self.project);
        match save_project_bundle(&mut self.project, p) {
            Ok(()) => {
                self.project_path = Some(p.clone());
                self.record_recent_project(p);
                self.dirty_since_autosave = false;
                self.status_banner =
                    format!("Saved full session & audio bundle to {}", p.display());
            }
            Err(e) => {
                self.status_banner = format!("Save error: {}", e);
            }
        }
    }

    fn open_project_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .set_title("Open FerricDAW Session")
            .add_filter("FerricDAW / JSON", &["ferric", "json"])
            .pick_file()
        {
            self.open_project_from_path(&p);
        }
    }

    fn open_project_from_path(&mut self, p: &PathBuf) {
        match load_project_bundle(p, self.engine.sample_rate) {
            Ok(loaded) => {
                self.snapshot_before("Open Project");
                self.project = loaded;
                self.project_path = Some(p.clone());
                self.record_recent_project(p);
                self.selected_track = self.project.tracks.first().map(|t| t.id);
                self.selected_clip = None;
                self.arrangement.multi_selected_clips.clear();
                self.sync_engine();
                self.dirty_since_autosave = false;
                self.status_banner = format!("Opened session bundle {}", p.display());
            }
            Err(e) => {
                self.status_banner = format!("Open error: {}", e);
            }
        }
    }

    fn glue_selected_clips(&mut self) {
        if self.arrangement.multi_selected_clips.len() < 2 {
            self.status_banner =
                "Select 2+ clips on a track (Shift+Click or Marquee Drag) then press Ctrl+G to Glue!".into();
            return;
        }
        let target_tid = self.arrangement.multi_selected_clips[0].0;
        let ids: Vec<ClipId> = self
            .arrangement
            .multi_selected_clips
            .iter()
            .filter(|&&(t, _)| t == target_tid)
            .map(|&(_, c)| c)
            .collect();
        if ids.len() < 2 {
            return;
        }
        self.snapshot_before("Glue / Consolidate Clips");
        if let Some(glued_id) =
            self.project
                .glue_clips(target_tid, &ids, self.engine.sample_rate)
        {
            self.arrangement.multi_selected_clips.clear();
            self.selected_clip = Some((target_tid, glued_id));
            self.sync_engine();
            self.status_banner = format!("✓ Glued {} clips into consolidated item!", ids.len());
        }
    }

    fn import_audio_to_track(&mut self, track_id: TrackId) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Import Audio File (WAV, FLAC, MP3, OGG, AIFF)")
            .add_filter("Audio Files", &["wav", "flac", "mp3", "ogg", "aiff"])
            .pick_file()
        {
            match import_audio_file(&path, self.engine.sample_rate) {
                Ok(buf) => {
                    self.snapshot_before("Import Audio Clip");
                    let beats = self.project.seconds_to_beats(buf.duration_secs()).max(1.0);
                    let at = self.project.snap_grid.snap(
                        self.engine.latest_telemetry.playhead_beat,
                        self.project.time_signature,
                    );
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
                    self.status_banner = format!("Audio import failed: {}", e);
                }
            }
        }
    }

    fn handle_shortcuts_and_musical_typing(&mut self, ctx: &Context) {
        if ctx.wants_keyboard_input() {
            return;
        }

        let (ctrl, shift) =
            ctx.input(|i| (i.modifiers.command || i.modifiers.ctrl, i.modifiers.shift));

        if ctx.input(|i| i.key_pressed(Key::F1)) {
            self.show_help_modal = !self.show_help_modal;
            return;
        }
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
            if shift {
                self.save_project_as_dialog();
            } else {
                self.save_project_dialog();
            }
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::G)) {
            self.glue_selected_clips();
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
        if ctrl && ctx.input(|i| i.key_pressed(Key::F)) {
            self.execute_palette_command(PaletteCommand::ToggleFreezeSelectedTrack);
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::D)) {
            if self.arrangement.multi_selected_clips.len() > 1 {
                self.snapshot_before("Duplicate Selected Clips");
                let mut new_sel = Vec::new();
                let batch = self.arrangement.multi_selected_clips.clone();
                for (tid, cid) in batch {
                    if let Some(nid) = self.project.duplicate_clip(tid, cid) {
                        new_sel.push((tid, nid));
                    }
                }
                self.arrangement.multi_selected_clips = new_sel;
                self.sync_engine();
            } else if let Some((tid, cid)) = self.selected_clip {
                self.snapshot_before("Duplicate Clip");
                if let Some(nid) = self.project.duplicate_clip(tid, cid) {
                    self.selected_clip = Some((tid, nid));
                    self.sync_engine();
                }
            }
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::C)) {
            if let Some((tid, cid)) = self.selected_clip {
                if let Some(clip) = self.project.find_track(tid).and_then(|t| t.find_clip(cid)) {
                    self.clipboard_clip = Some(clip.clone());
                    self.status_banner = format!("Copied clip '{}'", clip.name);
                }
            }
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::X)) {
            if let Some((tid, cid)) = self.selected_clip.take() {
                if let Some(clip) = self
                    .project
                    .find_track(tid)
                    .and_then(|t| t.find_clip(cid))
                    .cloned()
                {
                    self.snapshot_before("Cut Clip");
                    self.clipboard_clip = Some(clip);
                    self.project
                        .delete_clip(tid, cid, self.arrangement.ripple_edit);
                    self.sync_engine();
                }
            }
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::V)) {
            if let Some(mut pasted) = self.clipboard_clip.clone() {
                let dst_tid = self
                    .selected_track
                    .unwrap_or_else(|| self.project.add_track("Pasted Track"));
                self.snapshot_before("Paste Clip");
                pasted.id = uuid::Uuid::new_v4();
                pasted.start_beat = self.project.snap_grid.snap(
                    self.engine.latest_telemetry.playhead_beat,
                    self.project.time_signature,
                );
                let new_cid = pasted.id;
                if let Some(trk) = self.project.find_track_mut(dst_tid) {
                    trk.clips.push(pasted);
                }
                self.selected_clip = Some((dst_tid, new_cid));
                self.sync_engine();
                self.status_banner = "Pasted clip at Playhead.".into();
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
            if self.arrangement.multi_selected_clips.len() > 1 {
                self.snapshot_before("Delete Selected Clips");
                let batch: Vec<_> = self.arrangement.multi_selected_clips.drain(..).collect();
                for (tid, cid) in batch {
                    self.project
                        .delete_clip(tid, cid, self.arrangement.ripple_edit);
                }
                self.selected_clip = None;
                self.sync_engine();
            } else if let Some((tid, cid)) = self.selected_clip.take() {
                self.snapshot_before("Delete Clip");
                self.project
                    .delete_clip(tid, cid, self.arrangement.ripple_edit);
                self.sync_engine();
            }
        }

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
            ui.spacing_mut().item_spacing.x = 6.0;

            if pill_toggle(ui, "☰ Browser", self.browser.visible, ACCENT_TEAL)
                .on_hover_text("Show/Hide Left Studio Browser (Akai LPK25 + DS-10 Rig, Samples, Presets & CLAP Plugins)")
                .clicked()
            {
                self.browser.visible = !self.browser.visible;
            }

            ui.menu_button(
                egui::RichText::new("🧲 FERRIC DAW")
                    .size(13.0)
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
                    if ui
                        .button("🎹 Spawn Akai LPK25 + KORG DS-10 Hybrid Rig")
                        .clicked()
                    {
                        self.spawn_akai_and_ds10_rig();
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
                    if !self.recent_projects.is_empty() {
                        ui.menu_button("🕘 Open Recent Session", |ui| {
                            for rp in self.recent_projects.clone() {
                                if ui.button(rp.display().to_string()).clicked() {
                                    self.open_project_from_path(&rp);
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    if ui.button("💾 Save Session Bundle (.ferric + Audio)...  Ctrl+S").clicked() {
                        self.save_project_dialog();
                        ui.close_menu();
                    }
                    if ui.button("💾 Save Session As...  Ctrl+Shift+S").clicked() {
                        self.save_project_as_dialog();
                        ui.close_menu();
                    }
                    if ui.button("🛡 Recover Last Crash-Autosave").clicked() {
                        self.recover_autosave();
                        ui.close_menu();
                    }
                    if ui.button("🎹 Import Standard MIDI File (.mid)...").clicked() {
                        self.import_smf_midi_dialog(None);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("💿 Bounce Master / Stems to FLAC or WAV...  Ctrl+B").clicked() {
                        self.execute_palette_command(PaletteCommand::OpenBounceModal);
                        ui.close_menu();
                    }
                    if ui.button("🔗 Glue / Consolidate Selected Clips  Ctrl+G").clicked() {
                        self.glue_selected_clips();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("❓ Quickstart & Keyboard Shortcuts  F1").clicked() {
                        self.show_help_modal = true;
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

            if ui.button("⏮").on_hover_text("Return to Start").clicked() {
                self.engine.send(EngineCommand::SeekBeat(0.0));
            }
            let play_lbl = if telem.is_playing { "⏸ PAUSE" } else { "▶ PLAY" };
            if pill_toggle(ui, play_lbl, telem.is_playing, ACCENT_TEAL).clicked() {
                self.toggle_play();
            }
            if pill_toggle(ui, "● REC", telem.is_recording, ACCENT_RED).clicked() {
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

            // Count-In Bars (0 / 1 / 2 Bars) + Auto Punch-In/Out Popover!
            let ci_lbl = if self.project.count_in_bars == 0 {
                "Pre: Off".to_string()
            } else {
                format!("Pre: {}b", self.project.count_in_bars)
            };
            if pill_toggle(ui, &ci_lbl, self.project.count_in_bars > 0, ACCENT_AMBER)
                .on_hover_text("Cycle Metronome Count-In before Recording (Off → 1 Bar → 2 Bars)")
                .clicked()
            {
                self.project.count_in_bars = (self.project.count_in_bars + 1) % 3;
                self.sync_engine();
            }

            ui.menu_button(
                egui::RichText::new(if self.project.punch_in_enabled {
                    "🎯 PUNCH ON"
                } else {
                    "Punch"
                })
                .size(11.0)
                .color(if self.project.punch_in_enabled {
                    ACCENT_RED
                } else {
                    TEXT_MAIN
                }),
                |ui| {
                    let mut pch = false;
                    pch |= ui
                        .checkbox(&mut self.project.punch_in_enabled, "Enable Auto Punch-In / Out")
                        .changed();
                    pch |= ui
                        .add(
                            egui::DragValue::new(&mut self.project.punch_start_beat)
                                .range(0.0..=512.0)
                                .speed(0.5)
                                .prefix("Punch In Beat: "),
                        )
                        .changed();
                    pch |= ui
                        .add(
                            egui::DragValue::new(&mut self.project.punch_end_beat)
                                .range(1.0..=512.0)
                                .speed(0.5)
                                .prefix("Punch Out Beat: "),
                        )
                        .changed();
                    if pch {
                        self.sync_engine();
                    }
                },
            );

            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("⚡ Capture")
                            .size(11.0)
                            .strong()
                            .color(ACCENT_AMBER),
                    )
                    .fill(Color32::from_rgb(44, 36, 22)),
                )
                .on_hover_text("Retroactive Capture (Shift+R): Drops last 15s of LPK25 MIDI or DS-10 Audio!")
                .clicked()
            {
                self.execute_palette_command(PaletteCommand::CaptureRetroactive);
            }

            ui.separator();

            // LCD Position (or Count-In Countdown!) + BPM + TAP TEMPO
            egui::Frame::new()
                .fill(BG_DEEP)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .corner_radius(4)
                .inner_margin(egui::Margin::symmetric(7, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if telem.count_in_beats_remaining > 0.05 {
                            ui.label(
                                egui::RichText::new(format!(
                                    "COUNT -{:03.1}",
                                    telem.count_in_beats_remaining
                                ))
                                .size(14.0)
                                .monospace()
                                .strong()
                                .color(ACCENT_AMBER),
                            );
                        } else {
                            let bbt = self.project.format_bar_beat_tick(telem.playhead_beat);
                            let secs = self.project.beats_to_seconds(telem.playhead_beat);
                            let cur_ts = self.project.time_signature_at_beat(telem.playhead_beat);
                            ui.label(
                                egui::RichText::new(bbt)
                                    .size(14.0)
                                    .monospace()
                                    .strong()
                                    .color(ACCENT_TEAL),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "{:05.2}s  {}/{}",
                                    secs, cur_ts.numerator, cur_ts.denominator
                                ))
                                .size(11.0)
                                .monospace()
                                .color(TEXT_DIM),
                            );
                        }
                    });
                });

            if !self.project.tempo_map.is_empty() {
                let live_bpm = self.project.bpm_at_beat(telem.playhead_beat);
                ui.label(
                    egui::RichText::new(format!("🎼 {:.1} BPM", live_bpm))
                        .size(11.5)
                        .monospace()
                        .strong()
                        .color(ACCENT_AMBER),
                )
                .on_hover_text("Live Conductor Tempo Map Active!");
            } else if ui
                .add(
                    egui::DragValue::new(&mut self.project.bpm)
                        .range(30.0..=300.0)
                        .speed(0.25)
                        .suffix(" BPM"),
                )
                .changed()
            {
                self.sync_engine();
            }
            if ui
                .small_button("TAP")
                .on_hover_text("Tap 4x along to your Nintendo DS KORG DS-10 to lock exact BPM!")
                .clicked()
            {
                if let Some(new_bpm) = self.tap_tempo.register_tap() {
                    self.project.bpm = (new_bpm * 2.0).round() * 0.5;
                    self.sync_engine();
                    self.status_banner = format!("Tap Tempo locked: {:.1} BPM", self.project.bpm);
                }
            }

            let mt_lbl = format!("🎹 Keys C{}", self.musical_typing_octave);
            if pill_toggle(
                ui,
                &mt_lbl,
                self.musical_typing_enabled,
                Color32::from_rgb(186, 108, 226),
            )
            .clicked()
            {
                self.musical_typing_enabled = !self.musical_typing_enabled;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pill_toggle(ui, "❓ Help", self.show_help_modal, ACCENT_TEAL)
                    .on_hover_text("Open Studio Quickstart & Shortcut Guide (F1)")
                    .clicked()
                {
                    self.show_help_modal = !self.show_help_modal;
                }
                if ui
                    .button(egui::RichText::new("⌘ Ctrl+K").size(11.0).color(ACCENT_FERRIC))
                    .clicked()
                {
                    self.palette.toggle();
                }

                ui.label(
                    egui::RichText::new(format!(
                        "DSP {:.1}% | {:.1} LUFS",
                        telem.cpu_load_percent, telem.master.momentary_lufs
                    ))
                    .size(10.5)
                    .monospace()
                    .color(TEXT_DIM),
                );

                if self.engine.has_akai_lpk25() {
                    ui.label(
                        egui::RichText::new("LPK25 ●")
                            .size(10.5)
                            .strong()
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
        egui::Window::new("💿 Mastering Bounce & Lossless FLAC / WAV Stem Export")
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([495.0, 360.0])
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Deterministic mastering render with ITU-R BS.1770-4 Integrated LUFS Normalization, 4x Oversampled True-Peak Limiter & TPDF Dither.",
                    )
                    .size(10.5)
                    .color(TEXT_DIM),
                );
                ui.separator();

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.bounce_modal.export_stems,
                        false,
                        "Stereo Master Mix",
                    );
                    ui.radio_value(
                        &mut self.bounce_modal.export_stems,
                        true,
                        "All Individual Track Stems (Folder)",
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Container Format:");
                    for &fmt in ExportFormat::ALL {
                        ui.selectable_value(&mut self.bounce_modal.format, fmt, fmt.label());
                    }
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
                    ui.separator();
                    ui.label("Bit Depth:");
                    egui::ComboBox::from_id_salt("bounce_bit_depth")
                        .selected_text(self.bounce_modal.bit_depth.label())
                        .width(145.0)
                        .show_ui(ui, |ui| {
                            for &bd in BitDepth::ALL {
                                if self.bounce_modal.format == ExportFormat::Flac
                                    && bd == BitDepth::Float32
                                {
                                    continue; // FLAC supports lossless 16-bit & 24-bit integer PCM
                                }
                                ui.selectable_value(
                                    &mut self.bounce_modal.bit_depth,
                                    bd,
                                    bd.label(),
                                );
                            }
                        });
                });

                if self.bounce_modal.format == ExportFormat::Flac
                    && self.bounce_modal.bit_depth == BitDepth::Float32
                {
                    self.bounce_modal.bit_depth = BitDepth::Pcm24;
                }

                ui.separator();
                ui.label(
                    egui::RichText::new("🎚 Mastering Loudness, True-Peak Ceiling & Dither")
                        .size(11.5)
                        .strong()
                        .color(ACCENT_FERRIC),
                );

                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.bounce_modal.enable_lufs_norm,
                        "Normalize Integrated Loudness:",
                    );
                    ui.add_enabled(
                        self.bounce_modal.enable_lufs_norm,
                        egui::DragValue::new(&mut self.bounce_modal.target_lufs)
                            .range(-28.0..=-5.0)
                            .speed(0.1)
                            .suffix(" LUFS"),
                    );
                    if ui
                        .small_button("-14 Stream")
                        .on_hover_text("Spotify / Apple Music / YouTube Standard (-14.0 LUFS, -1.0 dBTP)")
                        .clicked()
                    {
                        self.bounce_modal.enable_lufs_norm = true;
                        self.bounce_modal.target_lufs = -14.0;
                        self.bounce_modal.true_peak_ceiling_dbtp = -1.0;
                    }
                    if ui
                        .small_button("-9 Club")
                        .on_hover_text("Loud Electronic / Club Master (-9.0 LUFS, -0.5 dBTP)")
                        .clicked()
                    {
                        self.bounce_modal.enable_lufs_norm = true;
                        self.bounce_modal.target_lufs = -9.0;
                        self.bounce_modal.true_peak_ceiling_dbtp = -0.5;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("True-Peak Ceiling:");
                    ui.add(
                        egui::DragValue::new(&mut self.bounce_modal.true_peak_ceiling_dbtp)
                            .range(-6.0..=0.0)
                            .speed(0.05)
                            .suffix(" dBTP"),
                    );
                    ui.separator();
                    ui.checkbox(
                        &mut self.bounce_modal.tpdf_dither,
                        "Triangular TPDF Dither (16/24-bit)",
                    );
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

                let opts = MasteringOptions {
                    format: self.bounce_modal.format,
                    sample_rate: self.bounce_modal.sample_rate,
                    bit_depth: self.bounce_modal.bit_depth,
                    start_beat: self.bounce_modal.start_beat,
                    end_beat: self.bounce_modal.end_beat,
                    normalize_target_lufs: self
                        .bounce_modal
                        .enable_lufs_norm
                        .then_some(self.bounce_modal.target_lufs),
                    true_peak_ceiling_dbtp: self.bounce_modal.true_peak_ceiling_dbtp,
                    tpdf_dither: self.bounce_modal.tpdf_dither,
                };

                ui.add_space(6.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!(
                                "⚡ Render Master / Stems (.{})",
                                opts.format.extension().to_uppercase()
                            ))
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
                                render_all_stems_with_options(&self.project, &dir, &opts)
                                    .map_err(|e| e.to_string()),
                            );
                        }
                    } else {
                        let ext = opts.format.extension();
                        if let Some(file) = rfd::FileDialog::new()
                            .set_title("Export Master Audio")
                            .add_filter(ext.to_uppercase(), &[ext])
                            .set_file_name(format!("master_mix.{}", ext))
                            .save_file()
                        {
                            self.bounce_modal.last_report = Some(
                                render_master_with_options(&self.project, &file, &opts)
                                    .map_err(|e| e.to_string()),
                            );
                        }
                    }
                }

                if let Some(ref res) = self.bounce_modal.last_report {
                    ui.separator();
                    match res {
                        Ok(rep) => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "✓ Exported {} file(s) ({:.1}s) • Integrated: {:.1} LUFS • True Peak: {:+.2} dBTP",
                                    rep.output_files.len(),
                                    rep.duration_secs,
                                    rep.integrated_lufs,
                                    rep.true_peak_dbtp
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

    fn process_browser_actions(&mut self, browser_actions: Vec<BrowserAction>) {
        let mut need_sync = false;
        for act in browser_actions {
            match act {
                BrowserAction::PreviewSample(buf) => {
                    self.engine.send(EngineCommand::PreviewBuffer(buf));
                }
                BrowserAction::DropSampleOnTrack { name, buffer } => {
                    self.snapshot_before("Drop Browser Sample");
                    let tid = self
                        .selected_track
                        .unwrap_or_else(|| self.project.add_track("Sample Track"));
                    let at = self.project.snap_grid.snap(
                        self.engine.latest_telemetry.playhead_beat,
                        self.project.time_signature,
                    );
                    let beats = self.project.seconds_to_beats(buffer.duration_secs()).max(1.0);
                    if let Some(trk) = self.project.find_track_mut(tid) {
                        let clip = TimelineClip::new_audio(name, at, beats, buffer);
                        self.selected_clip = Some((tid, clip.id));
                        trk.clips.push(clip);
                    }
                    need_sync = true;
                }
                BrowserAction::LoadIntoSampler { name, buffer } => {
                    self.snapshot_before("Load Sample into Sampler");
                    let tid = self
                        .selected_track
                        .unwrap_or_else(|| self.project.add_track("Sampler Track"));
                    let slot = FxSlot::new_sampler_with_buffer(name.clone(), buffer);
                    self.engine.force_reload_sampler(&slot);
                    if let Some(trk) = self.project.find_track_mut(tid) {
                        trk.fx_chain.retain(|f| f.kind != FxKind::SamplePlayer);
                        trk.fx_chain.insert(0, slot);
                    }
                    self.selected_track = Some(tid);
                    self.bottom_tab = BottomTab::FxAndModulators;
                    self.status_banner =
                        format!("Loaded '{}' into 16-Voice Chromatic/Slice Sampler!", name);
                    need_sync = true;
                }
                BrowserAction::ApplyPreset(preset) => {
                    if let Some(tid) = self.selected_track {
                        self.snapshot_before("Load Factory Preset");
                        if let Some(trk) = self.project.find_track_mut(tid) {
                            let mut slot = FxSlot::new(preset.kind);
                            slot.preset_name = preset.name.to_string();
                            slot.wet_mix = preset.wet_mix;
                            slot.params = preset.params.to_vec();
                            trk.fx_chain.push(slot);
                        }
                        self.bottom_tab = BottomTab::FxAndModulators;
                        need_sync = true;
                    }
                }
                BrowserAction::LoadExternalPlugin(plug) => {
                    if let Some(tid) = self.selected_track {
                        self.snapshot_before("Insert External Plugin");
                        let fmt_badge = plug.format.badge();
                        if let Some(trk) = self.project.find_track_mut(tid) {
                            trk.fx_chain.push(FxSlot::new_external_plugin(
                                plug.format,
                                plug.bundle_path,
                                plug.id,
                                plug.name.clone(),
                            ));
                        }
                        self.status_banner =
                            format!("✓ Inserted {} plugin '{}'", fmt_badge, plug.name);
                        self.bottom_tab = BottomTab::FxAndModulators;
                        need_sync = true;
                    }
                }
                BrowserAction::ClearPluginQuarantine => {
                    self.engine.quarantine.clear_all();
                    self.status_banner = "Cleared crashed plugin quarantine list.".into();
                }
                BrowserAction::ImportMidiFile(p) => {
                    self.import_smf_midi_dialog(Some(p));
                }
                BrowserAction::SetupAkaiAndDs10Rig => {
                    self.spawn_akai_and_ds10_rig();
                }
                BrowserAction::RescanMidi => {
                    self.engine.rescan_midi_devices();
                    self.status_banner = if self.engine.midi_port_names.is_empty() {
                        "Rescanned USB MIDI: no controller detected yet.".into()
                    } else {
                        format!(
                            "✓ Connected USB MIDI: {}",
                            self.engine.midi_port_names.join(", ")
                        )
                    };
                }
                BrowserAction::SwitchAudioDevices {
                    output,
                    input,
                    buffer_size,
                    recording_offset_samples,
                } => {
                    self.engine = EngineHandle::start_with_config(
                        &self.project,
                        output.as_deref(),
                        input.as_deref(),
                        buffer_size,
                        recording_offset_samples,
                    );
                    self.status_banner = format!(
                        "Audio Configured • Out: {} • In: {} • Buf: {:?}",
                        self.engine.audio_device_name,
                        self.engine.audio_input_name,
                        buffer_size
                    );
                }
                BrowserAction::SetInputGainDb(db) => {
                    self.engine.set_input_gain_db(db);
                }
                BrowserAction::SetRecordingOffsetSamples(smp) => {
                    self.engine.set_recording_offset_samples(smp);
                    let ms = (smp as f32 * 1000.0) / (self.engine.sample_rate.max(8_000) as f32);
                    self.status_banner = format!(
                        "Recording Latency Compensation set to {} samples ({:.2} ms)",
                        smp, ms
                    );
                }
                BrowserAction::TriggerLoopbackPing => {
                    self.engine.trigger_loopback_ping();
                    self.status_banner =
                        "📡 Sent 2.5ms Loopback Calibration Ping — listening on Line/Mic Input..."
                            .into();
                }
                BrowserAction::LockDetectedBpm(bpm) => {
                    self.snapshot_before("Lock Line-In BPM");
                    self.project.bpm = bpm.clamp(30.0, 280.0);
                    self.status_banner =
                        format!("✓ Locked Project Tempo to DS-10 / Line-In: {:.1} BPM!", bpm);
                    need_sync = true;
                }
                BrowserAction::KaossNote { note, is_note_on } => {
                    self.engine.send(EngineCommand::LiveMidi {
                        target_track: self.selected_track,
                        note,
                        velocity: if is_note_on { 110 } else { 0 },
                        is_note_on,
                    });
                }
                BrowserAction::ProjectMutated => {
                    need_sync = true;
                }
            }
        }
        if need_sync {
            self.sync_engine();
        }
    }
}

impl eframe::App for FerricDawApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.engine.poll_ui(&mut self.capture_inbox);
        if self
            .engine
            .apply_discovered_plugin_params(&mut self.project)
        {
            self.sync_engine();
        }
        self.handle_captured_takes();
        self.handle_midi_learn_poll();
        self.handle_shortcuts_and_musical_typing(ctx);
        self.tick_autosave();

        // Handle Drag-and-Drop of external WAV/FLAC/MP3/OGG/MID files directly from Windows Explorer!
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for path in dropped {
            let is_mid = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| matches!(e.to_ascii_lowercase().as_str(), "mid" | "midi"))
                .unwrap_or(false);
            if is_mid {
                self.import_smf_midi_dialog(Some(path));
            } else if let Ok(buf) = import_audio_file(&path, self.engine.sample_rate) {
                self.snapshot_before("Drop External Audio File");
                let tid = self
                    .selected_track
                    .unwrap_or_else(|| self.project.add_track("Dropped Audio"));
                let beats = self.project.seconds_to_beats(buf.duration_secs()).max(1.0);
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Dropped Audio")
                    .to_string();
                if let Some(trk) = self.project.find_track_mut(tid) {
                    let clip = TimelineClip::new_audio(
                        name,
                        self.engine.latest_telemetry.playhead_beat,
                        beats,
                        buf,
                    );
                    self.selected_clip = Some((tid, clip.id));
                    trk.clips.push(clip);
                }
                self.sync_engine();
            }
        }

        if self.engine.latest_telemetry.is_playing
            || self.engine.latest_telemetry.master.peak_l_db > -65.0
            || self.engine.latest_telemetry.input_peak_db > -65.0
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(45));
        }

        if let Some(cmd) = self.palette.show(ctx) {
            self.execute_palette_command(cmd);
        }
        self.render_bounce_modal(ctx);
        render_quickstart_help_modal(ctx, &mut self.show_help_modal);

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

        // 2. Status Bar
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
                            "▼ Hide Lower Dock"
                        } else {
                            "▲ Show Lower Dock"
                        })
                        .clicked()
                    {
                        self.show_bottom_dock = !self.show_bottom_dock;
                    }
                    ui.separator();
                    if let Some(ref learn) = self.pending_midi_learn {
                        ui.label(
                            egui::RichText::new(format!(
                                "🎯 MIDI LEARN ARMED for '{}' — twist any hardware CC knob (Esc to cancel)",
                                learn.label
                            ))
                            .size(11.0)
                            .strong()
                            .color(ACCENT_AMBER),
                        );
                        if ui.small_button("Cancel").clicked() {
                            self.pending_midi_learn = None;
                        }
                    } else {
                        ui.label(
                            egui::RichText::new(&self.status_banner)
                                .size(11.0)
                                .color(TEXT_DIM),
                        );
                    }
                });
            });

        // 3. Dockable Left Studio Browser (Hardware Rig / DS-10 / LPK25, Samples, Presets & CLAP)
        if self.browser.visible {
            egui::SidePanel::left("ferric_left_browser")
                .resizable(true)
                .default_width(248.0)
                .width_range(215.0..=360.0)
                .frame(
                    egui::Frame::new()
                        .fill(BG_PANEL)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .inner_margin(7.0),
                )
                .show(ctx, |ui| {
                    let mut b_actions = Vec::new();
                    let telem = self.engine.latest_telemetry;
                    let has_lpk = self.engine.has_akai_lpk25();
                    self.browser.ui(
                        ui,
                        self.engine.sample_rate,
                        self.project.bpm,
                        &self.engine.midi_port_names,
                        has_lpk,
                        &self.engine.audio_device_name,
                        &self.engine.audio_input_name,
                        &telem,
                        &mut b_actions,
                    );
                    self.process_browser_actions(b_actions);
                });
        }

        // 4. Lower Studio Dock (Piano Roll + Warp, FX + Sidechain + Learn, DS-10 KAOSS Pad, Mixer, Routing)
        if self.show_bottom_dock {
            egui::TopBottomPanel::bottom("ferric_lower_dock")
                .resizable(true)
                .default_height(275.0)
                .min_height(180.0)
                .max_height(490.0)
                .frame(
                    egui::Frame::new()
                        .fill(BG_PANEL)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .inner_margin(8.0),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for (tab, label) in [
                            (BottomTab::EditorAndComp, "🎹 MIDI / Warp / Comp"),
                            (BottomTab::FxAndModulators, "🎛 FX, Sidechain & Mods"),
                            (BottomTab::KaossPad, "🎮 DS-10 KAOSS X-Y Pad"),
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
                                    egui::RichText::new(format!(
                                        "Track: {}{}",
                                        trk.name,
                                        if trk.frozen { " [❄ FROZEN]" } else { "" }
                                    ))
                                    .size(11.5)
                                    .color(TEXT_MAIN),
                                );
                            }
                        }
                    });
                    ui.separator();

                    let mut dock_actions = Vec::new();
                    let mut kaoss_actions = Vec::new();
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
                            let open_editor_ids: Vec<uuid::Uuid> = self
                                .project
                                .tracks
                                .iter()
                                .flat_map(|t| t.fx_chain.iter())
                                .chain(self.project.master.fx_chain.iter())
                                .filter(|f| {
                                    f.is_external_plugin()
                                        && self.engine.is_plugin_editor_open(f.id)
                                })
                                .map(|f| f.id)
                                .collect();
                            render_fx_and_modulators_panel(
                                ui,
                                &mut self.project,
                                self.selected_track,
                                &mut self.inspect_master_fx,
                                &open_editor_ids,
                                &telem,
                                &mut dock_actions,
                            );
                        }
                        BottomTab::KaossPad => {
                            self.kaoss_pad.ui(
                                ui,
                                &mut self.project,
                                self.selected_track,
                                &mut kaoss_actions,
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

                    if !kaoss_actions.is_empty() {
                        self.process_browser_actions(kaoss_actions);
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
                            DockAction::ArmMidiLearn {
                                track_id,
                                target,
                                label,
                            } => {
                                if let Some(trk) = self.project.find_track_mut(track_id) {
                                    trk.ensure_automation_lane(target.clone());
                                }
                                self.pending_midi_learn = Some(PendingMidiLearn {
                                    track_id,
                                    target,
                                    label,
                                });
                                need_sync = true;
                            }
                            DockAction::ReloadSamplerFx(fx_slot) => {
                                self.engine.force_reload_sampler(&fx_slot);
                                need_sync = true;
                            }
                            DockAction::ToggleExternalPluginEditor(fx_id) => {
                                if self.engine.is_plugin_editor_open(fx_id) {
                                    self.engine.close_plugin_editor(fx_id);
                                    self.status_banner = "Closed floating plugin editor.".into();
                                } else {
                                    match self.engine.open_plugin_editor(fx_id) {
                                        Ok(()) => {
                                            self.status_banner =
                                                "🗔 Opened native Win32 HWND floating plugin editor!"
                                                    .into();
                                        }
                                        Err(err) => {
                                            self.status_banner = format!("Plugin GUI: {}", err);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if need_sync {
                        self.sync_engine();
                    }
                });
        }

        // 5. Central Hybrid Session Clip Launcher + Linear Multitrack Arrangement + Bezier Automation
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
                            if self.project.delete_clip(
                                track_id,
                                clip_id,
                                self.arrangement.ripple_edit,
                            ) {
                                if self.selected_clip == Some((track_id, clip_id)) {
                                    self.selected_clip = None;
                                }
                                need_sync = true;
                            }
                        }
                        ArrangementAction::ImportAudioToTrack(tid) => {
                            self.import_audio_to_track(tid);
                        }
                        ArrangementAction::ToggleFreezeTrack(tid) => {
                            self.toggle_freeze_track(tid);
                        }
                        ArrangementAction::MoveClipToTrack {
                            from_track,
                            to_track,
                            clip_id,
                        } => {
                            self.snapshot_before("Move Clip Between Tracks");
                            if self
                                .project
                                .move_clip_to_track(from_track, to_track, clip_id)
                            {
                                self.selected_track = Some(to_track);
                                self.selected_clip = Some((to_track, clip_id));
                                need_sync = true;
                            }
                        }
                        ArrangementAction::DeleteTrack(tid) => {
                            self.snapshot_before("Delete Track");
                            if self.project.delete_track(tid) {
                                if self.selected_track == Some(tid) {
                                    self.selected_track =
                                        self.project.tracks.first().map(|t| t.id);
                                }
                                if self.selected_clip.map(|(t, _)| t) == Some(tid) {
                                    self.selected_clip = None;
                                }
                                need_sync = true;
                            }
                        }
                        ArrangementAction::DuplicateTrack(tid) => {
                            self.snapshot_before("Duplicate Track");
                            if let Some(new_tid) = self.project.duplicate_track(tid) {
                                self.selected_track = Some(new_tid);
                                need_sync = true;
                            }
                        }
                        ArrangementAction::MoveTrack { track_id, delta } => {
                            self.snapshot_before("Reorder Track");
                            if self.project.move_track(track_id, delta) {
                                need_sync = true;
                            }
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
            .with_title("FerricDAW v1.0 — Full-Fledged Open-Source Rust Hybrid DAW")
            .with_inner_size([1520.0, 900.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_drag_and_drop(true)
            .with_icon(std::sync::Arc::new(build_app_icon())),
        ..Default::default()
    };
    eframe::run_native(
        "FerricDAW",
        native_options,
        Box::new(|cc| Ok(Box::new(FerricDawApp::new(cc)))),
    )
}
