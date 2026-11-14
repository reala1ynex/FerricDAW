use crate::theme::{ACCENT_FERRIC, ACCENT_TEAL, BG_ELEVATED, BORDER_SUBTLE, TEXT_DIM, TEXT_MAIN};
use egui::{Context, Key, Stroke, Window};
use ferric_model::FxKind;

#[derive(Debug, Clone)]
pub enum PaletteCommand {
    TogglePlay,
    ToggleRecord,
    CaptureRetroactive,
    SetupAkaiDs10Rig,
    ToggleFreezeSelectedTrack,
    AddUniversalTrack,
    AddSynthTrack,
    AddSamplerTrack,
    AddVocalPitchTrack,
    AddDrumTrack,
    AddGuitarAmpTrack,
    InsertFxOnSelected(FxKind),
    AddLfoModulator,
    SplitSelectedClipAtPlayhead,
    QuantizeSelectedClip,
    ToggleLoop,
    ToggleMetronome,
    OpenBounceModal,
    SaveProject,
    LoadDemoProject,
    Undo,
    Redo,
}

struct ActionItem {
    title: &'static str,
    shortcut: &'static str,
    category: &'static str,
    cmd: PaletteCommand,
}

pub struct CommandPalette {
    pub open: bool,
    pub query: String,
    pub selected_idx: usize,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected_idx: 0,
        }
    }
}

