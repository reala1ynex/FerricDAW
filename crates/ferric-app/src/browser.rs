use crate::theme::{
    chrome_card, chrome_row_label, pill_toggle, ACCENT_AMBER, ACCENT_FERRIC, ACCENT_TEAL, BG_DEEP,
    BORDER_SUBTLE, TEXT_DIM, TEXT_MAIN,
};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use ferric_engine::{
    import_audio_file, list_audio_device_matrix, list_audio_input_devices,
    list_audio_output_devices, realtek_combo_jack_risk, scan_all_external_plugins,
    AudioDeviceInfo, AudioDriverKind, DiscoveredExternalPlugin, EngineTelemetry, PluginQuarantine,
};
use ferric_model::{
    synthesize_browser_sample, AudioBuffer, ExternalPluginFormat, FactoryPreset, FxKind, FxSlot,
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
    /// Crashproof Plugin Playground: sandbox audition first.
    AuditionPlugin(DiscoveredExternalPlugin),
    InsertAuditionedPlugin,
    ClearPlayground,
    ClearPluginQuarantine,
    ImportMidiFile(PathBuf),
    SetupHardwareRig,
    SetupHardwareRigPreset(u8),
    SpawnDualKeyboardStudio,
    /// Force Speakers out + External Mic/Line in (helps combo-jack mute when a line source is plugged).
    FixLineInComboJackRouting,
    RescanMidi,
    SwitchAudioDevices {
        output: Option<String>,
        input: Option<String>,
        buffer_size: Option<u32>,
        recording_offset_samples: u32,
    },
    SetInputGainDb(f32),
    SetRecordingOffsetSamples(u32),
    TriggerLoopbackPing,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TheaterSkin {
    #[default]
    DefaultDesk,
    LaptopPads,
    HandheldTip,
}

impl TheaterSkin {
    pub fn label(self) -> &'static str {
        match self {
            Self::DefaultDesk => "Default Desk",
            Self::LaptopPads => "Laptop Pads",
            Self::HandheldTip => "Handheld Tip",
        }
    }
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
    pub selected_buffer_size: Option<u32>,
    pub selected_audio_driver: AudioDriverKind,
    pub recording_offset_samples: u32,
    pub input_gain_db: f32,
    /// Cached I/O matrix — never call `list_audio_device_matrix` every frame.
    pub io_matrix_cache: Vec<AudioDeviceInfo>,
    pub theater_skin: TheaterSkin,
    /// Honest Exclusive/ASIO fallback note from the last engine start.
    pub driver_honesty_note: Option<String>,
    /// Live Exclusive session active (not merely selected in the driver combo).
    pub live_exclusive: bool,
}

