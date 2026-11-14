use crate::theme::{
    pill_toggle, ACCENT_AMBER, ACCENT_FERRIC, ACCENT_TEAL, BG_DEEP, BG_ELEVATED, BORDER_SUBTLE,
    TEXT_DIM, TEXT_MAIN,
};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use ferric_engine::{
    import_audio_file, list_audio_input_devices, list_audio_output_devices,
    scan_all_external_plugins, DiscoveredExternalPlugin, EngineTelemetry, PluginQuarantine,
};
use ferric_model::{
    synthesize_browser_sample, AudioBuffer, ExternalPluginFormat, FactoryPreset, FxKind,
    MusicalScale, Project, TrackId, FACTORY_PRESETS, NOTE_PITCH_CLASSES,
};
use std::path::PathBuf;
use std::time::Instant;

pub enum BrowserAction {
    PreviewSample(AudioBuffer),
    DropSampleOnTrack { name: String, buffer: AudioBuffer },
    LoadIntoSampler { name: String, buffer: AudioBuffer },
    ApplyPreset(FactoryPreset),
    LoadExternalPlugin(DiscoveredExternalPlugin),
    ClearPluginQuarantine,
    ImportMidiFile(PathBuf),
    SetupAkaiAndDs10Rig,
    RescanMidi,
    SwitchAudioDevices {
        output: Option<String>,
        input: Option<String>,
    },
    SetInputGainDb(f32),
    LockDetectedBpm(f64),
    KaossNote {
        note: u8,
        is_note_on: bool,
    },
    ProjectMutated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserTab {
    SamplesAndLoops,
    PresetsAndClap,
    HardwareRig,
}

pub struct StudioBrowserState {
    pub visible: bool,
    pub tab: BrowserTab,
    pub search: String,
    pub plugin_search: String,
    pub plugin_format_filter: Option<ExternalPluginFormat>,
    pub custom_plugin_dirs: Vec<PathBuf>,
    pub user_folders: Vec<PathBuf>,
    pub scanned_files: Vec<PathBuf>,
    pub external_plugins: Vec<DiscoveredExternalPlugin>,
    pub quarantine_count: usize,
    pub audio_outputs: Vec<String>,
    pub audio_inputs: Vec<String>,
    pub selected_out: Option<String>,
    pub selected_in: Option<String>,
    pub input_gain_db: f32,
}

impl Default for StudioBrowserState {
    fn default() -> Self {
        let mut custom_plugin_dirs = Vec::new();
        for cand in [
            PathBuf::from("plugins"),
            PathBuf::from("C:\\Users\\ndrpe\\Projects\\FerricDAW\\plugins"),
        ] {
            if cand.is_dir() && !custom_plugin_dirs.contains(&cand) {
                custom_plugin_dirs.push(cand);
            }
        }
        let mut q = PluginQuarantine::load_default();
        let plugins = scan_all_external_plugins(&custom_plugin_dirs, &mut q);
        let q_count = q.count();
        let mut s = Self {
            visible: true,
            tab: BrowserTab::HardwareRig,
            search: String::new(),
            plugin_search: String::new(),
            plugin_format_filter: None,
            custom_plugin_dirs,
            user_folders: Vec::new(),
            scanned_files: Vec::new(),
            external_plugins: plugins,
            quarantine_count: q_count,
            audio_outputs: list_audio_output_devices(),
            audio_inputs: list_audio_input_devices(),
            selected_out: None,
            selected_in: None,
            input_gain_db: 0.0,
        };
        if let Ok(profile) = std::env::var("USERPROFILE") {
            let music = PathBuf::from(profile).join("Music");
            if music.is_dir() {
                s.scan_folder(&music);
                s.user_folders.push(music);
            }
        }
        s
    }
}

impl StudioBrowserState {
    pub fn scan_folder(&mut self, dir: &PathBuf) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten().take(150) {
                let p = entry.path();
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    let e = ext.to_ascii_lowercase();
                    if matches!(
                        e.as_str(),
                        "wav" | "flac" | "mp3" | "ogg" | "aiff" | "mid" | "midi"
                    ) && !self.scanned_files.contains(&p)
                    {
                        self.scanned_files.push(p);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        sample_rate: u32,
        bpm: f64,
        midi_ports: &[String],
        has_lpk25: bool,
        active_out: &str,
        active_in: &str,
        telem: &EngineTelemetry,
        actions: &mut Vec<BrowserAction>,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if pill_toggle(
                ui,
                "🎹 Rig / DS-10",
                self.tab == BrowserTab::HardwareRig,
                ACCENT_FERRIC,
            )
            .clicked()
            {
                self.tab = BrowserTab::HardwareRig;
            }
            if pill_toggle(
                ui,
                "🥁 Samples",
                self.tab == BrowserTab::SamplesAndLoops,
                ACCENT_TEAL,
            )
            .clicked()
            {
                self.tab = BrowserTab::SamplesAndLoops;
            }
            if pill_toggle(
                ui,
                "🎛 CLAP / VST3",
                self.tab == BrowserTab::PresetsAndClap,
                ACCENT_AMBER,
            )
            .clicked()
            {
                self.tab = BrowserTab::PresetsAndClap;
            }
        });
        ui.separator();