impl CommandPalette {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.query.clear();
            self.selected_idx = 0;
        }
    }

    pub fn show(&mut self, ctx: &Context) -> Option<PaletteCommand> {
        if !self.open {
            return None;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
            return None;
        }

        let catalog = Self::build_catalog();
        let q = self.query.trim().to_lowercase();
        let filtered: Vec<&ActionItem> = catalog
            .iter()
            .filter(|item| {
                q.is_empty()
                    || item.title.to_lowercase().contains(&q)
                    || item.category.to_lowercase().contains(&q)
            })
            .collect();

        if !filtered.is_empty() {
            if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
                self.selected_idx = (self.selected_idx + 1) % filtered.len();
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
                self.selected_idx = (self.selected_idx + filtered.len() - 1) % filtered.len();
            }
        } else {
            self.selected_idx = 0;
        }

        let mut triggered = None;

        Window::new("⌘ FerricDAW Action & FX Palette")
            .collapsible(false)
            .resizable(false)
            .title_bar(true)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 58.0])
            .fixed_size([520.0, 340.0])
            .frame(
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.5_f32, ACCENT_FERRIC))
                    .corner_radius(8)
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Type any action, track preset, or studio FX (e.g. 'Amp', 'Retroactive', 'Split', 'Bounce')...")
                        .desired_width(f32::INFINITY),
                );
                edit.request_focus();

                if ui.input(|i| i.key_pressed(Key::Enter)) && !filtered.is_empty() {
                    let idx = self.selected_idx.min(filtered.len() - 1);
                    triggered = Some(filtered[idx].cmd.clone());
                }

                ui.separator();

                egui::ScrollArea::vertical()
                    .max_height(265.0)
                    .show(ui, |ui| {
                        for (i, item) in filtered.iter().enumerate() {
                            let is_sel = i == self.selected_idx;
                            let bg = if is_sel {
                                ACCENT_FERRIC.gamma_multiply(0.28)
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            egui::Frame::new()
                                .fill(bg)
                                .stroke(if is_sel {
                                    Stroke::new(1.0_f32, ACCENT_FERRIC)
                                } else {
                                    Stroke::new(0.0_f32, BORDER_SUBTLE)
                                })
                                .corner_radius(4)
                                .inner_margin(egui::Margin::symmetric(8, 5))
                                .show(ui, |ui| {
                                    let resp = ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("[{}]", item.category))
                                                .size(10.0)
                                                .color(ACCENT_TEAL)
                                                .monospace(),
                                        );
                                        ui.label(
                                            egui::RichText::new(item.title)
                                                .size(12.0)
                                                .color(TEXT_MAIN),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if !item.shortcut.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new(item.shortcut)
                                                            .size(10.5)
                                                            .monospace()
                                                            .color(TEXT_DIM),
                                                    );
                                                }
                                            },
                                        );
                                    });
                                    if resp.response.interact(egui::Sense::click()).clicked() {
                                        triggered = Some(item.cmd.clone());
                                    }
                                });
                        }
                    });
            });

        if triggered.is_some() {
            self.open = false;
        }
        triggered
    }

    fn build_catalog() -> Vec<ActionItem> {
        let mut items = vec![
            ActionItem {
                title: "Spawn Akai LPK25 + Nintendo DS KORG DS-10 Hybrid Rig",
                shortcut: "",
                category: "HARDWARE",
                cmd: PaletteCommand::SetupAkaiDs10Rig,
            },
            ActionItem {
                title: "Freeze / Unfreeze Selected Track (Zero-CPU 32-bit Stem)",
                shortcut: "Ctrl+F",
                category: "CPU",
                cmd: PaletteCommand::ToggleFreezeSelectedTrack,
            },
            ActionItem {
                title: "Capture Retroactive Audio/MIDI Buffer (Last 15s)",
                shortcut: "Shift+R",
                category: "CAPTURE",
                cmd: PaletteCommand::CaptureRetroactive,
            },
            ActionItem {
                title: "Add Vocal Auto-Pitch & Scale Studio Track",
                shortcut: "",
                category: "TRACK",
                cmd: PaletteCommand::AddVocalPitchTrack,
            },
            ActionItem {
                title: "Add Chromatic & Transient-Slice Sampler Track",
                shortcut: "",
                category: "TRACK",
                cmd: PaletteCommand::AddSamplerTrack,
            },
            ActionItem {
                title: "Add Guitar Recording Track (Tuner + Tube Amp & 2x12 Cab)",
                shortcut: "",
                category: "TRACK",
                cmd: PaletteCommand::AddGuitarAmpTrack,
            },
            ActionItem {
                title: "Add PolySynth Track (16-Voice VA/FM + Stereo Delay)",
                shortcut: "",
                category: "TRACK",
                cmd: PaletteCommand::AddSynthTrack,
            },
            ActionItem {
                title: "Add 8-Pad Studio Drum Kit Track",
                shortcut: "",
                category: "TRACK",
                cmd: PaletteCommand::AddDrumTrack,
            },
            ActionItem {
                title: "Add Empty Universal Audio/MIDI/Folder Track",
                shortcut: "Ctrl+T",
                category: "TRACK",
                cmd: PaletteCommand::AddUniversalTrack,
            },
            ActionItem {
                title: "Split Selected Clip at Playhead",
                shortcut: "S",
                category: "EDIT",
                cmd: PaletteCommand::SplitSelectedClipAtPlayhead,
            },
            ActionItem {
                title: "Quantize Selected MIDI Clip to 1/16 Grid",
                shortcut: "Q",
                category: "MIDI",
                cmd: PaletteCommand::QuantizeSelectedClip,
            },
            ActionItem {
                title: "Add Tempo-Synced LFO Modulator to Selected Track",
                shortcut: "",
                category: "MOD",
                cmd: PaletteCommand::AddLfoModulator,
            },
            ActionItem {
                title: "Bounce / Export Master WAV or Multi-Track Stems...",
                shortcut: "Ctrl+B",
                category: "EXPORT",
                cmd: PaletteCommand::OpenBounceModal,
            },
            ActionItem {
                title: "Play / Stop Transport",
                shortcut: "Space",
                category: "TRANSPORT",
                cmd: PaletteCommand::TogglePlay,
            },
            ActionItem {
                title: "Toggle Record",
                shortcut: "R",
                category: "TRANSPORT",
                cmd: PaletteCommand::ToggleRecord,
            },
            ActionItem {
                title: "Toggle Loop Playback",
                shortcut: "L",
                category: "TRANSPORT",
                cmd: PaletteCommand::ToggleLoop,
            },
            ActionItem {
                title: "Toggle Metronome Click",
                shortcut: "C",
                category: "TRANSPORT",
                cmd: PaletteCommand::ToggleMetronome,
            },
            ActionItem {
                title: "Save Project (.ferric Git-Friendly RON)",
                shortcut: "Ctrl+S",
                category: "PROJECT",
                cmd: PaletteCommand::SaveProject,
            },
            ActionItem {
                title: "Reload 'Neon Oxide' Factory Demo Session",
                shortcut: "",
                category: "PROJECT",
                cmd: PaletteCommand::LoadDemoProject,
            },
            ActionItem {
                title: "Undo Last Action",
                shortcut: "Ctrl+Z",
                category: "HISTORY",
                cmd: PaletteCommand::Undo,
            },
            ActionItem {
                title: "Redo Action",
                shortcut: "Ctrl+Y",
                category: "HISTORY",
                cmd: PaletteCommand::Redo,
            },
        ];

        for &kind in FxKind::ALL {
            if kind != FxKind::ExternalClap {
                items.push(ActionItem {
                    title: kind.display_name(),
                    shortcut: "",
                    category: "INSERT FX",
                    cmd: PaletteCommand::InsertFxOnSelected(kind),
                });
            }
        }
        items
    }
}
