//! Dual programmable computer-keyboard MIDI pads.
//!
//! Pad A = letter piano row (works on every laptop).
//! Pad B = physical numpad when present, or the top number row on compact keyboards.

use egui::{Key, Ui};
use ferric_model::TrackId;
use std::collections::HashSet;

use crate::theme::{ACCENT_AMBER, ACCENT_FERRIC, ACCENT_TEAL, TEXT_DIM, TEXT_MAIN};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PadBLayout {
    /// Chromatic notes from base octave (good for second melody/bass).
    #[default]
    Chromatic,
    /// Fixed drum-style MIDI notes (C1 grid) for kits / one-shots.
    DrumGrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PadBSource {
    /// Prefer Win32 VK_NUMPAD* (true second MIDI surface).
    #[default]
    Numpad,
    /// Top-row Digits 1–0 (laptops without a numpad).
    NumberRow,
}

/// Two independently routable keyboard MIDI surfaces.
#[derive(Debug, Clone)]
pub struct KeyboardPadRig {
    pub enabled: bool,
    pub pad_b_source: PadBSource,
    pub pad_b_layout: PadBLayout,
    /// None → currently selected track.
    pub pad_a_track: Option<TrackId>,
    pub pad_b_track: Option<TrackId>,
    pub pad_a_octave: i8,
    pub pad_b_octave: i8,
    pub velocity: u8,
    held_a: HashSet<Key>,
    held_b_egui: HashSet<Key>,
    held_b_vk: HashSet<i32>,
}

impl Default for KeyboardPadRig {
    fn default() -> Self {
        Self {
            enabled: false,
            pad_b_source: PadBSource::Numpad,
            pad_b_layout: PadBLayout::Chromatic,
            pad_a_track: None,
            pad_b_track: None,
            pad_a_octave: 4,
            pad_b_octave: 3,
            velocity: 108,
            held_a: HashSet::new(),
            held_b_egui: HashSet::new(),
            held_b_vk: HashSet::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PadNoteEvent {
    pub target_track: Option<TrackId>,
    pub note: u8,
    pub velocity: u8,
    pub is_note_on: bool,
}

impl KeyboardPadRig {
    /// Letter-row chromatic map (Ableton/Reason style, white+black keys).
    pub const PAD_A_KEYS: &'static [(Key, u8)] = &[
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
        (Key::O, 13),
        (Key::L, 14),
        (Key::P, 15),
    ];

    /// Number-row Pad B (laptops): 1..0 → 10 pads.
    pub const PAD_B_NUMBER_ROW: &'static [(Key, u8)] = &[
        (Key::Num1, 0),
        (Key::Num2, 1),
        (Key::Num3, 2),
        (Key::Num4, 3),
        (Key::Num5, 4),
        (Key::Num6, 5),
        (Key::Num7, 6),
        (Key::Num8, 7),
        (Key::Num9, 8),
        (Key::Num0, 9),
    ];

    /// Win32 virtual-key codes for a full numpad surface.
    #[cfg(windows)]
    pub const PAD_B_NUMPAD_VK: &'static [(i32, u8)] = &[
        (0x67, 0), // VK_NUMPAD7
        (0x68, 1), // VK_NUMPAD8
        (0x69, 2), // VK_NUMPAD9
        (0x6F, 3), // VK_DIVIDE
        (0x64, 4), // VK_NUMPAD4
        (0x65, 5), // VK_NUMPAD5
        (0x66, 6), // VK_NUMPAD6
        (0x6A, 7), // VK_MULTIPLY
        (0x61, 8), // VK_NUMPAD1
        (0x62, 9), // VK_NUMPAD2
        (0x63, 10), // VK_NUMPAD3
        (0x6D, 11), // VK_SUBTRACT
        (0x60, 12), // VK_NUMPAD0
        (0x6E, 13), // VK_DECIMAL
        (0x6B, 14), // VK_ADD
    ];

    pub fn panic_release_all(&mut self) -> Vec<PadNoteEvent> {
        let mut out = Vec::new();
        let vel = self.velocity;
        let a_track = self.pad_a_track;
        let b_track = self.pad_b_track;
        for k in self.held_a.drain() {
            if let Some(note) = Self::pad_a_note(k, self.pad_a_octave) {
                out.push(PadNoteEvent {
                    target_track: a_track,
                    note,
                    velocity: 0,
                    is_note_on: false,
                });
            }
        }
        let b_layout = self.pad_b_layout;
        let b_oct = self.pad_b_octave;
        for k in self.held_b_egui.drain() {
            if let Some(semi) = Self::PAD_B_NUMBER_ROW.iter().find(|(kk, _)| *kk == k).map(|(_, s)| *s)
            {
                let note = Self::pad_b_note_static(b_layout, b_oct, semi);
                out.push(PadNoteEvent {
                    target_track: b_track,
                    note,
                    velocity: 0,
                    is_note_on: false,
                });
            }
        }
        let _ = vel;
        self.held_b_vk.clear();
        out
    }

    fn pad_a_note(key: Key, octave: i8) -> Option<u8> {
        let semi = Self::PAD_A_KEYS.iter().find(|(k, _)| *k == key)?.1;
        let base = ((octave as i32 + 1) * 12).clamp(12, 96) as u8;
        Some((base + semi).min(127))
    }

    fn pad_b_note(&self, pad_index: u8) -> u8 {
        Self::pad_b_note_static(self.pad_b_layout, self.pad_b_octave, pad_index)
    }

    fn pad_b_note_static(layout: PadBLayout, octave: i8, pad_index: u8) -> u8 {
        match layout {
            PadBLayout::Chromatic => {
                let base = ((octave as i32 + 1) * 12).clamp(12, 96) as u8;
                (base + pad_index).min(127)
            }
            PadBLayout::DrumGrid => (36u8.saturating_add(pad_index)).min(127),
        }
    }

    fn resolve_a_track(&self, selected: Option<TrackId>) -> Option<TrackId> {
        self.pad_a_track.or(selected)
    }

    fn resolve_b_track(&self, selected: Option<TrackId>) -> Option<TrackId> {
        self.pad_b_track.or(selected)
    }

    /// Poll egui + (on Windows) raw numpad VKs; emit note on/off events.
    pub fn poll(&mut self, ctx: &egui::Context, selected_track: Option<TrackId>) -> Vec<PadNoteEvent> {
        if !self.enabled {
            return Vec::new();
        }
        // Don't steal keys while typing into text fields.
        if ctx.wants_keyboard_input() {
            return Vec::new();
        }

        let mut events = Vec::new();
        let a_track = self.resolve_a_track(selected_track);
        let b_track = self.resolve_b_track(selected_track);
        let vel = self.velocity;

        // Octave shifts for Pad A: Z / X (same as classic musical typing).
        if ctx.input(|i| i.key_pressed(Key::Z)) {
            self.pad_a_octave = (self.pad_a_octave - 1).max(1);
        }
        if ctx.input(|i| i.key_pressed(Key::X)) {
            self.pad_a_octave = (self.pad_a_octave + 1).min(7);
        }
        // Pad B octave: , / .
        if ctx.input(|i| i.key_pressed(Key::Comma)) {
            self.pad_b_octave = (self.pad_b_octave - 1).max(1);
        }
        if ctx.input(|i| i.key_pressed(Key::Period)) {
            self.pad_b_octave = (self.pad_b_octave + 1).min(7);
        }

        for &(k, _) in Self::PAD_A_KEYS {
            let down = ctx.input(|i| i.key_down(k));
            let was = self.held_a.contains(&k);
            if down && !was {
                self.held_a.insert(k);
                if let Some(note) = Self::pad_a_note(k, self.pad_a_octave) {
                    events.push(PadNoteEvent {
                        target_track: a_track,
                        note,
                        velocity: vel,
                        is_note_on: true,
                    });
                }
            } else if !down && was {
                self.held_a.remove(&k);
                if let Some(note) = Self::pad_a_note(k, self.pad_a_octave) {
                    events.push(PadNoteEvent {
                        target_track: a_track,
                        note,
                        velocity: 0,
                        is_note_on: false,
                    });
                }
            }
        }

        match self.pad_b_source {
            PadBSource::NumberRow => {
                for &(k, idx) in Self::PAD_B_NUMBER_ROW {
                    let down = ctx.input(|i| i.key_down(k));
                    let was = self.held_b_egui.contains(&k);
                    let note = self.pad_b_note(idx);
                    if down && !was {
                        self.held_b_egui.insert(k);
                        events.push(PadNoteEvent {
                            target_track: b_track,
                            note,
                            velocity: vel,
                            is_note_on: true,
                        });
                    } else if !down && was {
                        self.held_b_egui.remove(&k);
                        events.push(PadNoteEvent {
                            target_track: b_track,
                            note,
                            velocity: 0,
                            is_note_on: false,
                        });
                    }
                }
            }
            PadBSource::Numpad => {
                #[cfg(windows)]
                {
                    for &(vk, idx) in Self::PAD_B_NUMPAD_VK {
                        let down = raw_vk_down(vk);
                        let was = self.held_b_vk.contains(&vk);
                        let note = self.pad_b_note(idx);
                        if down && !was {
                            self.held_b_vk.insert(vk);
                            events.push(PadNoteEvent {
                                target_track: b_track,
                                note,
                                velocity: vel,
                                is_note_on: true,
                            });
                        } else if !down && was {
                            self.held_b_vk.remove(&vk);
                            events.push(PadNoteEvent {
                                target_track: b_track,
                                note,
                                velocity: 0,
                                is_note_on: false,
                            });
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    // Non-Windows: fall back to number row semantics for Pad B.
                    for &(k, idx) in Self::PAD_B_NUMBER_ROW {
                        let down = ctx.input(|i| i.key_down(k));
                        let was = self.held_b_egui.contains(&k);
                        let note = self.pad_b_note(idx);
                        if down && !was {
                            self.held_b_egui.insert(k);
                            events.push(PadNoteEvent {
                                target_track: b_track,
                                note,
                                velocity: vel,
                                is_note_on: true,
                            });
                        } else if !down && was {
                            self.held_b_egui.remove(&k);
                            events.push(PadNoteEvent {
                                target_track: b_track,
                                note,
                                velocity: 0,
                                is_note_on: false,
                            });
                        }
                    }
                }
            }
        }

        events
    }

    pub fn render_panel(
        &mut self,
        ui: &mut Ui,
        tracks: &[(TrackId, String)],
        selected: Option<TrackId>,
    ) {
        ui.label(
            egui::RichText::new("⌨️ Dual Keyboard MIDI Pads")
                .size(12.0)
                .strong()
                .color(ACCENT_FERRIC),
        );
        ui.label(
            egui::RichText::new(
                "Pad A = letter keys (piano). Pad B = numpad, or number row on laptops without one. Each pad can target a different track/instrument.",
            )
            .size(10.5)
            .color(TEXT_DIM),
        );
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.enabled, "Enable keyboard pads");
            ui.add(egui::Slider::new(&mut self.velocity, 1..=127).text("Vel"));
        });

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Pad B source").color(TEXT_MAIN));
            ui.selectable_value(&mut self.pad_b_source, PadBSource::Numpad, "Numpad");
            ui.selectable_value(&mut self.pad_b_source, PadBSource::NumberRow, "Number row");
        });
        ui.horizontal(|ui| {
            ui.label("Pad B layout");
            ui.selectable_value(&mut self.pad_b_layout, PadBLayout::Chromatic, "Chromatic");
            ui.selectable_value(&mut self.pad_b_layout, PadBLayout::DrumGrid, "Drum grid");
        });

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!(
                "Pad A octave C{}  (Z/X)   •   Pad B octave C{}  (, / .)",
                self.pad_a_octave, self.pad_b_octave
            ))
            .size(11.0)
            .color(ACCENT_TEAL),
        );

        track_picker(ui, "Pad A ->", &mut self.pad_a_track, tracks, selected, ACCENT_FERRIC);
        track_picker(ui, "Pad B ->", &mut self.pad_b_track, tracks, selected, ACCENT_AMBER);

        ui.label(
            egui::RichText::new("Tip: leave a pad on Selected track to jam the focused instrument; assign Pad B to drums for split-hand laptop performance.")
                .size(10.0)
                .color(TEXT_DIM),
        );
    }
}

fn track_picker(
    ui: &mut Ui,
    label: &str,
    slot: &mut Option<TrackId>,
    tracks: &[(TrackId, String)],
    selected: Option<TrackId>,
    color: egui::Color32,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(color));
        egui::ComboBox::from_id_salt(label)
            .selected_text(match *slot {
                None => "Selected track".to_string(),
                Some(id) => tracks
                    .iter()
                    .find(|(t, _)| *t == id)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| "Missing track".into()),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(slot, None, "Selected track");
                for (id, name) in tracks {
                    ui.selectable_value(slot, Some(*id), name);
                }
            });
        if ui.small_button("Use selected").clicked() {
            *slot = selected;
        }
    });
}

#[cfg(windows)]
fn raw_vk_down(vk: i32) -> bool {
    // Prefer GetAsyncKeyState so numpad is distinct from the top digit row (egui merges them).
    unsafe {
        let state = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk);
        (state as u16 & 0x8000) != 0
    }
}