        match self.tab {
            BrowserTab::HardwareRig => {
                self.render_hardware_rig_tab(
                    ui,
                    midi_ports,
                    has_lpk25,
                    active_out,
                    active_in,
                    telem,
                    actions,
                );
            }
            BrowserTab::SamplesAndLoops => {
                self.render_samples_tab(ui, sample_rate, bpm, actions);
            }
            BrowserTab::PresetsAndClap => {
                self.render_presets_and_clap_tab(ui, actions);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_hardware_rig_tab(
        &mut self,
        ui: &mut Ui,
        midi_ports: &[String],
        has_lpk25: bool,
        active_out: &str,
        active_in: &str,
        telem: &EngineTelemetry,
        actions: &mut Vec<BrowserAction>,
    ) {
        egui::ScrollArea::vertical()
            .id_salt("hw_rig_scroll")
            .show(ui, |ui| {
                // Featured 1-Click Hardware Studio Rig Builder for Akai LPK25 + Nintendo DS KORG DS-10!
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.2_f32, ACCENT_FERRIC))
                    .corner_radius(6)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("⚡ Akai LPK25 + KORG DS-10 Studio")
                                .size(12.0)
                                .strong()
                                .color(ACCENT_FERRIC),
                        );
                        ui.label(
                            egui::RichText::new(
                                "1-click creates an armed LPK25 Synth/Sampler track + a dedicated Stereo Line-In track for your Nintendo DS (with Noise Gate, EQ, Tape Warmth & Tempo Delay)!",
                            )
                            .size(10.5)
                            .color(TEXT_DIM),
                        );
                        ui.add_space(4.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("🚀 Spawn LPK25 + DS-10 Hybrid Tracks")
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::BLACK),
                                )
                                .fill(ACCENT_FERRIC),
                            )
                            .clicked()
                        {
                            actions.push(BrowserAction::SetupAkaiAndDs10Rig);
                        }
                    });

                ui.add_space(6.0);

                // USB MIDI Status (Akai LPK25 Live Monitor)
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .corner_radius(6)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("🎹 USB MIDI Controllers")
                                    .size(11.5)
                                    .strong(),
                            );
                            if ui
                                .small_button("⟳ Rescan USB")
                                .on_hover_text("Hot-plug detect Akai LPK25 or other USB MIDI gear")
                                .clicked()
                            {
                                actions.push(BrowserAction::RescanMidi);
                            }
                        });

                        if midi_ports.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "No USB MIDI plugged in yet — plug in your Akai LPK25 & click '⟳ Rescan USB'!",
                                )
                                .size(10.5)
                                .color(ACCENT_AMBER),
                            );
                        } else {
                            for name in midi_ports {
                                let col = if has_lpk25 { ACCENT_TEAL } else { TEXT_MAIN };
                                ui.label(
                                    egui::RichText::new(format!("● {}", name))
                                        .size(11.0)
                                        .color(col),
                                );
                            }
                        }

                        ui.horizontal(|ui| {
                            if let Some(n) = telem.last_midi_note {
                                let pc = NOTE_PITCH_CLASSES[(n as usize) % 12];
                                let oct = (n as i32 / 12) - 1;
                                ui.label(
                                    egui::RichText::new(format!("Last Note: {}{} (#{})", pc, oct, n))
                                        .size(10.5)
                                        .monospace()
                                        .color(ACCENT_TEAL),
                                );
                            }
                            if telem.sustain_down {
                                ui.label(
                                    egui::RichText::new("[SUSTAIN]")
                                        .size(10.0)
                                        .strong()
                                        .color(ACCENT_AMBER),
                                );
                            }
                        });
                    });

                ui.add_space(6.0);

                // Audio Line-In / Output Selector + DS-10 Input Trim & Live Peak Meter!
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .corner_radius(6)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("🔌 Audio Interface / DS-10 Line-In")
                                .size(11.5)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(format!("Out: {}", active_out))
                                .size(10.0)
                                .color(TEXT_DIM),
                        );
                        ui.label(
                            egui::RichText::new(format!("In:  {}", active_in))
                                .size(10.0)
                                .color(ACCENT_TEAL),
                        );

                        egui::ComboBox::from_id_salt("hw_in_picker")
                            .selected_text(
                                self.selected_in
                                    .as_deref()
                                    .unwrap_or("Switch Line/Mic Input..."),
                            )
                            .width(210.0)
                            .show_ui(ui, |ui| {
                                for dev in &self.audio_inputs.clone() {
                                    if ui
                                        .selectable_label(
                                            self.selected_in.as_deref() == Some(dev.as_str()),
                                            dev,
                                        )
                                        .clicked()
                                    {
                                        self.selected_in = Some(dev.clone());
                                        actions.push(BrowserAction::SwitchAudioDevices {
                                            output: self.selected_out.clone(),
                                            input: self.selected_in.clone(),
                                        });
                                    }
                                }
                            });

                        egui::ComboBox::from_id_salt("hw_out_picker")
                            .selected_text(
                                self.selected_out
                                    .as_deref()
                                    .unwrap_or("Switch Speaker/Headphone Out..."),
                            )
                            .width(210.0)
                            .show_ui(ui, |ui| {
                                for dev in &self.audio_outputs.clone() {
                                    if ui
                                        .selectable_label(
                                            self.selected_out.as_deref() == Some(dev.as_str()),
                                            dev,
                                        )
                                        .clicked()
                                    {
                                        self.selected_out = Some(dev.clone());
                                        actions.push(BrowserAction::SwitchAudioDevices {
                                            output: self.selected_out.clone(),
                                            input: self.selected_in.clone(),
                                        });
                                    }
                                }
                            });

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Line-In Gain:").size(10.5));
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.input_gain_db)
                                        .range(-24.0..=24.0)
                                        .speed(0.2)
                                        .suffix(" dB"),
                                )
                                .changed()
                            {
                                actions.push(BrowserAction::SetInputGainDb(self.input_gain_db));
                            }
                        });

                        // Live Hardware Line-In Meter (perfect for gain-staging Nintendo DS headphone out!)
                        let (mrect, _) =
                            ui.allocate_exact_size(Vec2::new(210.0, 8.0), Sense::hover());
                        ui.painter().rect_filled(mrect, 2, BG_DEEP);
                        let norm = ((telem.input_peak_db + 60.0) / 66.0).clamp(0.0, 1.0);
                        if norm > 0.01 {
                            ui.painter().rect_filled(
                                Rect::from_min_size(
                                    mrect.min,
                                    Vec2::new(mrect.width() * norm, mrect.height()),
                                ),
                                2,
                                ACCENT_TEAL,
                            );
                        }
                        if let Some(det_bpm) = telem.detected_input_bpm {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "🎯 Line-In Tempo: {:.1} BPM",
                                        det_bpm
                                    ))
                                    .size(10.5)
                                    .strong()
                                    .color(ACCENT_AMBER),
                                );
                                if ui
                                    .small_button("⚡ Lock BPM")
                                    .on_hover_text(
                                        "Sync FerricDAW project BPM to the beat detected on Nintendo DS / Line-In!",
                                    )
                                    .clicked()
                                {
                                    actions.push(BrowserAction::LockDetectedBpm(det_bpm as f64));
                                }
                            });
                        } else {
                            ui.label(
                                egui::RichText::new(
                                    "Play a 4-bar beat on your KORG DS-10 over Line-In for automatic BPM detection (or click TAP)!",
                                )
                                .size(9.5)
                                .color(TEXT_DIM),
                            );
                        }
                    });
            });
    }

    fn render_samples_tab(
        &mut self,
        ui: &mut Ui,
        sample_rate: u32,
        bpm: f64,
        actions: &mut Vec<BrowserAction>,
    ) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Filter WAV / FLAC / MP3 / OGG / MID...")
                    .desired_width(145.0),
            );
            if ui
                .small_button("+ Folder")
                .on_hover_text("Add Sample Pack or Recording Folder")
                .clicked()
            {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.scan_folder(&dir);
                    self.user_folders.push(dir);
                }
            }
        });

        egui::ScrollArea::vertical()
            .id_salt("samples_browser_scroll")
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Built-In Studio Pack (1-Click Audition / Drop / Slice)")
                        .size(10.5)
                        .strong()
                        .color(ACCENT_TEAL),
                );
                for idx in 0..4 {
                    let (name, buf) = synthesize_browser_sample(idx, sample_rate, bpm);
                    if !self.search.is_empty()
                        && !name.to_lowercase().contains(&self.search.to_lowercase())
                    {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("▶").on_hover_text("Audition Preview").clicked() {
                            actions.push(BrowserAction::PreviewSample(buf.clone()));
                        }
                        if ui
                            .small_button("+Trk")
                            .on_hover_text("Drop onto Selected Track at Playhead")
                            .clicked()
                        {
                            actions.push(BrowserAction::DropSampleOnTrack {
                                name: name.clone(),
                                buffer: buf.clone(),
                            });
                        }
                        if ui
                            .small_button("🎹Slice")
                            .on_hover_text("Load into Chromatic/Slice Sampler on Selected Track!")
                            .clicked()
                        {
                            actions.push(BrowserAction::LoadIntoSampler {
                                name: name.clone(),
                                buffer: buf,
                            });
                        }
                        ui.add(egui::Label::new(egui::RichText::new(name).size(10.5)).truncate());
                    });
                }

                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "Disk Audio & MIDI Files ({})",
                        self.scanned_files.len()
                    ))
                    .size(10.5)
                    .strong()
                    .color(TEXT_DIM),
                );

                let q = self.search.to_lowercase();
                for path in self.scanned_files.clone() {
                    let fname = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("file")
                        .to_string();
                    if !q.is_empty() && !fname.to_lowercase().contains(&q) {
                        continue;
                    }
                    let is_midi = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "mid" | "midi"))
                        .unwrap_or(false);

                    ui.horizontal(|ui| {
                        if is_midi {
                            if ui.small_button("🎹 Import .MID").clicked() {
                                actions.push(BrowserAction::ImportMidiFile(path.clone()));
                            }
                        } else {
                            if ui.small_button("▶").clicked() {
                                if let Ok(buf) = import_audio_file(&path, sample_rate) {
                                    actions.push(BrowserAction::PreviewSample(buf));
                                }
                            }
                            if ui.small_button("+Trk").clicked() {
                                if let Ok(buf) = import_audio_file(&path, sample_rate) {
                                    actions.push(BrowserAction::DropSampleOnTrack {
                                        name: fname.clone(),
                                        buffer: buf,
                                    });
                                }
                            }
                        }
                        ui.add(egui::Label::new(egui::RichText::new(fname).size(10.5)).truncate());
                    });
                }
            });
    }

    pub fn rescan_plugins(&mut self) {
        let mut q = PluginQuarantine::load_default();
        self.external_plugins = scan_all_external_plugins(&self.custom_plugin_dirs, &mut q);
        self.quarantine_count = q.count();
    }

    fn render_presets_and_clap_tab(&mut self, ui: &mut Ui, actions: &mut Vec<BrowserAction>) {
        egui::ScrollArea::vertical()
            .id_salt("presets_clap_scroll")
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Curated Studio FX & Synth Presets")
                        .size(11.0)
                        .strong()
                        .color(ACCENT_FERRIC),
                );
                for preset in FACTORY_PRESETS {
                    ui.horizontal(|ui| {
                        if ui.small_button("+ Load").clicked() {
                            actions.push(BrowserAction::ApplyPreset(preset.clone()));
                        }
                        ui.label(
                            egui::RichText::new(format!("[{}]", preset.category))
                                .size(9.5)
                                .color(ACCENT_TEAL),
                        );
                        ui.add(
                            egui::Label::new(egui::RichText::new(preset.name).size(10.5)).truncate(),
                        );
                    });
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "CLAP & VST3 Plugins ({})",
                            self.external_plugins.len()
                        ))
                        .size(11.0)
                        .strong()
                        .color(ACCENT_AMBER),
                    );
                    if ui
                        .small_button("⟳ Scan")
                        .on_hover_text(
                            "Rescan standard CommonProgramFiles\\CLAP and CommonProgramFiles\\VST3 directories",
                        )
                        .clicked()
                    {
                        self.rescan_plugins();
                    }
                    if ui
                        .small_button("+ Dir")
                        .on_hover_text("Add & scan a custom .clap or .vst3 plugin directory")
                        .clicked()
                    {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            if !self.custom_plugin_dirs.contains(&dir) {
                                self.custom_plugin_dirs.push(dir);
                            }
                            self.rescan_plugins();
                        }
                    }
                });

                // Search + Format Filter Pills (All / CLAP / VST3)
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.plugin_search)
                            .hint_text("Search Vital, Surge, FabFilter...")
                            .desired_width(120.0),
                    );
                    if pill_toggle(ui, "All", self.plugin_format_filter.is_none(), ACCENT_TEAL)
                        .clicked()
                    {
                        self.plugin_format_filter = None;
                    }
                    if pill_toggle(
                        ui,
                        "CLAP",
                        self.plugin_format_filter == Some(ExternalPluginFormat::Clap),
                        ACCENT_FERRIC,
                    )
                    .clicked()
                    {
                        self.plugin_format_filter = Some(ExternalPluginFormat::Clap);
                    }
                    if pill_toggle(
                        ui,
                        "VST3",
                        self.plugin_format_filter == Some(ExternalPluginFormat::Vst3),
                        ACCENT_AMBER,
                    )
                    .clicked()
                    {
                        self.plugin_format_filter = Some(ExternalPluginFormat::Vst3);
                    }
                });

                if self.quarantine_count > 0 {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "🛡 {} crashed plugin(s) quarantined",
                                self.quarantine_count
                            ))
                            .size(9.5)
                            .color(ACCENT_AMBER),
                        );
                        if ui.small_button("Reset").clicked() {
                            actions.push(BrowserAction::ClearPluginQuarantine);
                            self.rescan_plugins();
                        }
                    });
                }

                let pq = self.plugin_search.trim().to_lowercase();
                let filtered: Vec<&DiscoveredExternalPlugin> = self
                    .external_plugins
                    .iter()
                    .filter(|p| {
                        self.plugin_format_filter
                            .map(|f| p.format == f)
                            .unwrap_or(true)
                            && (pq.is_empty()
                                || p.name.to_lowercase().contains(&pq)
                                || p.vendor.to_lowercase().contains(&pq))
                    })
                    .collect();

                if filtered.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "No matching .clap or .vst3 plugins found. Install any Windows CLAP or VST3 synth/effect & click '⟳ Scan' or '+ Dir'!",
                        )
                        .size(10.0)
                        .color(TEXT_DIM),
                    );
                } else {
                    for plug in filtered {
                        ui.horizontal(|ui| {
                            if ui.small_button("+ Insert").clicked() {
                                actions.push(BrowserAction::LoadExternalPlugin(plug.clone()));
                            }
                            let badge_col = match plug.format {
                                ExternalPluginFormat::Clap => ACCENT_FERRIC,
                                ExternalPluginFormat::Vst3 => ACCENT_AMBER,
                            };
                            ui.label(
                                egui::RichText::new(format!("[{}]", plug.format.badge()))
                                    .size(9.5)
                                    .monospace()
                                    .color(badge_col),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("{} — {}", plug.name, plug.vendor))
                                        .size(10.5),
                                )
                                .truncate(),
                            );
                        });
                    }
                }
            });
    }
}

