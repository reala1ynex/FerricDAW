#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod arrangement;
mod browser;
mod function_test;
mod help_modal;
mod host_fingerprint;
mod keyboard_pads;
mod mixer;
mod palette;
mod piano_roll;
mod theme;
mod tutorial_coach;
mod update;

use arrangement::{ArrangementAction, ArrangementState, NewTrackKind};
use browser::{BrowserAction, KaossPadState, StudioBrowserState, TapTempoTracker};
use keyboard_pads::{KeyboardPadRig, PadBSource};
use help_modal::{build_app_icon, render_quickstart_help_modal};
use tutorial_coach::{TutorialAction, TutorialCoach};
use eframe::egui;
use egui::{Color32, Context, Key, Stroke};
use ferric_engine::{
    audio_buffer_from_capture, import_audio_file, load_project_bundle,
    preferred_line_input_name, preferred_speakers_output_name, realtek_combo_jack_risk,
    render_all_stems_with_options, render_master_with_options, render_track_freeze_buffer,
    save_project_bundle, BitDepth, BounceReport, CapturedTakeEvent, DiscoveredExternalPlugin,
    EngineCommand, EngineHandle, ExportFormat, MasteringOptions,
};
use ferric_model::{
    build_demo_project, build_tutorial_project_through, import_smf_file, night_current_step_complete,
    AutomationTarget, ClipId, FxKind, FxSlot, GrooveTemplate, HistoryManager, InputSource,
    MidiLearnMapping, MixSnapshot, ModulatorSlot, NightCurrentStep, Project, Take, TakeContent,
    TimelineClip, Track, TrackId, VelocityAccentShape, VoicingStyle,
};
use host_fingerprint::{compute_host_seed, format_host_seed, random_host_seed};
use mixer::{render_mixer_console, render_routing_matrix};
use palette::{CommandPalette, PaletteCommand};
use piano_roll::{render_fx_and_modulators_panel, DockAction, PianoRollState};
use std::path::PathBuf;
use theme::{
    apply_studio_theme, chrome_card, chrome_row_label, chrome_section_title, pill_toggle,
    ACCENT_AMBER, ACCENT_FERRIC, ACCENT_RED, ACCENT_TEAL, BG_DEEP, BG_ELEVATED, BG_PANEL,
    BORDER_SUBTLE, TEXT_DIM, TEXT_MAIN,
};
use update::UpdateService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BottomTab {
    EditorAndComp,
    FxAndModulators,
    KaossPad,
    MixingConsole,
    RoutingMatrix,
    LaunchPad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HardwareRigPreset {
    UsbMidiAndLineIn,
    AkaiLpk25AndDs10,
    MidiKeyboardOnly,
    LineInOnly,
}

impl HardwareRigPreset {
    fn snapshot_label(self) -> &'static str {
        match self {
            Self::UsbMidiAndLineIn => "Spawn Hardware Rig (MIDI + Line-In)",
            Self::AkaiLpk25AndDs10 => "Spawn Hardware Rig (keyboard + handheld tip)",
            Self::MidiKeyboardOnly => "Spawn Hardware Rig (MIDI only)",
            Self::LineInOnly => "Spawn Hardware Rig (Line-In only)",
        }
    }

    fn includes_midi(self) -> bool {
        !matches!(self, Self::LineInOnly)
    }

    fn includes_line_in(self) -> bool {
        !matches!(self, Self::MidiKeyboardOnly)
    }

    fn status_banner(self) -> &'static str {
        match self {
            Self::UsbMidiAndLineIn => {
                "Spawned Hardware Rig — USB MIDI + Line-In (Speakers out + Line-In / Ext Mic preferred)"
            }
            Self::AkaiLpk25AndDs10 => {
                "Spawned Hardware Rig tip preset — same generic MIDI + Line-In tracks (tip naming only)"
            }
            Self::MidiKeyboardOnly => "Spawned USB MIDI Synth track — arm and play any controller",
            Self::LineInOnly => {
                "Spawned Line-In Capture track — Speakers out + Line-In / Ext Mic preferred"
            }
        }
    }
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
    /// Last successful master bounce path (for A/B audition).
    last_master_path: Option<PathBuf>,
    /// Golden Bounce: reference WAV (defaults to Night Current demo).
    reference_path: Option<PathBuf>,
    reference_lufs: Option<f32>,
    reference_true_peak: Option<f32>,
    reference_duration: Option<f64>,
    reference_peaks: Vec<f32>,
    bounce_peaks: Vec<f32>,
    loudness_match_ab: bool,
    show_golden_panel: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchQuantize {
    Bar,
    Beat,
    Quarter,
}

impl LaunchQuantize {
    fn label(self) -> &'static str {
        match self {
            Self::Bar => "1 Bar",
            Self::Beat => "1 Beat",
            Self::Quarter => "1/4",
        }
    }

    fn quantize_beat(self, beat: f64, beats_per_bar: f64) -> f64 {
        let grid = match self {
            Self::Bar => beats_per_bar.max(1.0),
            Self::Beat => 1.0,
            Self::Quarter => 0.25,
        };
        (beat / grid).ceil() * grid
    }
}

struct MixMorphState {
    from: MixSnapshot,
    to: MixSnapshot,
    start_beat: f64,
    duration_beats: f64,
    last_applied_t: f32,
}


struct PendingMidiLearn {
    track_id: TrackId,
    target: AutomationTarget,
    label: String,
}

/// Destructive session swap waiting on Save / Discard / Cancel.
#[derive(Debug, Clone)]
enum PendingDestructive {
    NewEmpty,
    LoadDemo,
    OpenPath(PathBuf),
    TutorialApply(NightCurrentStep),
    /// User tried to close the window with unsaved / in-progress work.
    QuitApp,
}

#[derive(Debug, Clone, Copy)]
enum FirstRunChoice {
    Tutorial,
    Afterglow,
    Empty,
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
    /// Persisted lower-dock height (egui alone stores content height and snaps back).
    bottom_dock_height: f32,
    /// One-shot viewport clamp so a tiny restored window can't stick in the corner.
    viewport_bootstrapped: bool,
    inspect_master_fx: bool,
    selected_track: Option<TrackId>,
    selected_clip: Option<(TrackId, ClipId)>,
    clipboard_clip: Option<TimelineClip>,
    recent_projects: Vec<PathBuf>,
    show_help_modal: bool,
    tutorial: TutorialCoach,
    pending_destructive: Option<PendingDestructive>,
    dirty_since_autosave: bool,
    dirty_since_project_save: bool,
    last_autosave_inst: std::time::Instant,
    pending_midi_learn: Option<PendingMidiLearn>,
    keyboard_pads: KeyboardPadRig,
    bounce_modal: BounceModalState,
    show_first_run: bool,
    status_banner: String,
    capture_inbox: Vec<CapturedTakeEvent>,
    /// Scene Launch Pad focus — number row fires scenes when true.
    launch_pad_focused: bool,
    launch_quantize: LaunchQuantize,
    mix_morph_enabled: bool,
    mix_morph_beats: f64,
    mix_morph: Option<MixMorphState>,
    pending_scene_launch: Option<(usize, f64)>,
    /// Plugin Playground: sandboxed audition candidate awaiting Insert.
    playground_plugin: Option<DiscoveredExternalPlugin>,
    playground_ok: bool,
    updater: UpdateService,
    show_update_modal: bool,
    quit_for_update: bool,
}