impl Default for StudioBrowserState {
    fn default() -> Self {
        let mut custom_plugin_dirs = Vec::new();
        let mut plugin_cands = vec![PathBuf::from("plugins")];
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                plugin_cands.push(dir.join("plugins"));
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            plugin_cands.push(cwd.join("plugins"));
        }
        for cand in plugin_cands {
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
            selected_buffer_size: None,
            selected_audio_driver: AudioDriverKind::WasapiShared,
            recording_offset_samples: 0,
            input_gain_db: 0.0,
            io_matrix_cache: Vec::new(),
            theater_skin: TheaterSkin::DefaultDesk,
            driver_honesty_note: None,
            live_exclusive: false,
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
        keyboard_pads: &mut crate::keyboard_pads::KeyboardPadRig,
        pad_tracks: &[(ferric_model::TrackId, String)],
        selected_track: Option<ferric_model::TrackId>,
        actions: &mut Vec<BrowserAction>,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if pill_toggle(
                ui,
                "🎹 Hardware Rig",
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
                "🎛 Presets / Plugins",
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
                    keyboard_pads,
                    pad_tracks,
                    selected_track,
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
        keyboard_pads: &mut crate::keyboard_pads::KeyboardPadRig,
        pad_tracks: &[(ferric_model::TrackId, String)],
        selected_track: Option<ferric_model::TrackId>,
        actions: &mut Vec<BrowserAction>,
    ) {
        egui::ScrollArea::vertical()
            .id_salt("hw_rig_scroll")
            .show(ui, |ui| {
                // Hardware Rig Theater — desk layout + skins (Pad A/B chrome)
                chrome_card(ui, ACCENT_TEAL, |ui| {
                        ui.label(
                            egui::RichText::new("🎭 Hardware Rig Theater")
                                .size(12.0)
                                .strong()
                                .color(ACCENT_TEAL),
                        );
                        ui.horizontal(|ui| {
                            for skin in [
                                TheaterSkin::DefaultDesk,
                                TheaterSkin::LaptopPads,
                                TheaterSkin::HandheldTip,
                            ] {
                                if pill_toggle(
                                    ui,
                                    skin.label(),
                                    self.theater_skin == skin,
                                    ACCENT_FERRIC,
                                )
                                .clicked()
                                {
                                    self.theater_skin = skin;
                                }
                            }
                        });
                        ui.add_space(4.0);
                        let want_excl = matches!(
                            self.selected_audio_driver,
                            AudioDriverKind::WasapiExclusive
                        );
                        let want_asio =
                            matches!(self.selected_audio_driver, AudioDriverKind::Asio);
                        let live_excl = self.live_exclusive;
                        let asio_fell_back = want_asio
                            && self
                                .driver_honesty_note
                                .as_ref()
                                .map(|n| n.to_ascii_lowercase().contains("asio"))
                                .unwrap_or(false);
                        let status = match (want_asio, asio_fell_back, want_excl, live_excl) {
                            (true, true, _, _) => "ASIO → WASAPI fallback",
                            (true, false, _, _) => "ASIO requested",
                            (false, _, true, true) => "Exclusive ● locked",
                            (false, _, true, false) => "Exclusive → Shared fallback",
                            _ => "Shared",
                        };
                        let buf = self
                            .selected_buffer_size
                            .map(|b| format!("{b}"))
                            .unwrap_or_else(|| "auto".into());
                        let ping = telem
                            .measured_loopback_samples
                            .map(|s| format!("{s} smp"))
                            .unwrap_or_else(|| "—".into());
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · out {} · buf {} · ping {}",
                                status, active_out, buf, ping
                            ))
                            .size(11.0)
                            .strong()
                            .monospace()
                            .color(if live_excl {
                                ACCENT_TEAL
                            } else if want_excl {
                                ACCENT_AMBER
                            } else {
                                TEXT_DIM
                            }),
                        );
                        if let Some(note) = self.driver_honesty_note.as_ref() {
                            ui.label(
                                egui::RichText::new(note.as_str())
                                    .size(10.0)
                                    .color(ACCENT_AMBER),
                            );
                        }
                        ui.add_space(4.0);
                        match self.theater_skin {
                            TheaterSkin::DefaultDesk => {
                                ui.columns(3, |cols| {
                                    cols[0].label(
                                        egui::RichText::new("MIDI")
                                            .size(11.0)
                                            .strong()
                                            .color(ACCENT_FERRIC),
                                    );
                                    cols[0].label(
                                        egui::RichText::new(if midi_ports.is_empty() {
                                            "No USB MIDI"
                                        } else {
                                            "Controller ready"
                                        })
                                        .size(10.0)
                                        .color(TEXT_DIM),
                                    );
                                    cols[1].label(
                                        egui::RichText::new("XY / Pads")
                                            .size(11.0)
                                            .strong()
                                            .color(ACCENT_TEAL),
                                    );
                                    cols[1].label(
                                        egui::RichText::new(if keyboard_pads.enabled {
                                            "Keyboard pads ON"
                                        } else {
                                            "Kaoss + dual pads"
                                        })
                                        .size(10.0)
                                        .color(TEXT_DIM),
                                    );
                                    cols[2].label(
                                        egui::RichText::new("Line-In")
                                            .size(11.0)
                                            .strong()
                                            .color(ACCENT_AMBER),
                                    );
                                    cols[2].label(
                                        egui::RichText::new(format!(
                                            "{:.0} dB in",
                                            telem.input_peak_db
                                        ))
                                        .size(10.0)
                                        .color(TEXT_DIM),
                                    );
                                });
                            }
                            TheaterSkin::LaptopPads => {
                                ui.label(
                                    egui::RichText::new(
                                        "Laptop Pads skin — letters = Pad A synth, numpad = Pad B kit. Tip preset only; any MIDI works.",
                                    )
                                    .size(10.5)
                                    .color(TEXT_DIM),
                                );
                            }
                            TheaterSkin::HandheldTip => {
                                ui.label(
                                    egui::RichText::new(
                                        "Handheld Tip skin — Line-In + latency truth for phone/DS-style sources. Same generic I/O.",
                                    )
                                    .size(10.5)
                                    .color(TEXT_DIM),
                                );
                            }
                        }
                });
                ui.add_space(6.0);

                chrome_card(ui, ACCENT_FERRIC, |ui| {
                        ui.label(
                            egui::RichText::new("⚡ Hardware Studio Rig")
                                .size(12.0)
                                .strong()
                                .color(ACCENT_FERRIC),
                        );
                        ui.label(
                            egui::RichText::new(
                                "1-click creates an armed USB MIDI synth track plus a Line-In capture track (EQ + compressor). Works with any controller and any line-level source.",
                            )
                            .size(10.5)
                            .color(TEXT_DIM),
                        );
                        ui.add_space(4.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Spawn MIDI + Line-In Rig")
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::BLACK),
                                )
                                .fill(ACCENT_FERRIC),
                            )
                            .clicked()
                        {
                            actions.push(BrowserAction::SetupHardwareRig);
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("MIDI only").clicked() {
                                actions.push(BrowserAction::SetupHardwareRigPreset(2));
                            }
                            if ui.small_button("Line-In only").clicked() {
                                actions.push(BrowserAction::SetupHardwareRigPreset(3));
                            }
                            if ui
                                .small_button("Keyboard + handheld tip")
                                .on_hover_text(
                                    "Same generic MIDI + Line-In tracks — tip naming only, not brand-required.",
                                )
                                .clicked()
                            {
                                actions.push(BrowserAction::SetupHardwareRigPreset(1));
                            }
                        });
                        ui.add_space(4.0);
                        if ui
                            .button(
                                egui::RichText::new("🔊 Fix: Force Speakers + Line-In")
                                    .size(11.0)
                                    .strong(),
                            )
                            .on_hover_text(
                                "Combo jacks often switch Windows to Headphones and mute Speakers. This locks Out=Speakers and In=External Microphone / Line-In.",
                            )
                            .clicked()
                        {
                            actions.push(BrowserAction::FixLineInComboJackRouting);
                        }
                        ui.add_space(4.0);
                        if ui
                            .button(
                                egui::RichText::new("⌨️ Spawn Dual Keyboard Studio (Prism + Kit)")
                                    .size(11.0)
                                    .strong(),
                            )
                            .on_hover_text(
                                "Creates Ferric Prism on Pad A and a Drum Kit on Pad B, then enables dual keyboard MIDI pads.",
                            )
                            .clicked()
                        {
                            actions.push(BrowserAction::SpawnDualKeyboardStudio);
                        }
                });

                ui.add_space(6.0);

                chrome_card(ui, ACCENT_AMBER, |ui| {
                    keyboard_pads.render_panel(ui, pad_tracks, selected_track);
                });

                ui.add_space(6.0);

                // USB MIDI Status
                chrome_card(ui, BORDER_SUBTLE, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("USB MIDI Controllers")
                                .size(11.5)
                                .strong()
                                .color(TEXT_MAIN),
                        );
                        if ui
                            .small_button("Rescan USB")
                            .on_hover_text("Hot-plug detect any USB MIDI controller")
                            .clicked()
                        {
                            actions.push(BrowserAction::RescanMidi);
                        }
                    });

                    if midi_ports.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "No USB MIDI plugged in yet — connect a controller and click Rescan USB.",
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

                // Audio I/O — same row chrome as Pad A / Pad B pickers
                chrome_card(ui, BORDER_SUBTLE, |ui| {
                    ui.label(
                        egui::RichText::new("Audio I/O")
                            .size(11.5)
                            .strong()
                            .color(TEXT_MAIN),
                    );
                    if realtek_combo_jack_risk(active_out, active_in)
                        || active_out.to_lowercase().contains("headphone")
                    {
                        ui.colored_label(
                            ACCENT_AMBER,
                            egui::RichText::new(
                                "Combo-jack trap: Out is Headphones. Prefer Speakers + External Mic / Line-In.",
                            )
                            .size(10.5),
                        );
                    }

                    ui.horizontal(|ui| {
                        chrome_row_label(ui, "In ->", ACCENT_TEAL);
                        egui::ComboBox::from_id_salt("hw_in_picker")
                            .selected_text(
                                self.selected_in
                                    .as_deref()
                                    .or(Some(active_in))
                                    .unwrap_or("Switch Line/Mic Input..."),
                            )
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                let inputs = self.audio_inputs.clone();
                                for dev in &inputs {
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
                                            buffer_size: self.selected_buffer_size,
                                            recording_offset_samples: self.recording_offset_samples,
                                        });
                                    }
                                }
                            });
                    });

                    ui.horizontal(|ui| {
                        chrome_row_label(ui, "Out ->", ACCENT_FERRIC);
                        egui::ComboBox::from_id_salt("hw_out_picker")
                            .selected_text(
                                self.selected_out
                                    .as_deref()
                                    .or(Some(active_out))
                                    .unwrap_or("Switch Speaker/Headphone Out..."),
                            )
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                let outputs = self.audio_outputs.clone();
                                for dev in &outputs {
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
                                            buffer_size: self.selected_buffer_size,
                                            recording_offset_samples: self.recording_offset_samples,
                                        });
                                    }
                                }
                            });
                    });

                    ui.horizontal(|ui| {
                        chrome_row_label(ui, "Driver ->", ACCENT_AMBER);
                        egui::ComboBox::from_id_salt("hw_driver_picker")
                            .selected_text(self.selected_audio_driver.label())
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for &drv in AudioDriverKind::ALL {
                                    if ui
                                        .selectable_label(
                                            self.selected_audio_driver == drv,
                                            drv.label(),
                                        )
                                        .on_hover_text(drv.hover_hint())
                                        .clicked()
                                    {
                                        self.selected_audio_driver = drv;
                                        actions.push(BrowserAction::SwitchAudioDevices {
                                            output: self.selected_out.clone(),
                                            input: self.selected_in.clone(),
                                            buffer_size: self.selected_buffer_size,
                                            recording_offset_samples: self.recording_offset_samples,
                                        });
                                    }
                                }
                            });
                    });

                        // Multi-channel device matrix summary (cached — WASAPI enum every frame kills FPS)
                        ui.collapsing("I/O Channel Matrix", |ui| {
                            if self.io_matrix_cache.is_empty() {
                                self.io_matrix_cache = list_audio_device_matrix();
                            }
                            if ui.small_button("Refresh matrix").clicked() {
                                self.io_matrix_cache = list_audio_device_matrix();
                                self.audio_inputs = list_audio_input_devices();
                                self.audio_outputs = list_audio_output_devices();
                            }
                            for info in self.io_matrix_cache.iter().take(12) {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{}  ·  in {} / out {}  ·  {} Hz  ·  buf {}–{}",
                                        info.name,
                                        info.input_channels,
                                        info.output_channels,
                                        info.default_sample_rate,
                                        info.min_buffer,
                                        info.max_buffer
                                    ))
                                    .size(10.0)
                                    .color(TEXT_DIM),
                                );
                            }
                            ui.label(
                                egui::RichText::new(
                                    "Per-track: Mixer input picker selects In 1/2 … In 15/16 pairs or mono In 1–16.",
                                )
                                .size(10.0)
                                .color(ACCENT_TEAL),
                            );
                        });

                        // Buffer — Pad A/B row chrome
                        let buf_ms = (telem.buffer_size_frames as f32 * 1000.0)
                            / (telem.sample_rate.max(8_000) as f32);
                        let buf_lbl = match self.selected_buffer_size {
                            None => format!(
                                "Auto ({} smp / {:.1} ms)",
                                telem.buffer_size_frames, buf_ms
                            ),
                            Some(n) => format!(
                                "{} smp ({:.1} ms)",
                                n,
                                (n as f32 * 1000.0) / (telem.sample_rate.max(8_000) as f32)
                            ),
                        };
                        ui.horizontal(|ui| {
                            chrome_row_label(ui, "Buffer ->", TEXT_DIM);
                            egui::ComboBox::from_id_salt("hw_buf_size_picker")
                                .selected_text(buf_lbl)
                                .width(200.0)
                                .show_ui(ui, |ui| {
                                    for (opt, label) in [
                                        (None, "Driver Default (Auto)"),
                                        (Some(64), "64 samples (~1.3 ms Ultra-Low)"),
                                        (Some(128), "128 samples (~2.7 ms Live Tracking)"),
                                        (Some(256), "256 samples (~5.3 ms Balanced)"),
                                        (Some(512), "512 samples (~10.7 ms Mixing)"),
                                        (Some(1024), "1024 samples (~21.3 ms Heavy Mastering)"),
                                    ] {
                                        if ui
                                            .selectable_label(
                                                self.selected_buffer_size == opt,
                                                label,
                                            )
                                            .clicked()
                                        {
                                            self.selected_buffer_size = opt;
                                            actions.push(BrowserAction::SwitchAudioDevices {
                                                output: self.selected_out.clone(),
                                                input: self.selected_in.clone(),
                                                buffer_size: self.selected_buffer_size,
                                                recording_offset_samples: self
                                                    .recording_offset_samples,
                                            });
                                        }
                                    }
                                });
                        });

                        // Round-Trip Latency Offset & 1-Click Loopback Ping Calibrator!
                        ui.horizontal(|ui| {
                            let off_ms = (self.recording_offset_samples as f32 * 1000.0)
                                / (telem.sample_rate.max(8_000) as f32);
                            ui.label(
                                egui::RichText::new(format!("Rec Offset ({:.1}ms):", off_ms))
                                    .size(10.0)
                                    .color(TEXT_DIM),
                            );
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.recording_offset_samples)
                                        .range(0..=24_000)
                                        .speed(8.0)
                                        .suffix(" smp"),
                                )
                                .on_hover_text(
                                    "Trims round-trip ADC+DAC latency from recorded overdubs so Line-In / mics / guitars align sample-tight.",
                                )
                                .changed()
                            {
                                actions.push(BrowserAction::SetRecordingOffsetSamples(
                                    self.recording_offset_samples,
                                ));
                            }
                        });

                        ui.horizontal(|ui| {
                            let ping_lbl = if telem.ping_in_progress {
                                "Listening..."
                            } else {
                                "Loopback Ping"
                            };
                            if ui
                                .small_button(ping_lbl)
                                .on_hover_text(
                                    "Route Out → Line-In (or mic near speaker) and measure round-trip hardware latency.",
                                )
                                .clicked()
                            {
                                actions.push(BrowserAction::TriggerLoopbackPing);
                            }
                            if let Some(meas) = telem.measured_loopback_samples {
                                let m_ms =
                                    (meas as f32 * 1000.0) / (telem.sample_rate.max(8_000) as f32);
                                if ui
                                    .small_button(format!("Apply {} smp ({:.1}ms)", meas, m_ms))
                                    .clicked()
                                {
                                    self.recording_offset_samples = meas;
                                    actions.push(BrowserAction::SetRecordingOffsetSamples(meas));
                                }
                            }
                        });
                        let ping_ms = telem
                            .measured_loopback_samples
                            .map(|s| (s as f32 * 1000.0) / (telem.sample_rate.max(8_000) as f32));
                        ui.label(
                            egui::RichText::new(match ping_ms {
                                Some(ms) => format!(
                                    "Latency: {} frames ({:.2} ms buffer) · last ping {:.1} ms",
                                    telem.buffer_size_frames, buf_ms, ms
                                ),
                                None => format!(
                                    "Latency: {} frames ({:.2} ms buffer) · ping not measured yet",
                                    telem.buffer_size_frames, buf_ms
                                ),
                            })
                            .size(10.5)
                            .color(ACCENT_TEAL),
                        );

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Line-In Gain:").size(10.5));
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.input_gain_db)
                                        .range(-24.0..=36.0)
                                        .speed(0.2)
                                        .suffix(" dB"),
                                )
                                .changed()
                            {
                                actions.push(BrowserAction::SetInputGainDb(self.input_gain_db));
                            }
                            if ui
                                .small_button("+6 DS")
                                .on_hover_text(
                                    "Modest Line-In trim — keep Windows Mic Boost at 0% to avoid hiss",
                                )
                                .clicked()
                            {
                                self.input_gain_db = 6.0;
                                actions.push(BrowserAction::SetInputGainDb(self.input_gain_db));
                            }
                        });

                        // Live hardware Line-In meter (gain-stage headphone / line outs)
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
                        ui.label(
                            egui::RichText::new(format!(
                                "Input peak: {:+.1} dBFS",
                                telem.input_peak_db
                            ))
                            .size(10.0)
                            .monospace()
                            .color(if telem.input_peak_db < -50.0 {
                                ACCENT_AMBER
                            } else {
                                ACCENT_TEAL
                            }),
                        );
                        if telem.input_peak_db < -50.0 {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(
                                        "No usable input. Set Windows Recording → External Microphone / Line-In as Default. Mic Boost = 0%. Raise Ferric Line-In Gain. Use TRS into the mic/combo jack — not Steam/AMD Array.",
                                    )
                                    .size(10.0)
                                    .color(ACCENT_AMBER),
                                )
                                .wrap(),
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
                                        "Sync project BPM to the beat detected on Line-In",
                                    )
                                    .clicked()
                                {
                                    actions.push(BrowserAction::LockDetectedBpm(det_bpm as f64));
                                }
                            });
                        } else {
                            ui.label(
                                egui::RichText::new(
                                    "Play a 4-bar beat over Line-In for automatic BPM detection (or click TAP)",
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
                ui.label(
                    egui::RichText::new("🛡 Plugin Playground (sandbox audition → insert)")
                        .size(11.0)
                        .strong()
                        .color(ACCENT_TEAL),
                );
                ui.label(
                    egui::RichText::new(
                        "Audition runs sandboxed. Crash quarantines the plugin. Survive → Insert into selected track.",
                    )
                    .size(10.0)
                    .color(TEXT_DIM),
                );
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
                            if ui
                                .small_button("▶ Audition")
                                .on_hover_text("Sandbox audition in Plugin Playground")
                                .clicked()
                            {
                                actions.push(BrowserAction::AuditionPlugin(plug.clone()));
                            }
                            if ui
                                .small_button("+ Insert")
                                .on_hover_text("Insert sandboxed plugin on selected track")
                                .clicked()
                            {
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

/// Interactive Kaoss / XY performance pad.
/// Drag sculpts the selected track's instrument (and insert FX) live — cutoff, resonance, morph, space.
/// Optional scale-locked note trigger rides on top of that effect morph.
/// With **Write XY automation** on, play/record captures the morph as automation lanes.
pub struct KaossPadState {
    pub x: f32,
    pub y: f32,
    pub note_trigger_mode: bool,
    /// When true, dragging during play or record writes FxParam automation for the morph.
    pub write_xy_automation: bool,
    pub octave: u8,
    active_note: Option<u8>,
    trail: Vec<Pos2>,
    last_write_beat: f64,
}

impl Default for KaossPadState {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            note_trigger_mode: false,
            write_xy_automation: true,
            octave: 4,
            active_note: None,
            trail: Vec::with_capacity(24),
            last_write_beat: f64::NEG_INFINITY,
        }
    }
}

/// Apply Kaoss XY; returns `(fx_id, param_index)` pairs that changed (for automation write).
fn apply_kaoss_xy_to_track(
    track: &mut ferric_model::Track,
    x: f32,
    y: f32,
) -> Vec<(ferric_model::FxId, usize)> {
    let mut touched: Vec<(ferric_model::FxId, usize)> = Vec::new();
    let mut push = |fx: &mut FxSlot, idxs: &[usize]| {
        for &i in idxs {
            touched.push((fx.id, i));
        }
    };
    for fx in &mut track.fx_chain {
        match fx.kind {
            FxKind::PolySynth => {
                fx.set_param(2, 120.0 * (100.0_f32).powf(x));
                fx.set_param(3, 0.12 + y * 0.72);
                fx.set_param(1, y * 70.0);
                push(fx, &[2, 3, 1]);
            }
            FxKind::MonoBass => {
                fx.set_param(2, 40.0 * (200.0_f32).powf(x));
                fx.set_param(3, 0.08 + y * 0.82);
                fx.set_param(4, y * 95.0);
                push(fx, &[2, 3, 4]);
            }
            FxKind::PrismSynth | FxKind::HostDna => {
                fx.set_param(0, x * 100.0);
                fx.set_param(1, y * 85.0);
                fx.set_param(4, 180.0 * (90.0_f32).powf(x));
                fx.set_param(5, 0.08 + y * 0.7);
                push(fx, &[0, 1, 4, 5]);
            }
            FxKind::WavetableSynth => {
                fx.set_param(1, x * 100.0);
                fx.set_param(4, y * 80.0);
                fx.set_param(6, 120.0 * (150.0_f32).powf(x));
                fx.set_param(7, 0.08 + y * 0.72);
                push(fx, &[1, 4, 6, 7]);
            }
            FxKind::SamplePlayer => {
                fx.set_param(3, 180.0 * (85.0_f32).powf(x));
                fx.set_param(4, 0.10 + y * 0.72);
                push(fx, &[3, 4]);
            }
            FxKind::DrumKit => {
                fx.set_param(0, 35.0 + x * 65.0);
                fx.set_param(4, y * 90.0);
                push(fx, &[0, 4]);
            }
            FxKind::ParametricEq => {
                fx.set_param(6, 800.0 * (12.0_f32).powf(x));
                fx.set_param(7, -6.0 + y * 14.0);
                fx.set_param(8, 0.5 + y * 4.0);
                push(fx, &[6, 7, 8]);
            }
            FxKind::StereoDelay => {
                fx.set_param(2, y * 82.0);
                fx.set_param(4, 900.0 + x * 9000.0);
                push(fx, &[2, 4]);
            }
            FxKind::PlateHallReverb => {
                fx.set_param(1, 18.0 + y * 78.0);
                fx.set_param(2, x * 90.0);
                push(fx, &[1, 2]);
            }
            FxKind::TapeSaturator => {
                fx.set_param(0, x * 18.0);
                fx.set_param(1, y * 90.0);
                push(fx, &[0, 1]);
            }
            FxKind::AmpCabSim => {
                fx.set_param(0, x * 30.0);
                fx.set_param(5, y * 100.0);
                push(fx, &[0, 5]);
            }
            FxKind::Compressor => {
                fx.set_param(0, -42.0 + x * 36.0);
                fx.set_param(1, 1.5 + y * 12.0);
                push(fx, &[0, 1]);
            }
            FxKind::MultibandOtt => {
                fx.set_param(0, x * 100.0);
                fx.set_param(1, 10.0 + y * 240.0);
                push(fx, &[0, 1]);
            }
            FxKind::ExternalClap => {
                fx.set_param(0, x * 100.0);
                fx.set_param(1, y * 100.0);
                push(fx, &[0, 1]);
            }
            _ => {}
        }
    }

    // Audio / empty racks: insert a performable EQ so Kaoss always has an effect target.
    if touched.is_empty() {
        let mut eq = FxSlot::new(FxKind::ParametricEq);
        eq.set_param(6, 800.0 * (12.0_f32).powf(x));
        eq.set_param(7, -6.0 + y * 14.0);
        eq.set_param(8, 0.5 + y * 4.0);
        let id = eq.id;
        track.fx_chain.push(eq);
        touched.extend([(id, 6), (id, 7), (id, 8)]);
    }
    touched
}

fn write_kaoss_automation(
    track: &mut ferric_model::Track,
    touched: &[(ferric_model::FxId, usize)],
    playhead_beat: f64,
) {
    use ferric_model::{AutomationMode, AutomationTarget};
    if matches!(track.automation_mode, AutomationMode::Off | AutomationMode::Read) {
        track.automation_mode = AutomationMode::Write;
    }
    for &(fx_id, param_index) in touched {
        let Some(fx) = track.fx_chain.iter().find(|f| f.id == fx_id) else {
            continue;
        };
        let val = fx.param(param_index);
        let norm = fx.normalize_param(param_index, val);
        let lane_idx =
            track.ensure_automation_lane(AutomationTarget::FxParam { fx_id, param_index });
        track.automation_lanes[lane_idx].upsert_point(playhead_beat, norm);
    }
}

impl KaossPadState {
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        selected_track: Option<TrackId>,
        playhead_beat: f64,
        is_playing: bool,
        is_recording: bool,
        actions: &mut Vec<BrowserAction>,
    ) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Kaoss / XY Pad")
                    .strong()
                    .color(ACCENT_FERRIC),
            );
            ui.label(
                egui::RichText::new("Sculpts selected track instrument + FX")
                    .size(11.0)
                    .color(TEXT_DIM),
            );
            ui.checkbox(&mut self.write_xy_automation, "Write XY automation")
                .on_hover_text(
                    "While Play or Rec is on, pad moves are written as FxParam automation on the selected track.",
                );
            ui.checkbox(&mut self.note_trigger_mode, "Also trigger scale notes");
            ui.label(egui::RichText::new("Octave:").size(11.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.octave).range(2..=6));
            ui.separator();
            chrome_row_label(ui, "Key ->", ACCENT_TEAL);
            egui::ComboBox::from_id_salt("kaoss_root_key")
                .selected_text(NOTE_PITCH_CLASSES[(project.root_pitch_class as usize) % 12])
                .width(54.0)
                .show_ui(ui, |ui| {
                    for (i, &nm) in NOTE_PITCH_CLASSES.iter().enumerate() {
                        ui.selectable_value(&mut project.root_pitch_class, i as u8, nm);
                    }
                });
            chrome_row_label(ui, "Scale ->", ACCENT_AMBER);
            egui::ComboBox::from_id_salt("kaoss_scale_mode")
                .selected_text(project.musical_scale.label())
                .width(155.0)
                .show_ui(ui, |ui| {
                    for &sc in MusicalScale::ALL {
                        ui.selectable_value(&mut project.musical_scale, sc, sc.label());
                    }
                });
            if self.write_xy_automation && (is_playing || is_recording) {
                ui.label(
                    egui::RichText::new(if is_recording {
                        "● REC → automation"
                    } else {
                        "● PLAY → automation"
                    })
                    .size(11.0)
                    .strong()
                    .color(if is_recording {
                        ACCENT_AMBER
                    } else {
                        ACCENT_TEAL
                    }),
                );
            }
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

                if let Some(tid) = selected_track {
                    if let Some(trk) = project.find_track_mut(tid) {
                        let touched = apply_kaoss_xy_to_track(trk, self.x, self.y);
                        if !touched.is_empty() {
                            let should_write = self.write_xy_automation
                                && (is_playing || is_recording)
                                && (playhead_beat - self.last_write_beat).abs() >= 1.0 / 64.0;
                            if should_write {
                                write_kaoss_automation(trk, &touched, playhead_beat);
                                self.last_write_beat = playhead_beat;
                            }
                            actions.push(BrowserAction::ProjectMutated);
                        }
                    }
                }

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

        let track_hint = selected_track
            .and_then(|tid| project.find_track(tid))
            .map(|t| t.name.as_str())
            .unwrap_or("(select a track)");
        p.text(
            Pos2::new(rect.left() + 10.0, rect.bottom() - 10.0),
            Align2::LEFT_BOTTOM,
            format!(
                "X brightness/cutoff {:03.0}% · Y resonance/space {:03.0}%  →  {}",
                self.x * 100.0,
                self.y * 100.0,
                track_hint
            ),
            FontId::monospace(10.5),
            TEXT_MAIN,
        );
    }
}

/// Tap-tempo averager — tap along to a Line-In beat in 4 clicks.
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