/// KORG DS-10 Inspired Interactive Touch KAOSS X-Y Pad!
/// Drag horizontally/vertically with the mouse (just like the Nintendo DS stylus!) to:
/// 1) Morph Filter Cutoff & Resonance / Delay Feedback on any selected Synth/Audio track in real-time
/// 2) Optionally trigger scale-locked KAOSSilator notes in the project's Musical Key & Scale!
pub struct KaossPadState {
    pub x: f32,
    pub y: f32,
    pub note_trigger_mode: bool,
    pub octave: u8,
    active_note: Option<u8>,
    trail: Vec<Pos2>,
}

impl Default for KaossPadState {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            note_trigger_mode: true,
            octave: 4,
            active_note: None,
            trail: Vec::with_capacity(24),
        }
    }
}

impl KaossPadState {
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        selected_track: Option<TrackId>,
        actions: &mut Vec<BrowserAction>,
    ) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("🎮 KORG DS-10 Style KAOSS X-Y Pad")
                    .strong()
                    .color(ACCENT_FERRIC),
            );
            ui.checkbox(
                &mut self.note_trigger_mode,
                "KAOSSilator Scale-Note Synth Trigger (Stylus Drag)",
            );
            ui.label(egui::RichText::new("Octave:").size(11.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.octave).range(2..=6));
            ui.separator();
            ui.label(egui::RichText::new("Session Key:").size(11.0).color(TEXT_DIM));
            egui::ComboBox::from_id_salt("kaoss_root_key")
                .selected_text(NOTE_PITCH_CLASSES[(project.root_pitch_class as usize) % 12])
                .width(54.0)
                .show_ui(ui, |ui| {
                    for (i, &nm) in NOTE_PITCH_CLASSES.iter().enumerate() {
                        ui.selectable_value(&mut project.root_pitch_class, i as u8, nm);
                    }
                });
            egui::ComboBox::from_id_salt("kaoss_scale_mode")
                .selected_text(project.musical_scale.label())
                .width(155.0)
                .show_ui(ui, |ui| {
                    for &sc in MusicalScale::ALL {
                        ui.selectable_value(&mut project.musical_scale, sc, sc.label());
                    }
                });
        });

        let pad_size = Vec2::new(ui.available_width().min(540.0), (ui.available_height() - 8.0).max(130.0));
        let (rect, resp) = ui.allocate_exact_size(pad_size, Sense::click_and_drag());
        let p = ui.painter();
        p.rect_filled(rect, 8, BG_DEEP);
        p.rect_stroke(
            rect,
            8,
            Stroke::new(1.5_f32, ACCENT_FERRIC),
            StrokeKind::Inside,
        );

        // Draw DS-10 grid lines
        for g in 1..8 {
            let gx = rect.left() + rect.width() * (g as f32 / 8.0);
            let gy = rect.top() + rect.height() * (g as f32 / 8.0);
            p.line_segment(
                [Pos2::new(gx, rect.top()), Pos2::new(gx, rect.bottom())],
                Stroke::new(0.8_f32, Color32::from_rgb(30, 34, 44)),
            );
            p.line_segment(
                [Pos2::new(rect.left(), gy), Pos2::new(rect.right(), gy)],
                Stroke::new(0.8_f32, Color32::from_rgb(30, 34, 44)),
            );
        }

        if resp.is_pointer_button_down_on() {
            if let Some(pos) = resp.interact_pointer_pos() {
                self.x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                self.y = (1.0 - (pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);

                if self.trail.len() >= 20 {
                    self.trail.remove(0);
                }
                self.trail.push(pos);

                // Morph Selected Track's Synth/Sampler/EQ/Delay Filter X-Y in real-time!
                if let Some(tid) = selected_track {
                    if let Some(trk) = project.find_track_mut(tid) {
                        for fx in &mut trk.fx_chain {
                            match fx.kind {
                                FxKind::PolySynth => {
                                    // X = Filter Cutoff (120Hz..12kHz), Y = Resonance + Detune!
                                    let cutoff = 120.0 * (100.0_f32).powf(self.x);
                                    fx.set_param(2, cutoff);
                                    fx.set_param(3, 0.12 + self.y * 0.72);
                                }
                                FxKind::SamplePlayer => {
                                    let cutoff = 180.0 * (85.0_f32).powf(self.x);
                                    fx.set_param(3, cutoff);
                                    fx.set_param(4, 0.10 + self.y * 0.72);
                                }
                                FxKind::StereoDelay => {
                                    fx.set_param(2, self.y * 82.0);
                                    fx.set_param(4, 900.0 + self.x * 9000.0);
                                }
                                _ => {}
                            }
                        }
                    }
                    actions.push(BrowserAction::ProjectMutated);
                }

                // Trigger Scale-Quantized KAOSSilator notes!
                if self.note_trigger_mode {
                    let base_midi = ((self.octave as i32 + 1) * 12).clamp(24, 84) as u8;
                    let raw_pitch = base_midi + (self.x * 24.0).round() as u8;
                    let snapped = project
                        .musical_scale
                        .snap_pitch(project.root_pitch_class, raw_pitch);
                    if self.active_note != Some(snapped) {
                        if let Some(prev) = self.active_note.take() {
                            actions.push(BrowserAction::KaossNote {
                                note: prev,
                                is_note_on: false,
                            });
                        }
                        self.active_note = Some(snapped);
                        actions.push(BrowserAction::KaossNote {
                            note: snapped,
                            is_note_on: true,
                        });
                    }
                }
            }
        } else {
            if let Some(prev) = self.active_note.take() {
                actions.push(BrowserAction::KaossNote {
                    note: prev,
                    is_note_on: false,
                });
            }
            if !self.trail.is_empty() {
                self.trail.remove(0);
            }
        }

        // Render glowing DS-10 stylus trail & crosshair!
        for (i, &pt) in self.trail.iter().enumerate() {
            let alpha = ((i + 1) as f32 / 22.0) * 0.65;
            p.circle_filled(pt, 3.0 + i as f32 * 0.35, ACCENT_TEAL.gamma_multiply(alpha));
        }

        let cx = rect.left() + self.x * rect.width();
        let cy = rect.bottom() - self.y * rect.height();
        p.line_segment(
            [Pos2::new(cx, rect.top()), Pos2::new(cx, rect.bottom())],
            Stroke::new(1.0_f32, ACCENT_FERRIC.gamma_multiply(0.55)),
        );
        p.line_segment(
            [Pos2::new(rect.left(), cy), Pos2::new(rect.right(), cy)],
            Stroke::new(1.0_f32, ACCENT_FERRIC.gamma_multiply(0.55)),
        );
        p.circle_filled(Pos2::new(cx, cy), 8.0, ACCENT_FERRIC);
        p.circle_stroke(Pos2::new(cx, cy), 12.0, Stroke::new(1.5_f32, ACCENT_TEAL));

        p.text(
            Pos2::new(rect.left() + 10.0, rect.bottom() - 10.0),
            Align2::LEFT_BOTTOM,
            format!("X (Pitch / Cutoff): {:03.0}%   Y (Resonance / Feedback): {:03.0}%", self.x * 100.0, self.y * 100.0),
            FontId::monospace(10.5),
            TEXT_MAIN,
        );
    }
}

/// Tap-Tempo Averager so you can tap along to your Nintendo DS KORG DS-10 beat in 4 clicks!
#[derive(Default)]
pub struct TapTempoTracker {
    taps: Vec<Instant>,
}

impl TapTempoTracker {
    pub fn register_tap(&mut self) -> Option<f64> {
        let now = Instant::now();
        if let Some(&last) = self.taps.last() {
            if now.duration_since(last).as_secs_f64() > 2.5 {
                self.taps.clear();
            }
        }
        self.taps.push(now);
        if self.taps.len() > 6 {
            self.taps.remove(0);
        }
        if self.taps.len() < 2 {
            return None;
        }
        let mut intervals = 0.0_f64;
        for w in self.taps.windows(2) {
            intervals += w[1].duration_since(w[0]).as_secs_f64();
        }
        let avg_sec = intervals / (self.taps.len() - 1) as f64;
        if avg_sec > 0.15 {
            Some((60.0 / avg_sec).clamp(30.0, 280.0))
        } else {
            None
        }
    }
}