impl FerricDawApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_studio_theme(&cc.egui_ctx);
        Self::new_core(Self::cli_open_path_from_args())
    }

    fn cli_open_path_from_args() -> Option<PathBuf> {
        std::env::args_os()
            .skip(1)
            .map(PathBuf::from)
            .find(|p| {
                let s = p.to_string_lossy();
                !s.starts_with('-') && p.exists()
            })
    }

    /// Headless constructor for `--function-test` (no egui window required).
    fn new_core(cli_path: Option<PathBuf>) -> Self {
        let first_run = !Self::first_run_done_marker().exists() && cli_path.is_none();

        // First launch: empty + chooser. Later launches: empty (playground via Ferric menu).
        let empty_session = || {
            let mut p = Project::new_empty("Untitled Session");
            let _ = p.add_track("01 Audio / MIDI");
            p
        };
        let (initial_project, show_first_run) = if first_run {
            (empty_session(), true)
        } else {
            (empty_session(), false)
        };

        let engine = EngineHandle::start(&initial_project);
        let project = if engine.sample_rate != 48_000 {
            empty_session()
        } else {
            initial_project
        };

        let lpk_badge = if !engine.midi_port_names.is_empty() {
            " • USB MIDI Connected!"
        } else {
            ""
        };

        let mut app = Self {
            selected_track: project.tracks.first().map(|t| t.id),
            selected_clip: None,
            bounce_modal: BounceModalState {
                open: false,
                export_stems: false,
                format: ExportFormat::Wav,
                sample_rate: engine.sample_rate,
                bit_depth: BitDepth::Pcm24,
                start_beat: 0.0,
                end_beat: project.project_end_beat().max(64.0),
                enable_lufs_norm: true,
                target_lufs: -14.0,
                true_peak_ceiling_dbtp: -1.0,
                tpdf_dither: true,
                last_report: None,
                last_master_path: None,
                reference_path: None,
                reference_lufs: None,
                reference_true_peak: None,
                reference_duration: None,
                reference_peaks: Vec::new(),
                bounce_peaks: Vec::new(),
                loudness_match_ab: true,
                show_golden_panel: false,
            },
            status_banner: if show_first_run {
                format!(
                    "Welcome — pick how to start • Out: {} • In: {}{}",
                    engine.audio_device_name, engine.audio_input_name, lpk_badge
                )
            } else {
                format!(
                    "v{} ready • Out: {} ({} Hz) • In: {}{} • Space: Play • Ctrl+K: Palette",
                    env!("CARGO_PKG_VERSION"),
                    engine.audio_device_name,
                    engine.sample_rate,
                    engine.audio_input_name,
                    lpk_badge
                )
            },
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
            bottom_dock_height: 300.0,
            viewport_bootstrapped: false,
            inspect_master_fx: false,
            clipboard_clip: None,
            recent_projects: Self::load_recent_projects_list(),
            show_help_modal: false,
            tutorial: TutorialCoach::default(),
            pending_destructive: None,
            dirty_since_autosave: false,
            dirty_since_project_save: false,
            last_autosave_inst: std::time::Instant::now(),
            pending_midi_learn: None,
            keyboard_pads: KeyboardPadRig::default(),
            show_first_run,
            capture_inbox: Vec::new(),
            launch_pad_focused: false,
            launch_quantize: LaunchQuantize::Bar,
            mix_morph_enabled: false,
            mix_morph_beats: 4.0,
            mix_morph: None,
            pending_scene_launch: None,
            playground_plugin: None,
            playground_ok: false,
            updater: UpdateService::default(),
            show_update_modal: false,
            quit_for_update: false,
        };
        app.engine.sync_project(&app.project);
        if let Some(note) = app.engine.driver_status_note {
            app.browser.driver_honesty_note = Some(note.to_string());
        }
        app.browser.live_exclusive = app.engine.has_exclusive_output();
        if UpdateService::feed_configured() {
            app.updater.check_async();
        }

        // Support launching `ferricdaw.exe my_song.ferric` (or `.json` / `.wav` / `.flac` / `.mid`) from CLI / File Association!
        if let Some(arg_path) = cli_path {
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
                app.show_first_run = false;
                app.mark_first_run_done();
            }
        }
        app
    }

    fn ferric_appdata_dir() -> PathBuf {
        let base = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("FerricDAW")
    }

    fn first_run_done_marker() -> PathBuf {
        Self::ferric_appdata_dir().join("first_run_done")
    }

    fn mark_first_run_done(&self) {
        let p = Self::first_run_done_marker();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(p, b"1");
    }

    fn recent_projects_file() -> PathBuf {
        Self::ferric_appdata_dir().join("recent_projects.txt")
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
        self.dirty_since_project_save = true;
    }

    fn sync_engine(&mut self) {
        self.engine.sync_project(&self.project);
        self.dirty_since_autosave = true;
        self.dirty_since_project_save = true;
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
            match save_project_bundle(&mut self.project, &path) {
                Ok(()) => {
                    self.dirty_since_autosave = false;
                    self.last_autosave_inst = std::time::Instant::now();
                }
                Err(e) => {
                    self.status_banner = format!("Autosave failed: {e}");
                    self.last_autosave_inst = std::time::Instant::now();
                }
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

    fn split_clip_to_stems(&mut self, track_id: TrackId, clip_id: ClipId) {
        let Some(src_clip) = self
            .project
            .find_track(track_id)
            .and_then(|t| t.clips.iter().find(|c| c.id == clip_id))
            .cloned()
        else {
            return;
        };
        let take = src_clip
            .takes
            .get(src_clip.active_take)
            .or_else(|| src_clip.takes.first());
        let Some(Take {
            content: TakeContent::Audio { buffer },
            ..
        }) = take
        else {
            self.status_banner = "Stem split needs an audio take.".into();
            return;
        };

        self.snapshot_before("Split Audio to 4 Stems");
        let stems = ferric_dsp::split_audio_to_4_stems(buffer);
        let start = src_clip.start_beat;
        let len = src_clip.length_beats;
        let n_scenes = self.project.scenes.len();
        let labels = [
            ("Drums / Perc", stems.drums_percussion, 0usize),
            ("Sub / Bass", stems.sub_bass, 1),
            ("Mids / Keys", stems.mids_keys_vocals, 2),
            ("Highs / Air", stems.highs_air, 3),
        ];
        for (name, buf, color) in labels {
            let mut trk = Track::new(format!("Stem · {name}"), color, n_scenes);
            trk.clips
                .push(TimelineClip::new_audio(format!("{name}"), start, len, buf));
            self.project.tracks.push(trk);
        }
        self.status_banner =
            "Split into 4 stem tracks (Drums / Bass / Mids / Highs) via HPSS.".into();
        self.sync_engine();
    }

    /// 1-Click Hardware Studio Rig presets (generic MIDI / Line-In first).
    fn spawn_hardware_studio_rig(&mut self) {
        self.spawn_hardware_rig_preset(HardwareRigPreset::UsbMidiAndLineIn);
    }

    fn spawn_hardware_rig_preset(&mut self, preset: HardwareRigPreset) {
        self.snapshot_before(preset.snapshot_label());
        self.engine.rescan_midi_devices();

        let mut focus = None;
        if preset.includes_midi() {
            let name = match preset {
                HardwareRigPreset::AkaiLpk25AndDs10 => "USB MIDI Synth (keyboard tip)",
                _ => "USB MIDI Synth",
            };
            let midi_id = self.project.add_track(name);
            if let Some(t) = self.project.find_track_mut(midi_id) {
                t.color = [232, 114, 62];
                t.input_source = InputSource::MidiAll;
                t.record_arm = true;
                let mut poly = FxSlot::new(FxKind::PolySynth);
                poly.set_param(0, 0.0);
                poly.set_param(1, 42.0);
                poly.set_param(2, 2400.0);
                poly.set_param(3, 0.38);
                t.fx_chain.push(poly);
                t.fx_chain.push(FxSlot::new(FxKind::StereoDelay));
            }
            focus = Some(midi_id);
        }

        if preset.includes_line_in() {
            let name = match preset {
                HardwareRigPreset::AkaiLpk25AndDs10 => "Line-In Capture (handheld tip name)",
                _ => "Line-In Capture",
            };
            let line_id = self.project.add_track(name);
            if let Some(t) = self.project.find_track_mut(line_id) {
                t.color = [66, 179, 165];
                t.input_source = InputSource::AudioMono { channel: 0 };
                t.record_arm = true;
                t.input_monitoring = true;
                t.gain_db = 0.0;
                let mut eq = FxSlot::new(FxKind::ParametricEq);
                eq.set_param(0, 100.0);
                eq.set_param(2, 0.0);
                eq.set_param(10, -3.0);
                eq.set_param(11, -1.0);
                let mut comp = FxSlot::new(FxKind::Compressor);
                comp.preset_name = "Line-In Leveler".into();
                comp.set_param(0, -24.0);
                comp.set_param(1, 2.5);
                comp.set_param(2, 10.0);
                comp.set_param(3, 150.0);
                comp.set_param(5, 3.0);
                t.fx_chain.push(eq);
                t.fx_chain.push(comp);
            }
            if focus.is_none() {
                focus = Some(line_id);
            }
            self.apply_line_in_safe_audio_routing();
        }

        self.selected_track = focus;
        self.bottom_tab = BottomTab::KaossPad;
        self.show_bottom_dock = true;
        self.sync_engine();
        self.status_banner = preset.status_banner().into();
    }

    /// Prefer Speakers output + Line-In / External Mic capture; clean Line-In monitor chain.
    fn apply_line_in_safe_audio_routing(&mut self) {
        let speakers = preferred_speakers_output_name();
        let line_in = preferred_line_input_name();
        if let Some(ref s) = speakers {
            self.browser.selected_out = Some(s.clone());
        }
        if let Some(ref i) = line_in {
            self.browser.selected_in = Some(i.clone());
        } else {
            let fallback = self
                .browser
                .audio_inputs
                .iter()
                .find(|n| {
                    let l = n.to_lowercase();
                    l.contains("external microphone") || l.contains("line in")
                })
                .cloned();
            self.browser.selected_in = fallback;
        }
        self.browser.input_gain_db = 6.0;
        for t in &mut self.project.tracks {
            let name = t.name.to_lowercase();
            if !(name.contains("line-in") || name.contains("line in") || name.contains("capture"))
            {
                continue;
            }
            t.input_source = InputSource::AudioMono { channel: 0 };
            t.record_arm = true;
            t.input_monitoring = true;
            t.gain_db = 0.0;
            t.fx_chain
                .retain(|fx| fx.kind != FxKind::NoiseGate && fx.kind != FxKind::TapeSaturator);
            if let Some(eq) = t.fx_chain.iter_mut().find(|fx| fx.kind == FxKind::ParametricEq) {
                eq.set_param(0, 100.0);
                eq.set_param(2, 0.0);
                eq.set_param(10, -3.0);
                eq.set_param(11, -1.0);
            } else {
                let mut eq = FxSlot::new(FxKind::ParametricEq);
                eq.set_param(0, 100.0);
                eq.set_param(10, -3.0);
                eq.set_param(11, -1.0);
                t.fx_chain.insert(0, eq);
            }
            if let Some(comp) = t.fx_chain.iter_mut().find(|fx| fx.kind == FxKind::Compressor) {
                comp.set_param(0, -24.0);
                comp.set_param(1, 2.5);
                comp.set_param(5, 3.0);
            } else {
                let mut comp = FxSlot::new(FxKind::Compressor);
                comp.preset_name = "Line-In Leveler".into();
                comp.set_param(0, -24.0);
                comp.set_param(1, 2.5);
                comp.set_param(2, 10.0);
                comp.set_param(3, 150.0);
                comp.set_param(5, 3.0);
                let insert_at = t
                    .fx_chain
                    .iter()
                    .position(|fx| fx.kind == FxKind::ParametricEq)
                    .map(|i| i + 1)
                    .unwrap_or(0);
                t.fx_chain.insert(insert_at, comp);
            }
        }
        self.engine = EngineHandle::start_with_config(
            &self.project,
            self.browser.selected_out.as_deref(),
            self.browser.selected_in.as_deref(),
            self.browser.selected_buffer_size,
            self.browser.recording_offset_samples,
            self.browser.selected_audio_driver,
        );
        self.engine.set_input_gain_db(6.0);
        let in_name = self.engine.audio_input_name.clone();
        if in_name.to_lowercase().contains("steam")
            || in_name.to_lowercase().contains("array")
            || in_name.to_lowercase().contains("nvidia")
        {
            self.status_banner = format!(
                "⚠ Capture device is '{in_name}' — switch Input to External Microphone / Line-In or Windows will not hear the line source."
            );
        } else {
            self.status_banner = format!(
                "Line-In capture: {in_name} @ +6 dB • Mic Boost 0% • hiss shelf cut (raise source volume, not Mic Boost)"
            );
        }
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
                                "Retroactive Line-In / Audio Riff"
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

    fn start_night_current_tutorial(&mut self) {
        self.show_help_modal = false;
        self.show_first_run = false;
        self.tutorial.start();
        self.status_banner =
            "Night Current tutorial open — coach bottom-right (Begin Session → Next)"
                .into();
    }

    fn apply_first_run_choice(&mut self, choice: FirstRunChoice) {
        self.mark_first_run_done();
        self.show_first_run = false;
        match choice {
            FirstRunChoice::Tutorial => {
                self.project = Project::new_empty("Untitled Session");
                let tid = self.project.add_track("01 Audio / MIDI");
                self.selected_track = Some(tid);
                self.selected_clip = None;
                self.project_path = None;
                self.sync_engine();
                self.start_night_current_tutorial();
            }
            FirstRunChoice::Afterglow => {
                self.execute_destructive(PendingDestructive::LoadDemo);
                self.status_banner =
                    "Afterglow Copper playground loaded — open 🎓 Tutorial anytime for Night Current."
                        .into();
            }
            FirstRunChoice::Empty => {
                self.execute_destructive(PendingDestructive::NewEmpty);
                self.status_banner =
                    "Empty session — Ctrl+K or 🎓 Tutorial when you want a guided song.".into();
            }
        }
    }

    fn render_first_run_modal(&mut self, ctx: &Context) {
        if !self.show_first_run {
            return;
        }
        let mut choice = None;
        egui::Window::new("Welcome to FerricDAW")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(420.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.5_f32, ACCENT_FERRIC))
                    .corner_radius(8)
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("How do you want to start?")
                        .size(16.0)
                        .strong()
                        .color(ACCENT_FERRIC),
                );
                ui.label(
                    egui::RichText::new(
                        "Plugins that crash run in a worker process — not the DAW. Pick a path:",
                    )
                    .size(12.0)
                    .color(TEXT_DIM),
                );
                ui.add_space(12.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 36.0],
                        egui::Button::new(
                            egui::RichText::new("🎓 Night Current Tutorial")
                                .strong()
                                .color(ACCENT_TEAL),
                        ),
                    )
                    .on_hover_text("Guided dark-techno song — coach walks you end-to-end")
                    .clicked()
                {
                    choice = Some(FirstRunChoice::Tutorial);
                }
                ui.add_space(6.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("Afterglow Copper playground"),
                    )
                    .on_hover_text("Full demo session to poke around")
                    .clicked()
                {
                    choice = Some(FirstRunChoice::Afterglow);
                }
                ui.add_space(6.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("Empty session"),
                    )
                    .clicked()
                {
                    choice = Some(FirstRunChoice::Empty);
                }
            });
        if let Some(c) = choice {
            self.apply_first_run_choice(c);
        }
    }

    fn session_has_user_work(&self) -> bool {
        self.dirty_since_project_save
            || self.project_path.is_some()
            || self.project.chord_track.len() > 2
            || self
                .project
                .tracks
                .iter()
                .any(|t| !t.clips.is_empty() || t.fx_chain.len() > 1)
    }

    fn request_destructive(&mut self, action: PendingDestructive) {
        if self.session_has_user_work() {
            self.pending_destructive = Some(action);
        } else {
            self.execute_destructive(action);
        }
    }

    fn execute_destructive(&mut self, action: PendingDestructive) {
        match action {
            PendingDestructive::NewEmpty => {
                self.snapshot_before("New Empty Session");
                self.project = Project::new_empty("Untitled Session");
                let tid = self.project.add_track("01 Audio / MIDI");
                self.selected_track = Some(tid);
                self.selected_clip = None;
                self.project_path = None;
                self.sync_engine();
                self.dirty_since_project_save = false;
                self.status_banner = "New empty session.".into();
            }
            PendingDestructive::LoadDemo => {
                self.snapshot_before("Load Demo Session");
                self.project = build_demo_project(self.engine.sample_rate);
                self.selected_track = self.project.tracks.first().map(|t| t.id);
                self.selected_clip = None;
                self.project_path = None;
                self.sync_engine();
                self.status_banner = "Loaded Afterglow Copper playground.".into();
            }
            PendingDestructive::OpenPath(p) => {
                self.open_project_from_path(&p);
            }
            PendingDestructive::TutorialApply(step) => {
                self.apply_tutorial_step(step);
                self.status_banner = format!(
                    "Tutorial step {} applied — Space to hear, or Play demo WAV to A/B",
                    step.letter()
                );
            }
            PendingDestructive::QuitApp => {
                self.quit_for_update = true;
            }
        }
    }

    fn handle_tutorial_action(&mut self, action: TutorialAction) {
        match action {
            TutorialAction::None => {}
            TutorialAction::Close => {
                self.tutorial.open = false;
            }
            TutorialAction::Restart => {
                self.tutorial.start();
            }
            TutorialAction::OpenBounce => {
                self.bounce_modal.start_beat = 0.0;
                self.bounce_modal.end_beat = 64.0;
                self.bounce_modal.open = true;
            }
            TutorialAction::PlayReference => {
                self.play_night_current_demo_reference();
            }
            TutorialAction::Back => {
                self.tutorial.step = self.tutorial.step.prev();
            }
            TutorialAction::Next => {
                // Never force a project wipe — only "Begin Session" / "Do this step" mutate.
                self.tutorial.step = self.tutorial.step.next();
            }
            TutorialAction::ApplyStep => {
                let mut step = self.tutorial.step;
                if step == NightCurrentStep::Welcome {
                    step = NightCurrentStep::Session;
                    self.tutorial.step = step;
                }
                if step.is_build_step() {
                    // Already built through this step — don't wipe manual edits with a rebuild.
                    if night_current_step_complete(&self.project, step) {
                        self.status_banner = format!(
                            "Step {} already in place — no rebuild. Next when ready.",
                            step.letter()
                        );
                    } else {
                        self.request_destructive(PendingDestructive::TutorialApply(step));
                    }
                }
            }
        }
    }

    fn apply_tutorial_step(&mut self, step: NightCurrentStep) {
        let label: &'static str = match step {
            NightCurrentStep::Welcome => "Tutorial: Welcome",
            NightCurrentStep::Session => "Tutorial: Session",
            NightCurrentStep::Chords => "Tutorial: Chords",
            NightCurrentStep::Harmony => "Tutorial: Harmony",
            NightCurrentStep::Drums => "Tutorial: Drums",
            NightCurrentStep::Hook => "Tutorial: Hook",
            NightCurrentStep::Counter => "Tutorial: Counter",
            NightCurrentStep::Sidechain => "Tutorial: Sidechain",
            NightCurrentStep::Mix => "Tutorial: Mix",
            NightCurrentStep::Bounce => "Tutorial: Bounce",
            NightCurrentStep::Done => "Tutorial: Done",
        };
        self.snapshot_before(label);
        self.project = build_tutorial_project_through(self.engine.sample_rate, step);
        self.project.tutorial_step_reached = self
            .project
            .tutorial_step_reached
            .max(step.index() as u8);
        self.project_path = None;
        self.selected_track = self.project.tracks.first().map(|t| t.id);
        self.selected_clip = None;
        self.bounce_modal.end_beat = 64.0;
        self.dirty_since_autosave = true;
        self.dirty_since_project_save = true;
        self.sync_engine();
    }

    fn play_night_current_demo_reference(&mut self) {
        let Some(path) = self.find_night_current_demo_path() else {
            self.status_banner =
                "demo/NightCurrent-Demo.wav not found next to ferricdaw.exe — check the release zip."
                    .into();
            return;
        };
        match import_audio_file(&path, self.engine.sample_rate) {
            Ok(buf) => {
                self.engine.send(EngineCommand::PreviewBuffer(buf));
                self.status_banner = format!("Playing reference demo — {}", path.display());
            }
            Err(e) => {
                self.status_banner = format!("Demo WAV found but failed to load: {e}");
            }
        }
    }

    fn render_destructive_confirm_modal(&mut self, ctx: &Context) {
        let Some(pending) = self.pending_destructive.clone() else {
            return;
        };
        let title = match &pending {
            PendingDestructive::NewEmpty => "Replace session with empty?",
            PendingDestructive::LoadDemo => "Load Afterglow Copper playground?",
            PendingDestructive::OpenPath(_) => "Open another session?",
            PendingDestructive::TutorialApply(step) => {
                if *step == NightCurrentStep::Session {
                    "Begin Night Current session?"
                } else {
                    "Apply tutorial step (rebuilds song)?"
                }
            }
            PendingDestructive::QuitApp => "Quit FerricDAW with unsaved work?",
        };
        let detail = match &pending {
            PendingDestructive::TutorialApply(_) => {
                "This rebuilds the tutorial project through the current step and replaces clips/FX. Save first if you care about the current session."
            }
            PendingDestructive::QuitApp => {
                "Your session has unsaved or in-progress work. Save before quitting, discard and quit, or cancel."
            }
            _ => "You have unsaved or in-progress work. Save before continuing, discard, or cancel.",
        };

        let mut open = true;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.5_f32, ACCENT_FERRIC))
                    .corner_radius(8)
                    .inner_margin(14.0),
            )
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(detail).color(TEXT_MAIN));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(egui::RichText::new("Save…").strong().color(ACCENT_TEAL))
                        .clicked()
                    {
                        let saved = self.save_project_dialog();
                        self.continue_after_save(saved, pending.clone());
                        // A cancelled or failed save keeps the pending action and session intact.
                    }
                    if ui
                        .button(egui::RichText::new("Discard & continue").color(ACCENT_AMBER))
                        .clicked()
                    {
                        self.pending_destructive = None;
                        self.execute_destructive(pending.clone());
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_destructive = None;
                    }
                });
            });
        if !open {
            self.pending_destructive = None;
        }
    }

    fn continue_after_save(&mut self, saved: bool, pending: PendingDestructive) {
        if saved {
            self.pending_destructive = None;
            self.execute_destructive(pending);
        }
    }

    fn execute_palette_command(&mut self, cmd: PaletteCommand) {
        match cmd {
            PaletteCommand::TogglePlay => self.toggle_play(),
            PaletteCommand::ToggleRecord => self.toggle_record(),
            PaletteCommand::SetupHardwareRig => self.spawn_hardware_studio_rig(),
            PaletteCommand::ToggleFreezeSelectedTrack => {
                if let Some(tid) = self.selected_track {
                    self.toggle_freeze_track(tid);
                }
            }
            PaletteCommand::CaptureRetroactive => {
                self.engine
                    .send(EngineCommand::CaptureRetroactive { seconds_back: 15.0 });
                self.status_banner =
                    "Requested 15s Retroactive Capture (USB MIDI, Musical Typing, or Line-In)"
                        .into();
            }
            PaletteCommand::AddUniversalTrack => {
                self.spawn_track_of_kind(NewTrackKind::Universal);
            }
            PaletteCommand::AddWavetableSynthTrack => {
                self.snapshot_before("Add FerricWave Wavetable Synth Track");
                let id = self.project.add_track(format!(
                    "{:02} FerricWave Cloud",
                    self.project.tracks.len() + 1
                ));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    t.fx_chain.push(FxSlot::new(FxKind::WavetableSynth));
                    t.fx_chain.push(FxSlot::new(FxKind::MultibandOtt));
                    t.fx_chain.push(FxSlot::new(FxKind::ConvolutionIr));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddSynthTrack => {
                self.snapshot_before("Add PolySynth Track");
                let id = self
                    .project
                    .add_track(format!("{:02} PolySynth", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    t.fx_chain.push(FxSlot::new(FxKind::PolySynth));
                    t.fx_chain.push(FxSlot::new(FxKind::StereoDelay));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddOrganTrack => {
                self.snapshot_before("Add Drawbar Organ Track");
                let id = self.project.add_track(format!("{:02} Drawbar Organ", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    t.fx_chain.push(FxSlot::new(FxKind::DrawbarOrgan));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddElectricPianoTrack => {
                self.snapshot_before("Add Electric Piano Track");
                let id = self.project.add_track(format!("{:02} Electric Piano", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    t.fx_chain.push(FxSlot::new(FxKind::ElectricPiano));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddMonoBassTrack => {
                self.snapshot_before("Add MonoBass Track");
                let id = self
                    .project
                    .add_track(format!("{:02} MonoBass", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.color = [180, 72, 48];
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    let mut bass = FxSlot::new(FxKind::MonoBass);
                    bass.preset_name = "Dark Sub MonoBass".into();
                    bass.set_param(0, 0.0);
                    bass.set_param(1, 55.0);
                    bass.set_param(2, 220.0);
                    bass.set_param(9, 62.0);
                    t.fx_chain.push(bass);
                    t.fx_chain.push(FxSlot::new(FxKind::Compressor));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddPrismTrack => {
                self.snapshot_before("Add Ferric Prism Track");
                let id = self
                    .project
                    .add_track(format!("{:02} Ferric Prism", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.color = [186, 108, 226];
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    let mut prism = FxSlot::new(FxKind::PrismSynth);
                    prism.preset_name = "Glass Pad Morph".into();
                    prism.set_param(0, 58.0);
                    prism.set_param(1, 48.0);
                    prism.set_param(2, 36.0);
                    t.fx_chain.push(prism);
                    t.fx_chain.push(FxSlot::new(FxKind::PlateHallReverb));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddHostDnaTrack => {
                self.snapshot_before("Add HostDNA Track");
                let seed = compute_host_seed();
                let id = self.project.add_track(format!(
                    "{:02} HostDNA {}",
                    self.project.tracks.len() + 1,
                    format_host_seed(seed)
                ));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.color = [72, 168, 140];
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    t.fx_chain.push(FxSlot::new_host_dna(seed));
                    t.fx_chain.push(FxSlot::new(FxKind::PlateHallReverb));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.status_banner = format!(
                    "HostDNA Init from this PC · seed {} (editable; paste/reseed anytime)",
                    format_host_seed(seed)
                );
                self.sync_engine();
            }
            PaletteCommand::SpawnDualKeyboardStudio => self.spawn_dual_keyboard_studio(),
            PaletteCommand::AddPadTrack => {
                self.snapshot_before("Add Soft Pad Track");
                let id = self
                    .project
                    .add_track(format!("{:02} Soft Pad", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.color = [120, 96, 180];
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    let mut pad = FxSlot::new(FxKind::PolySynth);
                    pad.preset_name = "Soft Analog Pad".into();
                    pad.set_param(0, 0.0);
                    pad.set_param(1, 38.0);
                    pad.set_param(2, 1800.0);
                    pad.set_param(5, 220.0);
                    pad.set_param(6, 780.0);
                    pad.set_param(7, 78.0);
                    pad.set_param(8, 920.0);
                    t.fx_chain.push(pad);
                    t.fx_chain.push(FxSlot::new(FxKind::PlateHallReverb));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddPercTrack => {
                self.snapshot_before("Add Percussion Track");
                let id = self
                    .project
                    .add_track(format!("{:02} Percussion", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.color = [220, 160, 48];
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    let mut kit = FxSlot::new(FxKind::DrumKit);
                    kit.preset_name = "Tight Techno Kit".into();
                    kit.set_param(0, 78.0);
                    kit.set_param(1, 48.0);
                    kit.set_param(2, 72.0);
                    kit.set_param(3, 85.0);
                    kit.set_param(4, 18.0);
                    t.fx_chain.push(kit);
                    let mut glue = FxSlot::new(FxKind::Compressor);
                    glue.preset_name = "Perc Glue".into();
                    glue.set_param(0, -18.0);
                    glue.set_param(1, 3.0);
                    glue.set_param(2, 8.0);
                    glue.set_param(3, 80.0);
                    t.fx_chain.push(glue);
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
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    t.fx_chain.push(FxSlot::new(FxKind::SamplePlayer));
                }
                self.selected_track = Some(id);
                self.bottom_tab = BottomTab::FxAndModulators;
                self.sync_engine();
            }
            PaletteCommand::AddVocalPitchTrack => {
                self.spawn_track_of_kind(NewTrackKind::Vocal);
            }
            PaletteCommand::AddDrumTrack => {
                self.snapshot_before("Add Drum Kit Track");
                let id = self
                    .project
                    .add_track(format!("{:02} Studio Drum Kit", self.project.tracks.len() + 1));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
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
                    t.record_arm = true;
                    t.input_monitoring = true;
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
            PaletteCommand::StartNightCurrentTutorial => {
                self.start_night_current_tutorial();
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
            PaletteCommand::SaveProject => { self.save_project_dialog(); }
            PaletteCommand::LoadDemoProject => {
                self.request_destructive(PendingDestructive::LoadDemo);
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

    fn save_project_dialog(&mut self) -> bool {
        let path = self.project_path.clone().or_else(|| {
            rfd::FileDialog::new()
                .set_title("Save FerricDAW Session (Git-Friendly RON or JSON)")
                .add_filter("FerricDAW Project (.ferric)", &["ferric"])
                .add_filter("JSON Project (.json)", &["json"])
                .set_file_name("session.ferric")
                .save_file()
        });
        path.is_some_and(|p| self.save_project_to_path(&p))
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

    fn save_project_to_path(&mut self, p: &PathBuf) -> bool {
        self.engine
            .harvest_plugin_states_into_project(&mut self.project);
        match save_project_bundle(&mut self.project, p) {
            Ok(()) => {
                self.project_path = Some(p.clone());
                self.record_recent_project(p);
                self.dirty_since_autosave = false;
                self.dirty_since_project_save = false;
                self.status_banner =
                    format!("Saved full session & audio bundle to {}", p.display());
                true
            }
            Err(e) => {
                self.status_banner = format!("Save error: {}", e);
                false
            }
        }
    }

    fn open_project_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .set_title("Open FerricDAW Session")
            .add_filter("FerricDAW / JSON", &["ferric", "json"])
            .pick_file()
        {
            self.request_destructive(PendingDestructive::OpenPath(p));
        }
    }

    fn open_project_from_path(&mut self, p: &PathBuf) {
        // Called from confirm dialog or when session is clean — always loads.
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
                self.dirty_since_project_save = false;
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

        if self.launch_pad_focused
            && self.bottom_tab == BottomTab::LaunchPad
            && !ctrl
            && !shift
        {
            let num_keys = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
            ];
            for (i, key) in num_keys.iter().enumerate() {
                if ctx.input(|inp| inp.key_pressed(*key)) && i < self.project.scenes.len() {
                    self.queue_scene_launch(i);
                    return;
                }
            }
        }

        if ctx.input(|i| i.key_pressed(Key::Space)) {
            self.toggle_play();
        }
        if shift && ctx.input(|i| i.key_pressed(Key::R)) {
            self.execute_palette_command(PaletteCommand::CaptureRetroactive);
            return;
        }
        // Transport letters when musical typing pads are off (avoids pad key collisions).
        if !ctrl && !shift && !self.keyboard_pads.enabled {
            if ctx.input(|i| i.key_pressed(Key::R)) {
                self.toggle_record();
                return;
            }
            if ctx.input(|i| i.key_pressed(Key::L)) {
                self.project.loop_enabled = !self.project.loop_enabled;
                self.sync_engine();
                return;
            }
            if ctx.input(|i| i.key_pressed(Key::C)) {
                self.project.metronome_enabled = !self.project.metronome_enabled;
                self.sync_engine();
                return;
            }
            if ctx.input(|i| i.key_pressed(Key::S)) {
                if let Some((tid, cid)) = self.selected_clip {
                    let beat = self.engine.latest_telemetry.playhead_beat;
                    self.snapshot_before("Split Clip at Playhead");
                    if let Some(new_id) = self.project.split_clip(tid, cid, beat) {
                        self.selected_clip = Some((tid, new_id));
                        self.sync_engine();
                    }
                }
                return;
            }
            if ctx.input(|i| i.key_pressed(Key::Q)) {
                if let Some((tid, cid)) = self.selected_clip {
                    self.snapshot_before("Quantize MIDI 1/16");
                    self.project.quantize_midi_clip(tid, cid, 0.25);
                    self.sync_engine();
                }
                return;
            }
        }
        // Selected MIDI notes: Backspace / Delete (authoritative — piano roll also handles as backup).
        if !ctrl
            && !self.piano_roll.selected_notes.is_empty()
            && ctx.input(|i| i.key_pressed(Key::Backspace) || i.key_pressed(Key::Delete))
        {
            if let Some((tid, cid)) = self.selected_clip {
                self.snapshot_before("Delete MIDI Notes");
                let kill = self.piano_roll.selected_notes.clone();
                if let Some(clip) = self
                    .project
                    .find_track_mut(tid)
                    .and_then(|t| t.find_clip_mut(cid))
                {
                    let active = clip.active_take.min(clip.takes.len().saturating_sub(1));
                    if let Some(TakeContent::Midi { notes, .. }) =
                        clip.takes.get_mut(active).map(|t| &mut t.content)
                    {
                        let before = notes.len();
                        notes.retain(|n| !kill.contains(&n.id));
                        if notes.len() != before {
                            self.status_banner = format!(
                                "Deleted {} MIDI note{}",
                                before - notes.len(),
                                if before - notes.len() == 1 { "" } else { "s" }
                            );
                        }
                    }
                }
                self.piano_roll.selected_notes.clear();
                self.sync_engine();
            }
            return;
        }

        // Delete selected timeline clip(s) — only when no MIDI notes are selected.
        if !ctrl
            && !shift
            && self.piano_roll.selected_notes.is_empty()
            && ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace))
        {
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

        if !ctrl {
            for ev in self.keyboard_pads.poll(ctx, self.selected_track) {
                self.engine.send(EngineCommand::LiveMidi {
                    target_track: ev.target_track,
                    note: ev.note,
                    velocity: ev.velocity,
                    is_note_on: ev.is_note_on,
                });
            }
        }
    }

    /// Create a track from a first-class type (Vocal / MIDI / Instrument / Audio / Universal).
    fn spawn_track_of_kind(&mut self, kind: NewTrackKind) {
        self.snapshot_before(kind.label());
        let idx = self.project.tracks.len() + 1;
        let id = match kind {
            NewTrackKind::Vocal => {
                let id = self.project.add_track(format!("{:02} Vocal", idx));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.color = [220, 92, 118];
                    t.input_source = InputSource::AudioMono { channel: 0 };
                    t.record_arm = true;
                    t.input_monitoring = true;
                    t.fx_chain.push(FxSlot::new(FxKind::NoiseGate));
                    let mut vocal = FxSlot::new(FxKind::VocalPitch);
                    vocal.preset_name = "Natural Vocal".into();
                    t.fx_chain.push(vocal);
                    t.fx_chain.push(FxSlot::new(FxKind::Compressor));
                }
                id
            }
            NewTrackKind::Midi => {
                let id = self.project.add_track(format!("{:02} MIDI", idx));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.color = [92, 168, 220];
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    t.input_monitoring = true;
                }
                id
            }
            NewTrackKind::Instrument => {
                let id = self.project.add_track(format!("{:02} Instrument", idx));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.color = [186, 108, 226];
                    t.input_source = InputSource::MidiAll;
                    t.record_arm = true;
                    t.input_monitoring = true;
                    t.fx_chain.push(FxSlot::new(FxKind::PolySynth));
                    t.fx_chain.push(FxSlot::new(FxKind::StereoDelay));
                }
                id
            }
            NewTrackKind::AudioInterface => {
                let id = self.project.add_track(format!("{:02} Audio In", idx));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.color = [72, 180, 140];
                    t.input_source = InputSource::AudioMono { channel: 0 };
                    t.record_arm = true;
                    t.input_monitoring = true;
                    t.fx_chain.push(FxSlot::new(FxKind::ParametricEq));
                    t.fx_chain.push(FxSlot::new(FxKind::Compressor));
                }
                id
            }
            NewTrackKind::Universal => {
                let id = self
                    .project
                    .add_track(format!("{:02} Universal Track", idx));
                if let Some(t) = self.project.find_track_mut(id) {
                    t.input_source = InputSource::AudioAndMidi { stereo_pair: 0 };
                    t.input_monitoring = true;
                }
                id
            }
        };
        self.selected_track = Some(id);
        self.status_banner = format!("Added {} — {}", kind.label(), kind.hint());
        if matches!(
            kind,
            NewTrackKind::Instrument | NewTrackKind::Vocal | NewTrackKind::AudioInterface
        ) {
            self.bottom_tab = BottomTab::FxAndModulators;
            self.show_bottom_dock = true;
        }
        self.sync_engine();
    }

    /// Spawn Prism on Pad A + Drum Kit on Pad B and enable dual keyboard pads.
    fn spawn_dual_keyboard_studio(&mut self) {
        self.snapshot_before("Spawn Dual Keyboard Studio");
        let prism_id = self.project.add_track("Pad A — Ferric Prism");
        if let Some(t) = self.project.find_track_mut(prism_id) {
            t.color = [186, 108, 226];
            t.input_source = InputSource::MidiAll;
            let mut prism = FxSlot::new(FxKind::PrismSynth);
            prism.preset_name = "Laptop Lead Prism".into();
            prism.set_param(0, 72.0);
            prism.set_param(1, 55.0);
            prism.set_param(2, 28.0);
            prism.set_param(3, 32.0);
            t.fx_chain.push(prism);
            t.fx_chain.push(FxSlot::new(FxKind::StereoDelay));
        }
        let kit_id = self.project.add_track("Pad B — Perc / Kit");
        if let Some(t) = self.project.find_track_mut(kit_id) {
            t.color = [220, 160, 48];
            t.input_source = InputSource::MidiAll;
            let mut kit = FxSlot::new(FxKind::DrumKit);
            kit.preset_name = "Tight Techno Kit".into();
            kit.set_param(0, 78.0);
            kit.set_param(1, 48.0);
            kit.set_param(3, 85.0);
            t.fx_chain.push(kit);
        }
        self.keyboard_pads.enabled = true;
        self.keyboard_pads.pad_a_track = Some(prism_id);
        self.keyboard_pads.pad_b_track = Some(kit_id);
        self.keyboard_pads.pad_b_layout = keyboard_pads::PadBLayout::DrumGrid;
        // Compact laptops: number row. Full keyboards: keep Numpad (raw VK).
        self.keyboard_pads.pad_b_source = PadBSource::Numpad;
        self.selected_track = Some(prism_id);
        self.browser.visible = true;
        self.browser.tab = browser::BrowserTab::HardwareRig;
        self.sync_engine();
        self.status_banner =
            "Dual Keyboard Studio ready — letters → Prism, numpad/number-row → drums. Toggle Pads in the transport bar."
                .into();
    }

    fn render_dna_strip(&mut self, ui: &mut egui::Ui) {
        chrome_card(ui, ACCENT_FERRIC, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                chrome_section_title(ui, "DNA", ACCENT_FERRIC);
                ui.label(
                    egui::RichText::new("Feel = timing · Velocity = accents · Apply writes Feel")
                        .size(10.0)
                        .color(TEXT_DIM),
                );
                chrome_row_label(ui, "Feel ->", ACCENT_TEAL);
                ui.spacing_mut().slider_width = 88.0;
                let mut feel_pct = (self.project.dna_feel * 100.0).round();
                if ui
                    .add(egui::Slider::new(&mut feel_pct, 0.0..=100.0).show_value(true))
                    .changed()
                {
                    self.project.dna_feel = (feel_pct / 100.0).clamp(0.0, 1.0);
                    self.sync_engine();
                }
                egui::ComboBox::from_id_salt("dna_groove_tmpl")
                    .selected_text(match self.project.dna_groove_template {
                        GrooveTemplate::Straight => "Straight",
                        GrooveTemplate::Mpc58Classic => "Techno Pocket",
                        GrooveTemplate::Mpc62BoomBap => "Boom-Bap",
                        GrooveTemplate::PushedUrgent16ths => "Push",
                        other => other.short_badge(),
                    })
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for &tmpl in GrooveTemplate::ALL {
                            let label = match tmpl {
                                GrooveTemplate::Straight => "Straight",
                                GrooveTemplate::Mpc58Classic => "Techno Pocket",
                                GrooveTemplate::Mpc62BoomBap => "Boom-Bap",
                                GrooveTemplate::PushedUrgent16ths => "Push",
                                other => other.label(),
                            };
                            if ui
                                .selectable_value(
                                    &mut self.project.dna_groove_template,
                                    tmpl,
                                    label,
                                )
                                .changed()
                            {
                                self.sync_engine();
                            }
                        }
                    });
                ui.separator();
                chrome_row_label(ui, "Velocity ->", ACCENT_AMBER);
                ui.spacing_mut().slider_width = 88.0;
                let mut vel_pct = (self.project.dna_velocity * 100.0).round();
                if ui
                    .add(egui::Slider::new(&mut vel_pct, 0.0..=100.0).show_value(true))
                    .changed()
                {
                    self.project.dna_velocity = (vel_pct / 100.0).clamp(0.0, 1.0);
                    self.sync_engine();
                }
                egui::ComboBox::from_id_salt("dna_accent_shape")
                    .selected_text(self.project.dna_accent_shape.label())
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for &shape in VelocityAccentShape::ALL {
                            if ui
                                .selectable_value(
                                    &mut self.project.dna_accent_shape,
                                    shape,
                                    shape.label(),
                                )
                                .changed()
                            {
                                self.sync_engine();
                            }
                        }
                    });
                if ui
                    .small_button("Apply drums")
                    .on_hover_text(
                        "Writes Feel groove into drum/kit note timing (undoable) · Velocity stays live",
                    )
                    .clicked()
                {
                    self.snapshot_before("Apply DNA to Drums");
                    self.project.apply_dna_to_tracks(true);
                    self.sync_engine();
                    self.status_banner =
                        "DNA Feel template applied to drums (Velocity DNA stays live).".into();
                }
                if ui
                    .small_button("Apply all MIDI")
                    .on_hover_text(
                        "Writes Feel groove into all MIDI tracks (undoable) · Velocity stays live",
                    )
                    .clicked()
                {
                    self.snapshot_before("Apply DNA to All MIDI");
                    self.project.apply_dna_to_tracks(false);
                    self.sync_engine();
                    self.status_banner =
                        "DNA Feel template applied to MIDI instruments (Velocity DNA stays live)."
                            .into();
                }
            });
        });
    }

    fn render_update_modal(&mut self, ctx: &Context) {
        if !self.show_update_modal {
            return;
        }
        let mut open = true;
        egui::Window::new("FerricDAW Updates")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.5_f32, ACCENT_FERRIC))
                    .corner_radius(8)
                    .inner_margin(14.0),
            )
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Installed: v{}",
                        UpdateService::current_version()
                    ))
                    .size(13.0)
                    .strong()
                    .color(TEXT_MAIN),
                );
                ui.label(
                    egui::RichText::new(UpdateService::feed_url())
                        .size(10.0)
                        .monospace()
                        .color(TEXT_DIM),
                );
                ui.add_space(8.0);

                if !UpdateService::feed_configured() {
                    ui.label(
                        egui::RichText::new(
                            "Update feed not configured yet. After GitHub Releases are set up, \
                             the portable zip will include update_feed.url automatically.",
                        )
                        .color(ACCENT_AMBER),
                    );
                } else if let Some(err) = &self.updater.last_error {
                    ui.label(egui::RichText::new(err).color(ACCENT_RED));
                    if ui.button("Retry check").clicked() {
                        self.updater.check_async();
                    }
                } else if let Some(feed) = self.updater.pending.clone() {
                    ui.label(
                        egui::RichText::new(format!("New version: v{}", feed.version))
                            .size(15.0)
                            .strong()
                            .color(ACCENT_TEAL),
                    );
                    if !feed.notes.is_empty() {
                        ui.label(egui::RichText::new(&feed.notes).color(TEXT_DIM));
                    }
                    ui.add_space(6.0);
                    if self.updater.downloading {
                        ui.label("Downloading…");
                    } else if self.updater.download_path.is_some() {
                        ui.label(
                            egui::RichText::new("Download ready — install will restart FerricDAW.")
                                .color(ACCENT_TEAL),
                        );
                        if ui
                            .button(
                                egui::RichText::new("Install & Restart")
                                    .strong()
                                    .color(ACCENT_FERRIC),
                            )
                            .clicked()
                        {
                            match self.updater.apply_and_restart() {
                                Ok(()) => {
                                    self.status_banner =
                                        "Applying update — FerricDAW will restart…".into();
                                    self.quit_for_update = true;
                                }
                                Err(e) => {
                                    self.updater.last_error = Some(e);
                                }
                            }
                        }
                    } else if ui
                        .button(egui::RichText::new("Download update").strong().color(ACCENT_TEAL))
                        .clicked()
                    {
                        self.updater.download_async();
                    }
                    if ui.button("Open release page").clicked() {
                        let _ = std::process::Command::new("cmd")
                            .args(["/C", "start", "", &feed.url])
                            .spawn();
                    }
                } else if self.updater.checked_this_session {
                    ui.label(
                        egui::RichText::new("You're on the latest version.")
                            .color(ACCENT_TEAL),
                    );
                    if ui.button("Check again").clicked() {
                        self.updater.check_async();
                    }
                } else {
                    ui.label("Checking…");
                    if ui.button("Check now").clicked() {
                        self.updater.check_async();
                    }
                }

                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    self.show_update_modal = false;
                }
            });
        if !open {
            self.show_update_modal = false;
        }
    }

    fn render_top_transport_bar(&mut self, ui: &mut egui::Ui) {
        let telem = self.engine.latest_telemetry;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            if pill_toggle(ui, "☰ Browser", self.browser.visible, ACCENT_TEAL)
                .on_hover_text("Show/Hide Left Studio Browser (Hardware Rig, Samples, Presets & CLAP/VST3)")
                .clicked()
            {
                self.browser.visible = !self.browser.visible;
            }

            ui.menu_button(
                egui::RichText::new("Ferric")
                    .size(13.0)
                    .strong()
                    .color(ACCENT_FERRIC),
                |ui| {
                    ui.set_min_width(240.0);
                    ui.label(egui::RichText::new("Session").size(10.5).color(TEXT_DIM));
                    if ui.button("New Empty Session").clicked() {
                        self.request_destructive(PendingDestructive::NewEmpty);
                        ui.close_menu();
                    }
                    if ui.button("Open…  Ctrl+O").clicked() {
                        self.open_project_dialog();
                        ui.close_menu();
                    }
                    if !self.recent_projects.is_empty() {
                        ui.menu_button("Open Recent", |ui| {
                            for rp in self.recent_projects.clone() {
                                if ui.button(rp.display().to_string()).clicked() {
                                    self.request_destructive(PendingDestructive::OpenPath(rp));
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    if ui.button("Save  Ctrl+S").clicked() {
                        self.save_project_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Save As…").clicked() {
                        self.save_project_as_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Recover Crash Autosave").clicked() {
                        self.recover_autosave();
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Add").size(10.5).color(TEXT_DIM));
                    ui.menu_button("Add Track…", |ui| {
                        for &kind in NewTrackKind::ALL {
                            if ui
                                .button(kind.label())
                                .on_hover_text(kind.hint())
                                .clicked()
                            {
                                self.spawn_track_of_kind(kind);
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.button("Import Audio onto Selected Track…").clicked() {
                        if self.selected_track.is_none() {
                            self.spawn_track_of_kind(NewTrackKind::AudioInterface);
                        }
                        if let Some(tid) = self.selected_track {
                            self.import_audio_to_track(tid);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Import MIDI (.mid)…").clicked() {
                        self.import_smf_midi_dialog(None);
                        ui.close_menu();
                    }
                    if ui.button("Hardware Studio Rig (MIDI + Line-In)").clicked() {
                        self.spawn_hardware_studio_rig();
                        ui.close_menu();
                    }
                    if ui.button("Afterglow Copper Playground").clicked() {
                        self.request_destructive(PendingDestructive::LoadDemo);
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Updates").size(10.5).color(TEXT_DIM));
                    if ui
                        .button(format!(
                            "Check for Updates… (v{})",
                            UpdateService::current_version()
                        ))
                        .clicked()
                    {
                        self.updater.check_async();
                        self.show_update_modal = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Export / Edit").size(10.5).color(TEXT_DIM));
                    if ui.button("Bounce Master / Stems…  Ctrl+B").clicked() {
                        self.execute_palette_command(PaletteCommand::OpenBounceModal);
                        ui.close_menu();
                    }
                    if ui.button("Glue Selected Clips  Ctrl+G").clicked() {
                        self.glue_selected_clips();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Night Current Tutorial").clicked() {
                        self.start_night_current_tutorial();
                        ui.close_menu();
                    }
                    if ui.button("Help & Shortcuts  F1").clicked() {
                        self.show_help_modal = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Undo  Ctrl+Z").clicked() {
                        self.perform_undo();
                        ui.close_menu();
                    }
                    if ui.button("Redo  Ctrl+Y").clicked() {
                        self.perform_redo();
                        ui.close_menu();
                    }
                },
            );

            ui.separator();

            if ui
                .small_button("⏮")
                .on_hover_text("Return playhead to start")
                .clicked()
            {
                self.engine.send(EngineCommand::SeekBeat(0.0));
            }
            if ui
                .small_button("◀Bar")
                .on_hover_text("Nudge playhead back 1 bar")
                .clicked()
            {
                let bpb = self.project.time_signature.beats_per_bar();
                let beat = (telem.playhead_beat - bpb).max(0.0);
                self.engine.send(EngineCommand::SeekBeat(beat));
            }
            if ui
                .small_button("Bar▶")
                .on_hover_text("Nudge playhead forward 1 bar")
                .clicked()
            {
                let bpb = self.project.time_signature.beats_per_bar();
                self.engine
                    .send(EngineCommand::SeekBeat(telem.playhead_beat + bpb));
            }
            let play_lbl = if telem.is_playing { "Pause" } else { "Play" };
            if pill_toggle(ui, play_lbl, telem.is_playing, ACCENT_TEAL)
                .on_hover_text("Space — play / pause")
                .clicked()
            {
                self.toggle_play();
            }
            if ui
                .small_button("Stop")
                .on_hover_text("Stop playback (keeps playhead)")
                .clicked()
            {
                if telem.is_recording {
                    self.engine.send(EngineCommand::SetRecord(false));
                }
                self.engine.send(EngineCommand::Stop);
            }
            if pill_toggle(ui, "Rec", telem.is_recording, ACCENT_RED)
                .on_hover_text("Record — arms selected track if nothing is armed")
                .clicked()
            {
                self.toggle_record();
            }
            if pill_toggle(ui, "Loop", self.project.loop_enabled, ACCENT_FERRIC)
                .on_hover_text("Toggle loop · right-click ruler to set loop region")
                .clicked()
            {
                self.project.loop_enabled = !self.project.loop_enabled;
                self.sync_engine();
            }
            if pill_toggle(ui, "Click", self.project.metronome_enabled, ACCENT_AMBER)
                .on_hover_text("Metronome click")
                .clicked()
            {
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
                .on_hover_text("Retroactive Capture (Shift+R): Drops last 15s of USB MIDI or Line-In audio")
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
                            let pos_resp = ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(bbt)
                                            .size(14.0)
                                            .monospace()
                                            .strong()
                                            .color(ACCENT_TEAL),
                                    )
                                    .sense(egui::Sense::click_and_drag()),
                                )
                                .on_hover_text(
                                    "Drag to scrub playhead · or click the arrangement ruler / empty lane",
                                );
                            if pos_resp.dragged() {
                                let dx = pos_resp.drag_delta().x;
                                let ppb = self.arrangement.pixels_per_beat.max(14.0);
                                let beat =
                                    (telem.playhead_beat + (dx / ppb) as f64).max(0.0);
                                self.engine.send(EngineCommand::SeekBeat(beat));
                            }
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
                    egui::RichText::new(format!("{:.1} BPM", live_bpm))
                        .size(11.5)
                        .monospace()
                        .strong()
                        .color(ACCENT_AMBER),
                )
                .on_hover_text("Conductor tempo map active — edit in Tempo Map lane");
            } else if ui
                .add(
                    egui::DragValue::new(&mut self.project.bpm)
                        .range(30.0..=300.0)
                        .speed(0.25)
                        .suffix(" BPM"),
                )
                .on_hover_text("Project tempo")
                .changed()
            {
                self.sync_engine();
            }
            // Global meter — always visible next to BPM
            let mut meter_changed = false;
            ui.label(egui::RichText::new("Meter").size(10.5).color(TEXT_DIM));
            meter_changed |= ui
                .add(
                    egui::DragValue::new(&mut self.project.time_signature.numerator)
                        .range(1..=16)
                        .speed(0.2),
                )
                .changed();
            ui.label(egui::RichText::new("/").size(11.0).color(TEXT_DIM));
            meter_changed |= ui
                .add(
                    egui::DragValue::new(&mut self.project.time_signature.denominator)
                        .range(1..=16)
                        .speed(0.2),
                )
                .changed();
            if meter_changed {
                self.sync_engine();
            }
            if ui
                .small_button("TAP")
                .on_hover_text("Tap 4× to set project tempo")
                .clicked()
            {
                if let Some(new_bpm) = self.tap_tempo.register_tap() {
                    self.project.bpm = (new_bpm * 2.0).round() * 0.5;
                    self.sync_engine();
                    self.status_banner = format!("Tap Tempo locked: {:.1} BPM", self.project.bpm);
                }
            }

            let mt_lbl = if self.keyboard_pads.enabled {
                format!(
                    "Pads A{} B{}",
                    self.keyboard_pads.pad_a_octave, self.keyboard_pads.pad_b_octave
                )
            } else {
                "Pads".into()
            };
            if pill_toggle(
                ui,
                &mt_lbl,
                self.keyboard_pads.enabled,
                Color32::from_rgb(186, 108, 226),
            )
            .on_hover_text(
                "Dual keyboard pads — letters = Pad A, numpad/number row = Pad B (Hardware Rig)",
            )
            .clicked()
            {
                if self.keyboard_pads.enabled {
                    for ev in self.keyboard_pads.panic_release_all() {
                        self.engine.send(EngineCommand::LiveMidi {
                            target_track: ev.target_track,
                            note: ev.note,
                            velocity: ev.velocity,
                            is_note_on: ev.is_note_on,
                        });
                    }
                }
                self.keyboard_pads.enabled = !self.keyboard_pads.enabled;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.updater.pending.is_some() {
                    if ui
                        .button(
                            egui::RichText::new("Update")
                                .size(11.0)
                                .strong()
                                .color(ACCENT_AMBER),
                        )
                        .on_hover_text("A newer FerricDAW build is available")
                        .clicked()
                    {
                        self.show_update_modal = true;
                    }
                }
                if pill_toggle(ui, "Help", self.show_help_modal, ACCENT_TEAL)
                    .on_hover_text("Quickstart, Feature Guide & Shortcuts (F1)")
                    .clicked()
                {
                    self.show_help_modal = !self.show_help_modal;
                }
                if pill_toggle(ui, "Tutorial", self.tutorial.open, ACCENT_FERRIC)
                    .on_hover_text("Night Current tutorial — build the song step by step")
                    .clicked()
                {
                    if self.tutorial.open {
                        self.tutorial.open = false;
                    } else {
                        self.start_night_current_tutorial();
                    }
                }
                if pill_toggle(ui, "Dock", self.show_bottom_dock, ACCENT_TEAL)
                    .on_hover_text("Show / hide lower dock (MIDI, FX, Mix…)")
                    .clicked()
                {
                    self.show_bottom_dock = !self.show_bottom_dock;
                }
                if ui
                    .button(egui::RichText::new("Ctrl+K").size(11.0).color(ACCENT_FERRIC))
                    .on_hover_text("Command palette — add tracks, FX, tools")
                    .clicked()
                {
                    self.palette.toggle();
                }

                ui.label(
                    egui::RichText::new(format!(
                        "DSP {:.0}% · {:.1} LUFS",
                        telem.cpu_load_percent, telem.master.momentary_lufs
                    ))
                    .size(10.5)
                    .monospace()
                    .color(TEXT_DIM),
                );

                if !self.engine.midi_port_names.is_empty() {
                    ui.label(
                        egui::RichText::new("MIDI ●")
                            .size(10.5)
                            .strong()
                            .color(ACCENT_TEAL),
                    )
                    .on_hover_text(self.engine.midi_port_names.join(", "));
                }
            });
        });
    }

    fn load_golden_reference_stats(&mut self, path: PathBuf) {
        match import_audio_file(&path, self.engine.sample_rate) {
            Ok(buf) => {
                let lufs = ferric_dsp::measure_integrated_lufs(
                    buf.sample_rate,
                    &buf.left,
                    &buf.right,
                );
                let tp = ferric_dsp::measure_true_peak_dbtp(&buf.left, &buf.right);
                let peaks: Vec<f32> = buf
                    .peaks
                    .iter()
                    .step_by((buf.peaks.len() / 128).max(1))
                    .map(|(mn, mx)| mn.abs().max(mx.abs()))
                    .take(128)
                    .collect();
                self.bounce_modal.reference_path = Some(path);
                self.bounce_modal.reference_lufs = Some(lufs);
                self.bounce_modal.reference_true_peak = Some(tp);
                self.bounce_modal.reference_duration = Some(buf.duration_secs());
                self.bounce_modal.reference_peaks = peaks;
                self.bounce_modal.show_golden_panel = true;
            }
            Err(e) => {
                self.status_banner = format!("Golden reference load failed: {e}");
            }
        }
    }

    fn ensure_default_golden_reference(&mut self) {
        if self.bounce_modal.reference_path.is_some() {
            return;
        }
        if let Some(p) = self.find_night_current_demo_path() {
            self.load_golden_reference_stats(p);
        }
    }

    fn find_night_current_demo_path(&self) -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("demo").join("NightCurrent-Demo.wav"));
                candidates.push(dir.join("assets").join("demo").join("NightCurrent-Demo.wav"));
            }
        }
        candidates.push(PathBuf::from("demo/NightCurrent-Demo.wav"));
        candidates.push(PathBuf::from("assets/demo/NightCurrent-Demo.wav"));
        // Also search upward from CWD for repo layouts during `cargo run`.
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("demo").join("NightCurrent-Demo.wav"));
            candidates.push(cwd.join("assets").join("demo").join("NightCurrent-Demo.wav"));
        }
        candidates.into_iter().find(|p| p.is_file())
    }

    fn render_bounce_modal(&mut self, ctx: &Context) {
        if !self.bounce_modal.open {
            return;
        }
        let mut keep_open = true;
        egui::Window::new("💿 Mastering Bounce & Golden Bounce Studio")
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(true)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_size([560.0, 520.0])
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
                            let report = render_all_stems_with_options(&self.project, &dir, &opts)
                                .map_err(|e| e.to_string());
                            if report.is_ok() {
                                self.project.tutorial_bounce_done = true;
                                self.project.tutorial_step_reached = self
                                    .project
                                    .tutorial_step_reached
                                    .max(NightCurrentStep::Bounce.index() as u8);
                            }
                            self.bounce_modal.last_report = Some(report);
                        }
                    } else {
                        let ext = opts.format.extension();
                        if let Some(file) = rfd::FileDialog::new()
                            .set_title("Export Master Audio")
                            .add_filter(ext.to_uppercase(), &[ext])
                            .set_file_name(format!("master_mix.{}", ext))
                            .save_file()
                        {
                            let report = render_master_with_options(&self.project, &file, &opts)
                                .map_err(|e| e.to_string());
                            if report.is_ok() {
                                self.bounce_modal.last_master_path = Some(file.clone());
                                self.project.tutorial_bounce_done = true;
                                self.project.tutorial_step_reached = self
                                    .project
                                    .tutorial_step_reached
                                    .max(NightCurrentStep::Bounce.index() as u8);
                                if let Ok(buf) =
                                    import_audio_file(&file, self.engine.sample_rate)
                                {
                                    self.bounce_modal.bounce_peaks = buf
                                        .peaks
                                        .iter()
                                        .step_by((buf.peaks.len() / 128).max(1))
                                        .map(|(mn, mx)| mn.abs().max(mx.abs()))
                                        .take(128)
                                        .collect();
                                }
                                self.ensure_default_golden_reference();
                            }
                            self.bounce_modal.last_report = Some(report);
                        }
                    }
                }

                let report_ok = self
                    .bounce_modal
                    .last_report
                    .as_ref()
                    .and_then(|r| r.as_ref().ok().cloned());
                let report_err = self
                    .bounce_modal
                    .last_report
                    .as_ref()
                    .and_then(|r| r.as_ref().err().cloned());
                if let Some(rep) = report_ok {
                    ui.separator();
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
                    self.bounce_modal.show_golden_panel = true;
                    ui.add_space(4.0);
                    let mut open_golden = false;
                    let mut play_ref = false;
                    let mut play_bounce = false;
                    ui.horizontal(|ui| {
                        if ui
                            .button(
                                egui::RichText::new("Open Golden Bounce")
                                    .strong()
                                    .color(ACCENT_TEAL),
                            )
                            .clicked()
                        {
                            open_golden = true;
                        }
                        if ui
                            .button("A/B vs reference")
                            .on_hover_text("Audition golden / demo reference")
                            .clicked()
                        {
                            play_ref = true;
                        }
                        if self.bounce_modal.last_master_path.is_some()
                            && ui
                                .button("Play my bounce")
                                .on_hover_text("Audition the file you just exported")
                                .clicked()
                        {
                            play_bounce = true;
                        }
                    });
                    if open_golden || play_ref {
                        self.ensure_default_golden_reference();
                        self.bounce_modal.show_golden_panel = true;
                    }
                    if play_ref {
                        self.play_night_current_demo_reference();
                    }
                    if play_bounce {
                        if let Some(path) = self.bounce_modal.last_master_path.clone() {
                            match import_audio_file(&path, self.engine.sample_rate) {
                                Ok(mut buf) => {
                                    if self.bounce_modal.loudness_match_ab {
                                        if let Some(ref_lufs) = self.bounce_modal.reference_lufs {
                                            let delta = ref_lufs
                                                - ferric_dsp::measure_integrated_lufs(
                                                    buf.sample_rate,
                                                    &buf.left,
                                                    &buf.right,
                                                );
                                            let g = ferric_dsp::db_to_lin(delta);
                                            let left: Vec<f32> =
                                                buf.left.iter().map(|s| s * g).collect();
                                            let right: Vec<f32> =
                                                buf.right.iter().map(|s| s * g).collect();
                                            buf = ferric_model::AudioBuffer::from_stereo(
                                                buf.sample_rate,
                                                left,
                                                right,
                                                buf.source_path.clone(),
                                            );
                                        }
                                    }
                                    let peaks: Vec<f32> = buf
                                        .peaks
                                        .iter()
                                        .step_by((buf.peaks.len() / 128).max(1))
                                        .map(|(mn, mx)| mn.abs().max(mx.abs()))
                                        .take(128)
                                        .collect();
                                    self.bounce_modal.bounce_peaks = peaks;
                                    self.engine.send(EngineCommand::PreviewBuffer(buf));
                                    self.status_banner =
                                        format!("Playing your bounce — {}", path.display());
                                }
                                Err(e) => {
                                    self.status_banner = format!("Could not play bounce: {e}");
                                }
                            }
                        }
                    }

                    if self.bounce_modal.show_golden_panel {
                        ui.separator();
                        ui.label(
                            egui::RichText::new("Golden Bounce Studio")
                                .strong()
                                .color(ACCENT_FERRIC),
                        );
                        ui.horizontal(|ui| {
                            let ref_name = self
                                .bounce_modal
                                .reference_path
                                .as_ref()
                                .and_then(|p| {
                                    p.file_name().and_then(|n| n.to_str()).map(str::to_string)
                                })
                                .unwrap_or_else(|| "NightCurrent-Demo.wav".into());
                            ui.label(
                                egui::RichText::new(format!("Ref: {ref_name}"))
                                    .size(10.5)
                                    .color(TEXT_DIM),
                            );
                            if ui.small_button("Pick reference…").clicked() {
                                if let Some(p) = rfd::FileDialog::new()
                                    .add_filter("Audio", &["wav", "flac", "mp3", "ogg"])
                                    .pick_file()
                                {
                                    self.load_golden_reference_stats(p);
                                }
                            }
                            if ui.small_button("Use demo").clicked() {
                                self.bounce_modal.reference_path = None;
                                self.ensure_default_golden_reference();
                            }
                            ui.checkbox(
                                &mut self.bounce_modal.loudness_match_ab,
                                "Loudness-match A/B",
                            );
                        });
                        let ref_lufs = self.bounce_modal.reference_lufs;
                        let ref_tp = self.bounce_modal.reference_true_peak;
                        let ref_dur = self.bounce_modal.reference_duration;
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("Your bounce").size(10.5).strong(),
                                );
                                ui.label(format!("{:.1} LUFS", rep.integrated_lufs));
                                ui.label(format!("{:+.2} dBTP", rep.true_peak_dbtp));
                                ui.label(format!("{:.1}s", rep.duration_secs));
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new("Reference").size(10.5).strong());
                                ui.label(format!("{:.1} LUFS", ref_lufs.unwrap_or(f32::NAN)));
                                ui.label(format!("{:+.2} dBTP", ref_tp.unwrap_or(f32::NAN)));
                                ui.label(format!("{:.1}s", ref_dur.unwrap_or(f64::NAN)));
                            });
                            if let Some(rl) = ref_lufs {
                                let delta = rep.integrated_lufs - rl;
                                ui.separator();
                                ui.label(
                                    egui::RichText::new(format!("Δ LUFS {:+.1}", delta))
                                        .strong()
                                        .color(if delta.abs() < 1.0 {
                                            ACCENT_TEAL
                                        } else {
                                            ACCENT_AMBER
                                        }),
                                );
                            }
                        });
                        let wave_h = 36.0_f32;
                        let bounce_peaks = self.bounce_modal.bounce_peaks.clone();
                        let ref_peaks = self.bounce_modal.reference_peaks.clone();
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), wave_h * 2.0 + 8.0),
                            egui::Sense::hover(),
                        );
                        let mid = rect.center().y;
                        let draw_peaks = |peaks: &[f32], y0: f32, col: Color32| {
                            if peaks.is_empty() {
                                return;
                            }
                            let w = rect.width() / peaks.len() as f32;
                            for (i, &p) in peaks.iter().enumerate() {
                                let x = rect.left() + i as f32 * w;
                                let h = p.clamp(0.0, 1.0) * (wave_h * 0.45);
                                ui.painter().line_segment(
                                    [egui::pos2(x, y0 - h), egui::pos2(x, y0 + h)],
                                    Stroke::new(1.0_f32, col),
                                );
                            }
                        };
                        draw_peaks(&bounce_peaks, mid - wave_h * 0.5, ACCENT_TEAL);
                        draw_peaks(&ref_peaks, mid + wave_h * 0.5, ACCENT_FERRIC);
                    }
                } else if let Some(err) = report_err {
                    ui.separator();
                    ui.label(egui::RichText::new(err).color(ACCENT_RED));
                }
            });
        self.bounce_modal.open = keep_open;
    }

    fn queue_scene_launch(&mut self, scene_idx: usize) {
        let beats_per_bar = self.project.time_signature.numerator.max(1) as f64;
        let now = self.engine.latest_telemetry.playhead_beat;
        let at = if self.engine.latest_telemetry.is_playing {
            self.launch_quantize.quantize_beat(now, beats_per_bar)
        } else {
            now
        };
        self.pending_scene_launch = Some((scene_idx, at));
        if self.mix_morph_enabled {
            // Mix slot follows scene index when possible (A/B/C/D).
            let slot = scene_idx.min(3);
            if let Some(Some(to)) = self.project.mix_snapshots.get(slot).cloned() {
                self.mix_morph = Some(MixMorphState {
                    from: self.project.capture_mix_state("morph-from"),
                    to,
                    start_beat: at,
                    duration_beats: self.mix_morph_beats.max(0.25),
                    last_applied_t: 0.0,
                });
            }
        }
        self.status_banner = format!(
            "Scene {} armed @ {:.2} ({})",
            scene_idx + 1,
            at,
            self.launch_quantize.label()
        );
    }

    fn tick_launch_and_morph(&mut self) {
        let beat = self.engine.latest_telemetry.playhead_beat;
        if let Some((scene_idx, at)) = self.pending_scene_launch {
            if !self.engine.latest_telemetry.is_playing || beat + 1e-4 >= at {
                self.project.launch_scene(scene_idx);
                self.pending_scene_launch = None;
                self.sync_engine();
            }
        }
        if let Some(mut morph) = self.mix_morph.take() {
            let t = ((beat - morph.start_beat) / morph.duration_beats.max(0.25)).clamp(0.0, 1.0)
                as f32;
            if (t - morph.last_applied_t).abs() > 0.02 || t >= 1.0 {
                self.project.apply_lerped_mix(&morph.from, &morph.to, t);
                morph.last_applied_t = t;
                self.sync_engine();
            }
            if t < 1.0 {
                self.mix_morph = Some(morph);
            }
        }
    }

    fn render_launch_pad(&mut self, ui: &mut egui::Ui) {
        self.launch_pad_focused = true;
        chrome_card(ui, ACCENT_FERRIC, |ui| {
            ui.horizontal(|ui| {
                chrome_section_title(ui, "Scene Launch Pad", ACCENT_FERRIC);
                chrome_row_label(ui, "Quantize ->", ACCENT_TEAL);
                for q in [LaunchQuantize::Bar, LaunchQuantize::Beat, LaunchQuantize::Quarter] {
                    if pill_toggle(ui, q.label(), self.launch_quantize == q, ACCENT_TEAL).clicked()
                    {
                        self.launch_quantize = q;
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.mix_morph_enabled, "Mix morph");
                ui.add(
                    egui::DragValue::new(&mut self.mix_morph_beats)
                        .range(0.25..=32.0)
                        .prefix("over ")
                        .suffix(" beats"),
                );
                ui.label(
                    egui::RichText::new("Keys 1–8 fire scenes while this tab is focused")
                        .size(10.0)
                        .color(TEXT_DIM),
                );
            });
        });
        ui.add_space(6.0);
        let scene_labels: Vec<(usize, String)> = self
            .project
            .scenes
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.name.clone()))
            .collect();
        let mut launch_idx = None;
        ui.horizontal_wrapped(|ui| {
            for (i, name) in &scene_labels {
                let armed = self.pending_scene_launch.map(|(s, _)| s) == Some(*i);
                let label = format!("{}\n{}", i + 1, name);
                if ui
                    .add_sized(
                        [96.0, 64.0],
                        egui::Button::new(
                            egui::RichText::new(label)
                                .size(12.0)
                                .strong()
                                .color(if armed {
                                    Color32::BLACK
                                } else {
                                    TEXT_MAIN
                                }),
                        )
                        .fill(if armed {
                            ACCENT_TEAL
                        } else {
                            BG_ELEVATED
                        }),
                    )
                    .clicked()
                {
                    launch_idx = Some(*i);
                }
            }
        });
        if let Some(i) = launch_idx {
            self.queue_scene_launch(i);
        }
        ui.add_space(8.0);
        chrome_card(ui, BORDER_SUBTLE, |ui| {
        ui.horizontal(|ui| {
            chrome_row_label(ui, "Mix morph target ->", ACCENT_AMBER);
            for (slot, name) in [(0, "Mix A"), (1, "Mix B"), (2, "Mix C"), (3, "Mix D")] {
                let filled = self
                    .project
                    .mix_snapshots
                    .get(slot)
                    .and_then(|s| s.as_ref())
                    .is_some();
                if ui
                    .add_enabled(
                        filled,
                        egui::Button::new(if filled {
                            name.to_string()
                        } else {
                            format!("{name} (empty)")
                        }),
                    )
                    .clicked()
                {
                    if let Some(Some(to)) = self.project.mix_snapshots.get(slot).cloned() {
                        let now = self.engine.latest_telemetry.playhead_beat;
                        self.mix_morph = Some(MixMorphState {
                            from: self.project.capture_mix_state("morph-from"),
                            to,
                            start_beat: now,
                            duration_beats: self.mix_morph_beats.max(0.25),
                            last_applied_t: 0.0,
                        });
                        self.status_banner = format!("Morphing toward {name}…");
                    }
                }
            }
        });
        });
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
                    self.snapshot_before("Load Factory Preset");
                    let tid = self.selected_track.unwrap_or_else(|| {
                        let n = self.project.tracks.len() + 1;
                        let name = if preset.kind.is_instrument() {
                            format!("{:02} Instrument", n)
                        } else {
                            format!("{:02} Universal Track", n)
                        };
                        let id = self.project.add_track(name);
                        if let Some(t) = self.project.find_track_mut(id) {
                            t.input_source = if preset.kind.is_instrument() {
                                InputSource::MidiAll
                            } else {
                                InputSource::AudioAndMidi { stereo_pair: 0 }
                            };
                        }
                        id
                    });
                    if let Some(trk) = self.project.find_track_mut(tid) {
                        let mut slot = FxSlot::new(preset.kind);
                        slot.preset_name = preset.name.to_string();
                        slot.wet_mix = preset.wet_mix;
                        slot.params = preset.params.to_vec();
                        trk.fx_chain.push(slot);
                    }
                    self.selected_track = Some(tid);
                    self.bottom_tab = BottomTab::FxAndModulators;
                    self.show_bottom_dock = true;
                    self.status_banner = format!("Loaded preset '{}'", preset.name);
                    need_sync = true;
                }
                BrowserAction::LoadExternalPlugin(plug) => {
                    self.snapshot_before("Insert External Plugin");
                    let tid = self.selected_track.unwrap_or_else(|| {
                        let id = self.project.add_track(format!(
                            "{:02} Universal Track",
                            self.project.tracks.len() + 1
                        ));
                        id
                    });
                    let fmt_badge = plug.format.badge();
                    if let Some(trk) = self.project.find_track_mut(tid) {
                        trk.fx_chain.push(FxSlot::new_external_plugin(
                            plug.format,
                            plug.bundle_path,
                            plug.id,
                            plug.name.clone(),
                        ));
                    }
                    self.selected_track = Some(tid);
                    self.status_banner =
                        format!("✓ Inserted {} plugin '{}'", fmt_badge, plug.name);
                    self.bottom_tab = BottomTab::FxAndModulators;
                    self.show_bottom_dock = true;
                    need_sync = true;
                }
                BrowserAction::AuditionPlugin(plug) => {
                    // Sandbox audition: load on a temporary playground track; quarantine on crash.
                    let path = plug.bundle_path.clone();
                    if self.engine.quarantine.is_quarantined(&path) {
                        self.status_banner = format!(
                            "🛡 '{}' is quarantined — Reset quarantine to retry.",
                            plug.name
                        );
                        self.playground_plugin = None;
                        self.playground_ok = false;
                    } else {
                        self.snapshot_before("Plugin Playground Audition");
                        let tid = self.project.add_track(format!("Playground · {}", plug.name));
                        if let Some(trk) = self.project.find_track_mut(tid) {
                            trk.fx_chain.push(FxSlot::new_external_plugin(
                                plug.format,
                                plug.bundle_path.clone(),
                                plug.id.clone(),
                                plug.name.clone(),
                            ));
                        }
                        self.selected_track = Some(tid);
                        self.playground_plugin = Some(plug.clone());
                        self.playground_ok = true;
                        self.bottom_tab = BottomTab::FxAndModulators;
                        self.status_banner = format!(
                            "Playground auditioning '{}' in sandbox — if it survives, Insert.",
                            plug.name
                        );
                        need_sync = true;
                    }
                }
                BrowserAction::InsertAuditionedPlugin => {
                    if let Some(plug) = self.playground_plugin.clone() {
                        if !self.playground_ok {
                            self.status_banner =
                                "Playground audition did not survive — not inserting.".into();
                        } else if let Some(tid) = self.selected_track {
                            // Prefer inserting onto a non-playground track if user reselected.
                            self.snapshot_before("Insert Surviving Playground Plugin");
                            if let Some(trk) = self.project.find_track_mut(tid) {
                                if !trk.name.starts_with("Playground ·") {
                                    trk.fx_chain.push(FxSlot::new_external_plugin(
                                        plug.format,
                                        plug.bundle_path,
                                        plug.id,
                                        plug.name.clone(),
                                    ));
                                    self.status_banner = format!(
                                        "✓ Inserted surviving plugin '{}' into {}",
                                        plug.name, trk.name
                                    );
                                    need_sync = true;
                                } else {
                                    self.status_banner = format!(
                                        "✓ '{}' already on Playground track — select another track to copy-insert.",
                                        plug.name
                                    );
                                }
                            }
                        }
                    }
                }
                BrowserAction::ClearPlayground => {
                    self.snapshot_before("Clear Plugin Playground");
                    self.project.tracks.retain(|t| !t.name.starts_with("Playground ·"));
                    if self
                        .selected_track
                        .and_then(|id| self.project.find_track(id))
                        .is_none()
                    {
                        self.selected_track = self.project.tracks.first().map(|t| t.id);
                    }
                    self.playground_plugin = None;
                    self.playground_ok = false;
                    self.status_banner = "Plugin Playground cleared (audition tracks removed).".into();
                    need_sync = true;
                }
                BrowserAction::ClearPluginQuarantine => {
                    self.engine.quarantine.clear_all();
                    self.status_banner = "Cleared crashed plugin quarantine list.".into();
                }
                BrowserAction::ImportMidiFile(p) => {
                    self.import_smf_midi_dialog(Some(p));
                }
                BrowserAction::SetupHardwareRig => {
                    self.spawn_hardware_studio_rig();
                }
                BrowserAction::SetupHardwareRigPreset(idx) => {
                    let preset = match idx {
                        1 => HardwareRigPreset::AkaiLpk25AndDs10,
                        2 => HardwareRigPreset::MidiKeyboardOnly,
                        3 => HardwareRigPreset::LineInOnly,
                        _ => HardwareRigPreset::UsbMidiAndLineIn,
                    };
                    self.spawn_hardware_rig_preset(preset);
                }
                BrowserAction::SpawnDualKeyboardStudio => {
                    self.spawn_dual_keyboard_studio();
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
                BrowserAction::FixLineInComboJackRouting => {
                    self.apply_line_in_safe_audio_routing();
                    self.status_banner = format!(
                        "🔊 Line-In routing • Out: {} • In: {}",
                        self.engine.audio_device_name, self.engine.audio_input_name
                    );
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
                        self.browser.selected_audio_driver,
                    );
                    let mut msg = format!(
                        "Audio Configured • Out: {} • In: {} • Buf: {:?} • {}",
                        self.engine.audio_device_name,
                        self.engine.audio_input_name,
                        buffer_size,
                        self.browser.selected_audio_driver.label()
                    );
                    if let Some(note) = self.engine.driver_status_note {
                        msg.push_str(" · ");
                        msg.push_str(note);
                        self.browser.driver_honesty_note = Some(note.to_string());
                    } else {
                        self.browser.driver_honesty_note = None;
                    }
                    self.browser.live_exclusive = self.engine.has_exclusive_output();
                    if realtek_combo_jack_risk(
                        &self.engine.audio_device_name,
                        &self.engine.audio_input_name,
                    ) || self
                        .engine
                        .audio_device_name
                        .to_lowercase()
                        .contains("headphone")
                    {
                        msg.push_str(
                            " ⚠ Headphones output + line source on combo jack usually = silence — use Force Speakers + Line-In",
                        );
                    }
                    self.status_banner = msg;
                }
                BrowserAction::SetInputGainDb(db) => {
                    self.engine.set_input_gain_db(db);
                }
                BrowserAction::SetRecordingOffsetSamples(smp) => {
                    self.browser.recording_offset_samples = smp;
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
                        format!("Locked project tempo to Line-In: {:.1} BPM", bpm);
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
        self.tick_launch_and_morph();
        // Playground survival: if auditioned plugin got quarantined after load, block Insert.
        if let Some(ref plug) = self.playground_plugin {
            if self.engine.quarantine.is_quarantined(&plug.bundle_path) {
                self.playground_ok = false;
            }
        }
        if !self.viewport_bootstrapped {
            self.viewport_bootstrapped = true;
            let inner = ctx.input(|i| i.viewport().inner_rect);
            let too_small = inner
                .map(|r| r.width() < 960.0 || r.height() < 600.0)
                .unwrap_or(true);
            if too_small {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1520.0, 900.0)));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }

        // Dirty close guard — CancelClose + confirm modal (I-01).
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quit_for_update
            && self.session_has_user_work()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.pending_destructive.is_none() {
                self.pending_destructive = Some(PendingDestructive::QuitApp);
            }
        }

        if self.quit_for_update {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

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

        let had_pending = self.updater.pending.is_some();
        self.updater.poll();
        if self.updater.pending.is_some() && !had_pending {
            self.show_update_modal = true;
            if let Some(ref feed) = self.updater.pending {
                self.status_banner = format!("Update available: v{}", feed.version);
            }
        }

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
        // Snapshot before modals so Esc that closes bounce/help this frame
        // cannot also dismiss the tutorial coach.
        let overlay_at_frame_start = self.palette.open
            || self.bounce_modal.open
            || self.show_help_modal
            || self.pending_destructive.is_some()
            || self.show_first_run
            || self.show_update_modal;
        self.render_first_run_modal(ctx);
        self.render_bounce_modal(ctx);
        self.render_destructive_confirm_modal(ctx);
        self.render_update_modal(ctx);
        if render_quickstart_help_modal(ctx, &mut self.show_help_modal) {
            self.start_night_current_tutorial();
        }
        {
            let overlay = overlay_at_frame_start
                || self.palette.open
                || self.bounce_modal.open
                || self.show_help_modal
                || self.pending_destructive.is_some()
                || self.show_first_run
                || self.show_update_modal;
            let action = self.tutorial.show(ctx, &self.project, overlay);
            self.handle_tutorial_action(action);
        }

        // 1. Top Studio Transport Bar
        egui::TopBottomPanel::top("ferric_transport_top")
            .exact_height(46.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(8, 5)),
            )
            .show(ctx, |ui| {
                self.render_top_transport_bar(ui);
            });

        egui::TopBottomPanel::top("ferric_dna_strip")
            .exact_height(34.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_PANEL)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ctx, |ui| {
                self.render_dna_strip(ui);
            });

        if !self.engine.has_live_output {
            egui::TopBottomPanel::top("ferric_offline_audio_banner")
                .exact_height(28.0)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(120, 36, 28))
                        .inner_margin(egui::Margin::symmetric(10, 4)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(
                                "OFFLINE AUDIO — no live output. Check devices / driver (Exclusive may have fallen back).",
                            )
                            .size(12.0)
                            .strong()
                            .color(Color32::WHITE),
                        );
                        if ui
                            .button(
                                egui::RichText::new("Open Hardware Rig")
                                    .strong()
                                    .color(ACCENT_AMBER),
                            )
                            .clicked()
                        {
                            self.browser.visible = true;
                            self.browser.tab = browser::BrowserTab::HardwareRig;
                        }
                    });
                });
        }

        // 2. Dockable Left Studio Browser (Hardware Rig, Samples, Presets & CLAP)
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
                    let pad_tracks: Vec<_> = self
                        .project
                        .tracks
                        .iter()
                        .map(|t| (t.id, t.name.clone()))
                        .collect();
                    self.browser.ui(
                        ui,
                        self.engine.sample_rate,
                        self.project.bpm,
                        &self.engine.midi_port_names,
                        has_lpk,
                        &self.engine.audio_device_name,
                        &self.engine.audio_input_name,
                        &telem,
                        &mut self.keyboard_pads,
                        &pad_tracks,
                        self.selected_track,
                        &mut b_actions,
                    );
                    self.process_browser_actions(b_actions);
                });
        }

        // 3. Status footer FIRST among bottom panels (egui pins the first bottom panel to the screen edge).
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
                    if let Some(ref plug) = self.playground_plugin {
                        ui.separator();
                        let ok = self.playground_ok;
                        ui.label(
                            egui::RichText::new(format!(
                                "Playground: {} {}",
                                plug.name,
                                if ok { "● survived" } else { "✕ quarantined" }
                            ))
                            .size(10.5)
                            .color(if ok { ACCENT_TEAL } else { ACCENT_RED }),
                        );
                        if ok
                            && ui
                                .small_button("Insert into selected track")
                                .clicked()
                        {
                            self.process_browser_actions(vec![BrowserAction::InsertAuditionedPlugin]);
                        }
                        if ui.small_button("Clear").clicked() {
                            self.process_browser_actions(vec![BrowserAction::ClearPlayground]);
                        }
                    }
                });
            });

        // 4. Lower Studio Dock (sits above the footer; height filled so resize doesn't snap back)
        if self.show_bottom_dock {
            let dock_h = self.bottom_dock_height.clamp(180.0, 620.0);
            let dock_resp = egui::TopBottomPanel::bottom("ferric_lower_dock")
                .resizable(true)
                .default_height(dock_h)
                .height_range(180.0..=620.0)
                .frame(
                    egui::Frame::new()
                        .fill(BG_PANEL)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .inner_margin(8.0),
                )
                .show(ctx, |ui| {
                    // Fill *this frame's* allocated height only — never max() with the
                    // previous dock_h or the panel ratchets and refuses to shrink.
                    let fill_h = ui.available_height();
                    ui.set_min_height(fill_h);
                    egui::ScrollArea::vertical()
                        .id_salt("ferric_lower_dock_scroll")
                        .max_height(fill_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (tab, label, tip) in [
                            (
                                BottomTab::EditorAndComp,
                                "MIDI",
                                "Piano roll · warp · take comp",
                            ),
                            (
                                BottomTab::FxAndModulators,
                                "FX",
                                "Instruments, inserts, sidechain & modulators",
                            ),
                            (BottomTab::KaossPad, "Kaoss", "XY pad — sculpt selected track sound"),
                            (
                                BottomTab::LaunchPad,
                                "Launch",
                                "Scene fire + mix morph (keys 1–8)",
                            ),
                            (
                                BottomTab::MixingConsole,
                                "Mix",
                                "Channel strip console · Mix A–D recall",
                            ),
                            (
                                BottomTab::RoutingMatrix,
                                "Route",
                                "Sends & routing matrix",
                            ),
                        ] {
                            if pill_toggle(ui, label, self.bottom_tab == tab, ACCENT_FERRIC)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                self.bottom_tab = tab;
                                self.launch_pad_focused = tab == BottomTab::LaunchPad;
                            }
                        }
                        if let Some(tid) = self.selected_track {
                            if let Some(trk) = self.project.find_track(tid) {
                                ui.separator();
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{}{}",
                                        trk.name,
                                        if trk.frozen { " · frozen" } else { "" }
                                    ))
                                    .size(11.5)
                                    .color(TEXT_MAIN),
                                );
                                ui.label(
                                    egui::RichText::new(trk.input_source.short_label())
                                        .size(10.5)
                                        .color(TEXT_DIM),
                                );
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("Hide")
                                .on_hover_text("Hide lower dock (toggle Dock in transport)")
                                .clicked()
                            {
                                self.show_bottom_dock = false;
                            }
                        });
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
                            chrome_card(ui, ACCENT_FERRIC, |ui| {
                                self.kaoss_pad.ui(
                                    ui,
                                    &mut self.project,
                                    self.selected_track,
                                    telem.playhead_beat,
                                    telem.is_playing,
                                    telem.is_recording,
                                    &mut kaoss_actions,
                                );
                            });
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
                        BottomTab::LaunchPad => {
                            self.render_launch_pad(ui);
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
                            DockAction::StatusMessage(msg) => {
                                self.status_banner = msg;
                            }
                            DockAction::RecallMix(slot) => {
                                self.snapshot_before("Recall Mix Snapshot");
                                if self.project.recall_mix_snapshot(slot) {
                                    need_sync = true;
                                }
                            }
                            DockAction::StoreMix(slot) => {
                                self.snapshot_before("Store Mix Snapshot");
                                let letter = ['A', 'B', 'C', 'D'][slot.min(3)];
                                self.project
                                    .capture_mix_snapshot(slot, format!("Mix {letter}"));
                                need_sync = true;
                            }
                            DockAction::AddVcaGroup => {
                                self.snapshot_before("Add VCA Group");
                                let n = self.project.vca_groups.len() + 1;
                                self.project.add_vca_group(format!("VCA {n}"));
                                need_sync = true;
                            }
                            DockAction::HostDnaReseed(fx_id) => {
                                self.snapshot_before("HostDNA Reseed");
                                let seed = compute_host_seed();
                                for t in &mut self.project.tracks {
                                    if let Some(fx) = t.fx_chain.iter_mut().find(|f| f.id == fx_id) {
                                        fx.apply_host_dna_seed(seed);
                                        need_sync = true;
                                        break;
                                    }
                                }
                            }
                            DockAction::HostDnaRandomize(fx_id) => {
                                self.snapshot_before("HostDNA Randomize");
                                let seed = random_host_seed();
                                for t in &mut self.project.tracks {
                                    if let Some(fx) = t.fx_chain.iter_mut().find(|f| f.id == fx_id) {
                                        fx.apply_host_dna_seed(seed);
                                        need_sync = true;
                                        break;
                                    }
                                }
                            }
                            DockAction::HostDnaSibling(fx_id, sib) => {
                                self.snapshot_before("HostDNA Sibling");
                                for t in &mut self.project.tracks {
                                    if let Some(fx) = t.fx_chain.iter_mut().find(|f| f.id == fx_id) {
                                        let base = if fx.host_dna_seed == 0 {
                                            compute_host_seed()
                                        } else {
                                            fx.host_dna_seed
                                        };
                                        fx.apply_host_dna_seed(FxSlot::host_dna_sibling_seed(
                                            base, sib,
                                        ));
                                        need_sync = true;
                                        break;
                                    }
                                }
                            }
                            DockAction::HostDnaPaste(fx_id, seed) => {
                                self.snapshot_before("HostDNA Paste Seed");
                                for t in &mut self.project.tracks {
                                    if let Some(fx) = t.fx_chain.iter_mut().find(|f| f.id == fx_id) {
                                        fx.apply_host_dna_seed(seed);
                                        need_sync = true;
                                        break;
                                    }
                                }
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
                            DockAction::CreateMidiTrackFromAudioBlobs {
                                source_name,
                                start_beat,
                                length_beats,
                                notes,
                            } => {
                                self.snapshot_before("Convert Audio Pitch Blobs to MIDI Track");
                                let note_cnt = notes.len();
                                let new_tid = self
                                    .project
                                    .add_track(format!("{} (Audio→MIDI)", source_name));
                                let clip = TimelineClip::new_midi(
                                    format!("{} Melody", source_name),
                                    start_beat,
                                    length_beats.max(1.0),
                                    notes,
                                );
                                let cid = clip.id;
                                if let Some(trk) = self.project.find_track_mut(new_tid) {
                                    trk.fx_chain.push(FxSlot::new(FxKind::WavetableSynth));
                                    trk.clips.push(clip);
                                }
                                self.selected_track = Some(new_tid);
                                self.selected_clip = Some((new_tid, cid));
                                self.status_banner = format!(
                                    "🎹 Extracted {} MIDI note(s) from '{}' onto a new FerricWave Wavetable Synth track!",
                                    note_cnt, source_name
                                );
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
                    // Consume leftover space so PanelState stores the dragged height, not content height.
                    ui.allocate_space(ui.available_size());
                        }); // ScrollArea
                });
            let measured = dock_resp.response.rect.height();
            if measured > 1.0 {
                self.bottom_dock_height = measured.clamp(180.0, 620.0);
            }
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
                        ArrangementAction::OpenFxPanel(tid) => {
                            self.selected_track = Some(tid);
                            self.inspect_master_fx = false;
                            self.bottom_tab = BottomTab::FxAndModulators;
                            self.show_bottom_dock = true;
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
                        ArrangementAction::GenerateArr {
                            spawn_pad,
                            spawn_bass,
                            spawn_arp,
                            section_idx,
                        } => {
                            let range = section_idx.and_then(|i| self.project.scene_beat_range(i));
                            // Peek: only snapshot when generate will create something.
                            let will_create = !self.project.chord_track.is_empty()
                                && (spawn_pad || spawn_bass || spawn_arp)
                                && range
                                    .map(|(a, b)| {
                                        self.project.chord_track.iter().any(|c| {
                                            c.start_beat < b && c.end_beat() > a
                                        })
                                    })
                                    .unwrap_or(true);
                            if will_create {
                                self.snapshot_before("Generate Arr");
                            }
                            let (ids, card) = self.project.generate_arrangement_from_chords_in_range(
                                VoicingStyle::OpenSpreadPad,
                                spawn_pad,
                                spawn_bass,
                                spawn_arp,
                                range,
                            );
                            if ids.is_empty() {
                                self.arrangement.generate_patch_card = None;
                            } else {
                                self.arrangement.generate_patch_card = Some(card.clone());
                                if let Some(tid) = ids.last().copied() {
                                    self.selected_track = Some(tid);
                                }
                                need_sync = true;
                            }
                            self.status_banner = card;
                        }
                        ArrangementAction::UndoGenerateArr => {
                            if self.history.last_undo_label() == Some("Generate Arr") {
                                self.perform_undo();
                                self.arrangement.generate_patch_card = None;
                                self.status_banner = "Undid Generate Arr.".into();
                            } else {
                                self.status_banner =
                                    "No Generate Arr on top of history — use Ctrl+Z for the last action."
                                        .into();
                            }
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
                        ArrangementAction::SplitAudioToStems {
                            track_id,
                            clip_id,
                        } => {
                            self.split_clip_to_stems(track_id, clip_id);
                            need_sync = true;
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
                        ArrangementAction::AddTrack(kind) => {
                            self.spawn_track_of_kind(kind);
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
    if std::env::args().any(|a| a == "--function-test") {
        let code = function_test::run();
        if code == 0 {
            return Ok(());
        }
        std::process::exit(code);
    }
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("FerricDAW v{}", env!("CARGO_PKG_VERSION")))
            .with_inner_size([1520.0, 900.0])
            .with_min_inner_size([1100.0, 700.0])
            .with_maximized(true)
            .with_drag_and_drop(true)
            .with_icon(std::sync::Arc::new(build_app_icon())),
        // Don't restore a previously saved tiny corner window.
        persist_window: false,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "FerricDAW",
        native_options,
        Box::new(|cc| Ok(Box::new(FerricDawApp::new(cc)))),
    )
}
