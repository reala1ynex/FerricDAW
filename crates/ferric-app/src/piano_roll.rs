use crate::theme::{
    StudioUiExt,
    chrome_card, chrome_row_label, chrome_section_title, pill_toggle, studio_knob, track_color32,
    ACCENT_AMBER, ACCENT_FERRIC, ACCENT_RED, ACCENT_TEAL, ACCENT_PURPLE, BG_DEEP, BG_ELEVATED, BORDER_SUBTLE,
    BUTTON_RADIUS, BUTTON_SURFACE,
    TEXT_DIM, TEXT_MAIN,
};
use egui::{Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use ferric_dsp::{
    analyze_audio_pitch_blobs, ParametricEqProcessor, WavetableSynthProcessor, SPECTRUM_BANDS,
};
use ferric_engine::{import_audio_file, EngineTelemetry};
use ferric_model::{
    export_smf_clip, AutomationTarget, ChordStamp, ClipId, FxKind, FxSlot, LfoShape, MidiCcPoint,
    MidiCcTarget, MidiNote, ModRouting, ModTarget, ModulatorKind, ModulatorSlot, MusicalScale,
    Project, TakeContent, TimeStretchMode, TrackId, NOTE_PITCH_CLASSES,
};
use uuid::Uuid;

pub enum DockAction {
    ProjectMutated(&'static str),
    /// Discrete undoable mix ops — main snapshots then applies.
    RecallMix(usize),
    StoreMix(usize),
    AddVcaGroup,
    HostDnaReseed(ferric_model::FxId),
    HostDnaRandomize(ferric_model::FxId),
    HostDnaSibling(ferric_model::FxId, u8),
    HostDnaPaste(ferric_model::FxId, u64),
    AuditionMidi {
        track_id: TrackId,
        note: u8,
        is_note_on: bool,
    },
    ArmMidiLearn {
        track_id: TrackId,
        target: AutomationTarget,
        label: String,
    },
    ReloadSamplerFx(FxSlot),
    ToggleExternalPluginEditor(ferric_model::FxId),
    CreateMidiTrackFromAudioBlobs {
        source_name: String,
        start_beat: f64,
        length_beats: f64,
        notes: Vec<MidiNote>,
    },
    /// Surface success/failure to the top status banner (e.g. MIDI export).
    StatusMessage(String),
}

pub struct PianoRollState {
    pub zoom_x: f32,
    pub lowest_pitch: u8,
    pub visible_semitones: u8,
    pub default_note_len: f64,
    pub scale_lock: bool,
    pub drum_step_view: bool,
    pub drum_bar_index: usize,
    pub audio_pitch_blob_view: bool,
    pub chord_stamp: ChordStamp,
    pub active_cc_lane: Option<MidiCcTarget>,
    /// Selected MIDI note tile IDs (Backspace / Delete removes these).
    pub selected_notes: Vec<Uuid>,
    h_scroll_px: f32,
    drag_note: Option<(Uuid, bool)>,
    drag_pitch_accum: f32,
    drag_warp: Option<Uuid>,
    drag_blob: Option<Uuid>,
    auditioning_pitch: Option<u8>,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            zoom_x: 56.0,
            lowest_pitch: 36,
            visible_semitones: 36,
            default_note_len: 0.5,
            scale_lock: true,
            drum_step_view: false,
            drum_bar_index: 0,
            audio_pitch_blob_view: false,
            chord_stamp: ChordStamp::SingleNote,
            active_cc_lane: None,
            selected_notes: Vec::new(),
            h_scroll_px: 0.0,
            drag_note: None,
            drag_pitch_accum: 0.0,
            drag_warp: None,
            drag_blob: None,
            auditioning_pitch: None,
        }
    }
}

impl PianoRollState {
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        selected_clip: Option<(TrackId, ClipId)>,
        telem: &EngineTelemetry,
        actions: &mut Vec<DockAction>,
    ) {
        let Some((track_id, clip_id)) = selected_clip else {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Double-click a clip on the timeline to edit it here.")
                            .size(14.0)
                            .strong()
                            .color(TEXT_MAIN),
                    );
                    ui.label(
                        egui::RichText::new(
                            "MIDI clips → piano roll · Audio clips → warp / fades / clip FX",
                        )
                        .size(12.0)
                        .color(TEXT_DIM),
                    );
                });
            });
            return;
        };

        let is_midi = project
            .find_track(track_id)
            .and_then(|t| t.find_clip(clip_id))
            .map(|c| c.is_midi())
            .unwrap_or(false);

        if is_midi {
            self.render_midi_editor(ui, project, track_id, clip_id, telem, actions);
        } else {
            self.render_audio_clip_inspector(ui, project, track_id, clip_id, telem, actions);
        }
    }

    fn render_midi_editor(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        track_id: TrackId,
        clip_id: ClipId,
        telem: &EngineTelemetry,
        actions: &mut Vec<DockAction>,
    ) {
        let t_col = project
            .find_track(track_id)
            .map(|t| track_color32(t.color))
            .unwrap_or(ACCENT_FERRIC);

        let root_pc = project.root_pitch_class;
        let scale = project.musical_scale;

        // MIDI toolbar — Pad A/B chrome family, horizontally scrollable
        egui::ScrollArea::horizontal()
            .id_salt("piano_roll_toolbar_scroll")
            .show(ui, |ui| {
                chrome_card(ui, BORDER_SUBTLE, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    if let Some(clip) = project
                        .find_track_mut(track_id)
                        .and_then(|t| t.find_clip_mut(clip_id))
                    {
                        chrome_row_label(ui, "Clip ->", ACCENT_FERRIC);
                        ui.add(
                            egui::TextEdit::singleline(&mut clip.name)
                                .desired_width(100.0)
                                .hint_text("Clip name"),
                        );
                    }

                    if pill_toggle(ui, "16-Step", self.drum_step_view, ACCENT_AMBER)
                        .on_hover_text("16-step drum pattern grid")
                        .clicked()
                    {
                        self.drum_step_view = !self.drum_step_view;
                    }

                    if pill_toggle(ui, "Scale Lock", self.scale_lock, ACCENT_TEAL)
                        .on_hover_text("Snap drawn notes to session key & scale")
                        .clicked()
                    {
                        self.scale_lock = !self.scale_lock;
                    }
                    chrome_row_label(ui, "Key ->", ACCENT_TEAL);
                    egui::ComboBox::from_id_salt("pr_root")
                        .selected_text(
                            NOTE_PITCH_CLASSES[(project.root_pitch_class as usize) % 12],
                        )
                        .width(44.0)
                        .show_ui(ui, |ui| {
                            for (i, &nm) in NOTE_PITCH_CLASSES.iter().enumerate() {
                                ui.selectable_value(&mut project.root_pitch_class, i as u8, nm);
                            }
                        });
                    chrome_row_label(ui, "Scale ->", ACCENT_AMBER);
                    egui::ComboBox::from_id_salt("pr_scale")
                        .selected_text(project.musical_scale.label())
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for &sc in MusicalScale::ALL {
                                ui.selectable_value(&mut project.musical_scale, sc, sc.label());
                            }
                        });

                    chrome_row_label(ui, "Stamp ->", TEXT_DIM);
                    egui::ComboBox::from_id_salt("pr_chord_stamp")
                        .selected_text(self.chord_stamp.label())
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for &cs in ChordStamp::ALL {
                                ui.selectable_value(&mut self.chord_stamp, cs, cs.label());
                            }
                        })
                        .response
                        .on_hover_text("Click empty grid to stamp this chord/interval");

                    if ui
                        .chrome_small_button("Q 1/16")
                        .on_hover_text("Quantize notes to 16ths")
                        .clicked()
                    {
                        project.quantize_midi_clip(track_id, clip_id, 0.25);
                        actions.push(DockAction::ProjectMutated("Quantize 1/16"));
                    }
                    if ui
                        .chrome_small_button("Humanize")
                        .on_hover_text("Subtle timing & velocity groove")
                        .clicked()
                    {
                        project.humanize_midi_clip(track_id, clip_id);
                        actions.push(DockAction::ProjectMutated("Humanize MIDI"));
                    }

                    let lane_lbl = self
                        .active_cc_lane
                        .map(|c| c.label())
                        .unwrap_or("Velocity");
                    egui::ComboBox::from_id_salt("pr_cc_lane_sel")
                        .selected_text(lane_lbl)
                        .width(96.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.active_cc_lane, None, "Velocity");
                            for &tgt in MidiCcTarget::ALL {
                                ui.selectable_value(
                                    &mut self.active_cc_lane,
                                    Some(tgt),
                                    tgt.label(),
                                );
                            }
                        });

                    if ui
                        .chrome_small_button("Export .mid")
                        .on_hover_text("Export clip as Standard MIDI File")
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Standard MIDI", &["mid"])
                            .set_file_name("clip.mid")
                            .save_file()
                        {
                            if let Some(clip) = project
                                .find_track(track_id)
                                .and_then(|t| t.find_clip(clip_id))
                            {
                                if let Some(TakeContent::Midi { notes, cc_points }) =
                                    clip.takes.get(clip.active_take).map(|t| &t.content)
                                {
                                    match export_smf_clip(&path, project.bpm, notes, cc_points) {
                                        Ok(()) => actions.push(DockAction::StatusMessage(format!(
                                            "✓ Exported MIDI → {}",
                                            path.display()
                                        ))),
                                        Err(e) => actions.push(DockAction::StatusMessage(format!(
                                            "MIDI export failed: {e}"
                                        ))),
                                    }
                                } else {
                                    actions.push(DockAction::StatusMessage(
                                        "MIDI export failed: active take is not MIDI.".into(),
                                    ));
                                }
                            }
                        }
                    }

                    if ui
                        .chrome_small_button("▼Oct")
                        .on_hover_text("Scroll piano roll down one octave")
                        .clicked()
                    {
                        self.lowest_pitch = self.lowest_pitch.saturating_sub(12).max(12);
                    }
                    if ui
                        .chrome_small_button("▲Oct")
                        .on_hover_text("Scroll piano roll up one octave")
                        .clicked()
                    {
                        self.lowest_pitch = (self.lowest_pitch + 12).min(84);
                    }

                    if !self.selected_notes.is_empty() {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} selected · Backspace deletes · drag to move",
                                self.selected_notes.len()
                            ))
                            .size(10.5)
                            .color(TEXT_DIM),
                        );
                    }
                });
                });
            });

        let Some(clip_start) = project
            .find_track(track_id)
            .and_then(|t| t.find_clip(clip_id))
            .map(|c| c.start_beat)
        else {
            return;
        };
        let clip_len = project
            .find_track(track_id)
            .and_then(|t| t.find_clip(clip_id))
            .map(|c| c.loop_length_beats.max(c.length_beats))
            .unwrap_or(16.0);

        if self.drum_step_view {
            self.render_drum_step_sequencer(
                ui, project, track_id, clip_id, clip_start, clip_len, t_col, telem, actions,
            );
            return;
        }

        let mut zoom_scroll_adjust = 0.0_f32;
        let scroll_out = egui::ScrollArea::horizontal()
            .id_salt("piano_roll_hscroll")
            .horizontal_scroll_offset(self.h_scroll_px)
            .show(ui, |ui| {
                let keys_w = 48.0_f32;
                let total_w = keys_w + (clip_len as f32 * self.zoom_x).max(ui.available_width());
                let avail_h = ui.available_height().max(150.0);
                let vel_h = 40.0_f32;
                let grid_h = (avail_h - vel_h - 4.0).max(105.0);
                let rows = self.visible_semitones.max(12) as usize;
                let row_h = grid_h / rows as f32;

                let (canvas, _) =
                    ui.allocate_exact_size(Vec2::new(total_w, grid_h + vel_h), Sense::hover());
                let keys_rect = Rect::from_min_size(canvas.min, Vec2::new(keys_w, grid_h));
                let grid_rect = Rect::from_min_size(
                    Pos2::new(canvas.left() + keys_w, canvas.top()),
                    Vec2::new(total_w - keys_w, grid_h),
                );
                let vel_rect = Rect::from_min_size(
                    Pos2::new(canvas.left() + keys_w, grid_rect.bottom() + 2.0),
                    Vec2::new(total_w - keys_w, vel_h - 2.0),
                );

                // Scroll-wheel zoom at cursor (beat stays under pointer); Shift+scroll pans pitch rows.
                if ui.rect_contains_pointer(canvas) {
                    let (shift, scroll, hover) = ui.input(|i| {
                        (
                            i.modifiers.shift,
                            i.smooth_scroll_delta,
                            i.pointer.hover_pos(),
                        )
                    });
                    if scroll.y.abs() > 0.1 {
                        if shift {
                            let delta_rows = (-scroll.y * 0.08).round() as i32;
                            if delta_rows != 0 {
                                let max_low =
                                    (127i32 - self.visible_semitones as i32).max(0);
                                self.lowest_pitch = (self.lowest_pitch as i32 + delta_rows)
                                    .clamp(0, max_low) as u8;
                            }
                        } else if let Some(pos) = hover {
                            let old_zoom = self.zoom_x;
                            let new_zoom =
                                (old_zoom * (1.0 + scroll.y * 0.0045)).clamp(12.0, 280.0);
                            if (new_zoom - old_zoom).abs() > 0.01 {
                                let beat_under =
                                    ((pos.x - grid_rect.left()) / old_zoom).max(0.0);
                                zoom_scroll_adjust = beat_under * (new_zoom - old_zoom);
                                self.zoom_x = new_zoom;
                            }
                        }
                    }
                }

                // Piano Keyboard Audition Strip + Scale Highlighting!
                let keys_resp = ui.interact(
                    keys_rect,
                    ui.id().with("piano_keys_strip"),
                    Sense::click_and_drag(),
                );
                for r in 0..rows {
                    let pitch = self.lowest_pitch + (rows - 1 - r) as u8;
                    let y0 = keys_rect.top() + r as f32 * row_h;
                    let kr =
                        Rect::from_min_size(Pos2::new(keys_rect.left(), y0), Vec2::new(keys_w, row_h));
                    let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                    let in_scale = scale.contains_pitch(root_pc, pitch);
                    let is_root = pitch % 12 == root_pc % 12;

                    let key_col = if self.auditioning_pitch == Some(pitch) {
                        t_col
                    } else if is_black {
                        Color32::from_rgb(24, 26, 32)
                    } else {
                        Color32::from_rgb(205, 210, 220)
                    };
                    ui.painter().rect_filled(kr.shrink(0.4), 1, key_col);
                    if pitch % 12 == 0 || is_root {
                        let oct = (pitch as i32 / 12) - 1;
                        let nm = NOTE_PITCH_CLASSES[(pitch as usize) % 12];
                        ui.painter().text(
                            Pos2::new(kr.right() - 3.0, kr.center().y),
                            Align2::RIGHT_CENTER,
                            format!("{}{}", nm, oct),
                            FontId::monospace(8.5),
                            if is_root { ACCENT_FERRIC } else { Color32::from_rgb(30, 32, 40) },
                        );
                    }

                    let gr = Rect::from_min_size(
                        Pos2::new(grid_rect.left(), y0),
                        Vec2::new(grid_rect.width(), row_h),
                    );
                    let row_bg = if self.scale_lock && is_root {
                        Color32::from_rgb(32, 28, 38)
                    } else if self.scale_lock && in_scale {
                        Color32::from_rgb(23, 27, 36)
                    } else if is_black {
                        Color32::from_rgb(15, 16, 21)
                    } else {
                        Color32::from_rgb(19, 21, 27)
                    };
                    ui.painter().rect_filled(gr, 0, row_bg);
                    ui.painter().line_segment(
                        [gr.left_bottom(), gr.right_bottom()],
                        Stroke::new(0.6_f32, Color32::from_rgb(30, 33, 42)),
                    );
                }

                if keys_resp.is_pointer_button_down_on() {
                    if let Some(pos) = keys_resp.interact_pointer_pos() {
                        let r = (((pos.y - keys_rect.top()) / row_h).floor() as usize).min(rows - 1);
                        let pitch = self.lowest_pitch + (rows - 1 - r) as u8;
                        if self.auditioning_pitch != Some(pitch) {
                            if let Some(prev) = self.auditioning_pitch.take() {
                                actions.push(DockAction::AuditionMidi {
                                    track_id,
                                    note: prev,
                                    is_note_on: false,
                                });
                            }
                            self.auditioning_pitch = Some(pitch);
                            actions.push(DockAction::AuditionMidi {
                                track_id,
                                note: pitch,
                                is_note_on: true,
                            });
                        }
                    }
                } else if let Some(prev) = self.auditioning_pitch.take() {
                    actions.push(DockAction::AuditionMidi {
                        track_id,
                        note: prev,
                        is_note_on: false,
                    });
                }

                let total_sixteenths = (clip_len * 4.0).ceil() as usize;
                for s in 0..=total_sixteenths {
                    let b = s as f64 * 0.25;
                    let x = grid_rect.left() + b as f32 * self.zoom_x;
                    if x > grid_rect.right() {
                        break;
                    }
                    let col = if s % 16 == 0 {
                        Color32::from_rgb(66, 72, 90)
                    } else if s % 4 == 0 {
                        Color32::from_rgb(42, 46, 58)
                    } else {
                        Color32::from_rgb(28, 31, 40)
                    };
                    ui.painter().line_segment(
                        [Pos2::new(x, grid_rect.top()), Pos2::new(x, grid_rect.bottom())],
                        Stroke::new(1.0_f32, col),
                    );
                }

                ui.painter().rect_filled(vel_rect, 2, BG_DEEP);

                let Some(clip_mut) = project
                    .find_track_mut(track_id)
                    .and_then(|t| t.find_clip_mut(clip_id))
                else {
                    return;
                };
                let active_take = clip_mut.active_take.min(clip_mut.takes.len().saturating_sub(1));
                let Some(TakeContent::Midi { notes, cc_points }) =
                    clip_mut.takes.get_mut(active_take).map(|t| &mut t.content)
                else {
                    return;
                };

                let mut delete_note_ids: Vec<Uuid> = Vec::new();
                let mut clicked_existing = false;
                let shift_held = ui.input(|i| i.modifiers.shift);

                for n_idx in 0..notes.len() {
                    let note = notes[n_idx];
                    if note.pitch < self.lowest_pitch
                        || note.pitch >= self.lowest_pitch + rows as u8
                    {
                        continue;
                    }
                    let r = (rows - 1) - (note.pitch - self.lowest_pitch) as usize;
                    let nx0 = grid_rect.left() + note.start_beat as f32 * self.zoom_x;
                    let nx1 = grid_rect.left() + note.end_beat() as f32 * self.zoom_x;
                    let ny0 = grid_rect.top() + r as f32 * row_h + 0.8;
                    let nrect = Rect::from_min_max(
                        Pos2::new(nx0, ny0),
                        Pos2::new(nx1.max(nx0 + 5.0), ny0 + row_h - 1.6),
                    );

                    let n_resp = ui.interact(
                        nrect,
                        ui.id().with(("midi_note", note.id)),
                        Sense::click_and_drag(),
                    );
                    let selected = self.selected_notes.contains(&note.id);
                    let vel_alpha = (note.velocity as f32 / 127.0).clamp(0.35, 1.0);
                    ui.painter()
                        .rect_filled(nrect, 2, t_col.gamma_multiply(vel_alpha));
                    ui.painter().rect_stroke(
                        nrect,
                        2,
                        Stroke::new(
                            if selected { 2.0_f32 } else { 1.0_f32 },
                            if selected {
                                ACCENT_TEAL
                            } else {
                                Color32::WHITE.gamma_multiply(0.7)
                            },
                        ),
                        StrokeKind::Inside,
                    );

                    if self.active_cc_lane.is_none() {
                        let vh = (note.velocity as f32 / 127.0) * (vel_rect.height() - 4.0);
                        ui.painter().line_segment(
                            [
                                Pos2::new(nx0 + 2.0, vel_rect.bottom()),
                                Pos2::new(nx0 + 2.0, vel_rect.bottom() - vh),
                            ],
                            Stroke::new(3.0_f32, t_col),
                        );
                    }

                    n_resp.context_menu(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Edit note · {}{}",
                                NOTE_PITCH_CLASSES[(note.pitch as usize) % 12],
                                (note.pitch as i32 / 12) - 1
                            ))
                            .strong(),
                        );
                        ui.separator();
                        let mut vel = notes[n_idx].velocity as i32;
                        if ui
                            .add(egui::Slider::new(&mut vel, 1..=127).text("Velocity"))
                            .changed()
                        {
                            notes[n_idx].velocity = vel as u8;
                            actions.push(DockAction::ProjectMutated("Edit Note Velocity"));
                        }
                        let mut len = notes[n_idx].length_beats as f32;
                        if ui
                            .add(
                                egui::Slider::new(&mut len, 0.0625..=8.0)
                                    .text("Length (beats)")
                                    .logarithmic(true),
                            )
                            .changed()
                        {
                            notes[n_idx].length_beats = len as f64;
                            self.default_note_len = notes[n_idx].length_beats;
                            actions.push(DockAction::ProjectMutated("Edit Note Length"));
                        }
                        let mut pitch = notes[n_idx].pitch as i32;
                        if ui
                            .add(egui::Slider::new(&mut pitch, 0..=127).text("Pitch"))
                            .changed()
                        {
                            let cand = pitch.clamp(0, 127) as u8;
                            notes[n_idx].pitch = if self.scale_lock {
                                scale.snap_pitch(root_pc, cand)
                            } else {
                                cand
                            };
                            actions.push(DockAction::ProjectMutated("Edit Note Pitch"));
                        }
                        ui.separator();
                        if ui.chrome_button("Delete note").clicked() {
                            delete_note_ids.push(note.id);
                            ui.close_menu();
                        }
                    });

                    if n_resp.clicked() {
                        clicked_existing = true;
                        if shift_held {
                            if let Some(i) =
                                self.selected_notes.iter().position(|id| *id == note.id)
                            {
                                self.selected_notes.remove(i);
                            } else {
                                self.selected_notes.push(note.id);
                            }
                        } else {
                            self.selected_notes.clear();
                            self.selected_notes.push(note.id);
                        }
                    } else if n_resp.drag_started() {
                        clicked_existing = true;
                        if !self.selected_notes.contains(&note.id) {
                            if !shift_held {
                                self.selected_notes.clear();
                            }
                            self.selected_notes.push(note.id);
                        }
                        let near_right = n_resp
                            .interact_pointer_pos()
                            .map(|p| (nrect.right() - p.x).abs() < 7.0)
                            .unwrap_or(false);
                        self.drag_note = Some((note.id, near_right));
                        self.drag_pitch_accum = 0.0;
                    }

                    if let Some((did, resize_r)) = self.drag_note {
                        if did == note.id && n_resp.dragged() {
                            let d = n_resp.drag_delta();
                            if resize_r {
                                notes[n_idx].length_beats =
                                    (notes[n_idx].length_beats + (d.x / self.zoom_x) as f64)
                                        .max(0.125);
                                self.default_note_len = notes[n_idx].length_beats;
                            } else {
                                let db = (d.x / self.zoom_x) as f64;
                                self.drag_pitch_accum += d.y;
                                let mut dp = 0i32;
                                if self.drag_pitch_accum.abs() >= row_h * 0.55 {
                                    dp = (-self.drag_pitch_accum / row_h).trunc() as i32;
                                    self.drag_pitch_accum += dp as f32 * row_h;
                                }
                                let move_ids: Vec<Uuid> = if self.selected_notes.contains(&did)
                                {
                                    self.selected_notes.clone()
                                } else {
                                    vec![did]
                                };
                                for id in move_ids {
                                    if let Some(n) = notes.iter_mut().find(|n| n.id == id) {
                                        n.start_beat = (n.start_beat + db).max(0.0);
                                        if dp != 0 {
                                            let cand =
                                                (n.pitch as i32 + dp).clamp(0, 127) as u8;
                                            n.pitch = if self.scale_lock {
                                                scale.snap_pitch(root_pc, cand)
                                            } else {
                                                cand
                                            };
                                        }
                                    }
                                }
                            }
                            actions.push(DockAction::ProjectMutated("Edit MIDI Note"));
                        }
                    }
                }

                // Draw & Edit MIDI CC / Pitch-Bend Automation Lane if selected!
                if let Some(cc_tgt) = self.active_cc_lane {
                    let vel_resp = ui.interact(
                        vel_rect,
                        ui.id().with("midi_cc_lane_draw"),
                        Sense::click_and_drag(),
                    );
                    if vel_resp.clicked() || vel_resp.dragged() {
                        if let Some(pos) = vel_resp.interact_pointer_pos() {
                            let beat = ((pos.x - vel_rect.left()) / self.zoom_x).max(0.0) as f64;
                            let val =
                                ((vel_rect.bottom() - pos.y) / vel_rect.height()).clamp(0.0, 1.0);
                            if let Some(existing) = cc_points
                                .iter_mut()
                                .find(|p| p.target == cc_tgt && (p.beat - beat).abs() < 0.12)
                            {
                                existing.value = val;
                            } else {
                                cc_points.push(MidiCcPoint {
                                    beat,
                                    target: cc_tgt,
                                    value: val,
                                });
                                cc_points.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
                            }
                            actions.push(DockAction::ProjectMutated("Draw MIDI CC"));
                        }
                    }

                    let lane_pts: Vec<Pos2> = cc_points
                        .iter()
                        .filter(|p| p.target == cc_tgt)
                        .map(|p| {
                            Pos2::new(
                                vel_rect.left() + p.beat as f32 * self.zoom_x,
                                vel_rect.bottom() - p.value * (vel_rect.height() - 4.0) - 2.0,
                            )
                        })
                        .collect();
                    for &pt in &lane_pts {
                        ui.painter().circle_filled(pt, 3.2, ACCENT_TEAL);
                    }
                    if lane_pts.len() >= 2 {
                        ui.painter()
                            .add(egui::Shape::line(lane_pts, Stroke::new(1.5_f32, ACCENT_TEAL)));
                    }
                }

                if !delete_note_ids.is_empty() {
                    notes.retain(|n| !delete_note_ids.contains(&n.id));
                    self.selected_notes
                        .retain(|id| !delete_note_ids.contains(id));
                    actions.push(DockAction::ProjectMutated("Delete MIDI Note"));
                }

                if self.drag_note.is_some() && !ui.input(|i| i.pointer.any_down()) {
                    // Snap dragged notes to 1/16 on drop.
                    if let Some((_, false)) = self.drag_note {
                        let ids = self.selected_notes.clone();
                        for id in ids {
                            if let Some(n) = notes.iter_mut().find(|n| n.id == id) {
                                n.start_beat = (n.start_beat * 4.0).round() / 4.0;
                            }
                        }
                        actions.push(DockAction::ProjectMutated("Snap MIDI Note"));
                    }
                    self.drag_note = None;
                    self.drag_pitch_accum = 0.0;
                }

                // Backup: main.rs also deletes selected notes on Backspace/Delete (runs earlier).
                if !self.selected_notes.is_empty()
                    && ui.input(|i| {
                        i.key_pressed(Key::Backspace) || i.key_pressed(Key::Delete)
                    })
                {
                    let kill = self.selected_notes.clone();
                    let before = notes.len();
                    notes.retain(|n| !kill.contains(&n.id));
                    if notes.len() != before {
                        self.selected_notes.clear();
                        actions.push(DockAction::ProjectMutated("Delete MIDI Notes"));
                    }
                }

                // Click empty grid cell -> Insert Single Note OR Full Chord Stamp (Scale-Aware)!
                let grid_bg_resp =
                    ui.interact(grid_rect, ui.id().with("midi_grid_bg"), Sense::click());
                if grid_bg_resp.clicked() && !clicked_existing {
                    self.selected_notes.clear();
                    if let Some(pos) = grid_bg_resp.interact_pointer_pos() {
                        let raw_b = ((pos.x - grid_rect.left()) / self.zoom_x) as f64;
                        let start_b = (raw_b * 4.0).floor() / 4.0;
                        let r = (((pos.y - grid_rect.top()) / row_h).floor() as usize).min(rows - 1);
                        let raw_pitch = self.lowest_pitch + (rows - 1 - r) as u8;
                        let base_pitch = if self.scale_lock {
                            scale.snap_pitch(root_pc, raw_pitch)
                        } else {
                            raw_pitch
                        };

                        let mut new_ids = Vec::new();
                        for &semi_offset in self.chord_stamp.intervals() {
                            let p = (base_pitch.saturating_add(semi_offset)).min(127);
                            let n = MidiNote::new(p, 102, start_b, self.default_note_len);
                            new_ids.push(n.id);
                            notes.push(n);
                        }
                        self.selected_notes = new_ids;
                        actions.push(DockAction::ProjectMutated("Insert MIDI Note/Chord"));
                    }
                }

                let local_ph = telem.playhead_beat - clip_start;
                if local_ph >= 0.0 && local_ph <= clip_len {
                    let px = grid_rect.left() + (local_ph.rem_euclid(clip_len)) as f32 * self.zoom_x;
                    ui.painter().line_segment(
                        [Pos2::new(px, grid_rect.top()), Pos2::new(px, vel_rect.bottom())],
                        Stroke::new(1.5_f32, ACCENT_FERRIC),
                    );
                }
            });
        self.h_scroll_px = (scroll_out.state.offset.x + zoom_scroll_adjust).max(0.0);
    }

    fn render_audio_clip_inspector(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        track_id: TrackId,
        clip_id: ClipId,
        telem: &EngineTelemetry,
        actions: &mut Vec<DockAction>,
    ) {
        let t_col = project
            .find_track(track_id)
            .map(|t| track_color32(t.color))
            .unwrap_or(ACCENT_TEAL);
        let bpm = project.bpm;
        let root_pc = project.root_pitch_class;
        let scale = project.musical_scale;

        let Some(clip) = project
            .find_track_mut(track_id)
            .and_then(|t| t.find_clip_mut(clip_id))
        else {
            return;
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(115.0));
            let mut ch = false;

            if pill_toggle(
                ui,
                "🎯 Melodyne Pitch Blobs",
                self.audio_pitch_blob_view,
                Color32::from_rgb(232, 114, 62),
            )
            .on_hover_text(
                "Toggle Inline Melodyne / ReaTune Per-Note Visual Pitch Blob Editor & Audio-to-MIDI Converter!",
            )
            .clicked()
            {
                self.audio_pitch_blob_view = !self.audio_pitch_blob_view;
                if self.audio_pitch_blob_view && clip.pitch_blobs.is_empty() {
                    let at = clip.active_take.min(clip.takes.len().saturating_sub(1));
                    if let Some(TakeContent::Audio { buffer }) =
                        clip.takes.get(at).map(|t| &t.content)
                    {
                        clip.pitch_blobs =
                            analyze_audio_pitch_blobs(buffer, bpm, clip.length_beats);
                        ch = true;
                    }
                }
            }

            if self.audio_pitch_blob_view {
                if ui
                    .chrome_small_button("🔍 Re-Analyze")
                    .on_hover_text("Re-detect vocal/guitar monophonic pitch blobs via YIN")
                    .clicked()
                {
                    let at = clip.active_take.min(clip.takes.len().saturating_sub(1));
                    if let Some(TakeContent::Audio { buffer }) =
                        clip.takes.get(at).map(|t| &t.content)
                    {
                        clip.pitch_blobs =
                            analyze_audio_pitch_blobs(buffer, bpm, clip.length_beats);
                        ch = true;
                    }
                }
                if ui
                    .chrome_small_button("🧲 Snap All to Scale")
                    .on_hover_text("Quantize every detected vocal/guitar note blob to the Session Key & Scale!")
                    .clicked()
                {
                    for b in &mut clip.pitch_blobs {
                        b.target_midi = scale.snap_continuous_midi(root_pc, b.detected_midi);
                        b.fine_cents = 0.0;
                    }
                    ch = true;
                }
                if !clip.pitch_blobs.is_empty()
                    && ui
                        .chrome_small_button("🎹 Audio → MIDI Track")
                        .on_hover_text(
                            "Convert detected vocal/guitar pitch blobs into a brand-new playable FerricWave Synth MIDI Track!",
                        )
                        .clicked()
                {
                    let extracted = clip.pitch_blobs_to_midi_notes();
                    actions.push(DockAction::CreateMidiTrackFromAudioBlobs {
                        source_name: clip.name.clone(),
                        start_beat: clip.start_beat,
                        length_beats: clip.length_beats,
                        notes: extracted,
                    });
                }
            }

            egui::ComboBox::from_id_salt("stretch_algo_combo")
                .selected_text(clip.stretch_mode.label())
                .width(128.0)
                .show_ui(ui, |ui| {
                    for m in [
                        TimeStretchMode::ElastiqueWsola,
                        TimeStretchMode::TapeVarispeed,
                    ] {
                        ch |= ui
                            .selectable_value(&mut clip.stretch_mode, m, m.label())
                            .changed();
                    }
                });

            ch |= ui
                .add(
                    egui::DragValue::new(&mut clip.stretch_rate)
                        .range(0.25..=4.0)
                        .speed(0.02)
                        .prefix("Rate ")
                        .suffix("x"),
                )
                .changed();
            ch |= ui
                .add(
                    egui::DragValue::new(&mut clip.pitch_semitones)
                        .range(-24.0..=24.0)
                        .speed(0.1)
                        .prefix("Pitch ")
                        .suffix(" st"),
                )
                .changed();

            if !self.audio_pitch_blob_view {
                if ui
                    .chrome_small_button("⚡ Auto-Warp")
                    .on_hover_text(
                        "Detect drum/guitar transients & place draggable WSOLA Stretch Markers!",
                    )
                    .clicked()
                {
                    clip.generate_transient_warp_markers(bpm);
                    ch = true;
                }
                if !clip.warp_markers.is_empty() && ui.chrome_small_button("Clear").clicked() {
                    clip.warp_markers.clear();
                    ch = true;
                }
            }

            ui.menu_button("+ Clip FX", |ui| {
                for &kind in FxKind::ALL {
                    if !kind.is_instrument()
                        && kind != FxKind::ExternalClap
                        && ui.chrome_button(kind.display_name()).clicked()
                    {
                        clip.clip_fx.push(FxSlot::new(kind));
                        actions.push(DockAction::ProjectMutated("Add Per-Clip FX"));
                        ui.close_menu();
                    }
                }
            });

            if ch {
                actions.push(DockAction::ProjectMutated("Adjust Audio Warp/Pitch"));
            }
        });

        let wave_h = (ui.available_height() - 36.0).max(90.0);
        let (wrect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), wave_h), Sense::hover());
        ui.painter().rect_filled(wrect, 4, BG_DEEP);
        ui.painter().rect_stroke(
            wrect,
            4,
            Stroke::new(1.0_f32, BORDER_SUBTLE),
            StrokeKind::Inside,
        );

        // MODE A: Interactive Melodyne / VariAudio Note-Blob Pitch Canvas!
        if self.audio_pitch_blob_view {
            let min_midi = clip
                .pitch_blobs
                .iter()
                .map(|b| b.target_midi.min(b.detected_midi).floor() as i32)
                .min()
                .unwrap_or(55)
                - 3;
            let max_midi = clip
                .pitch_blobs
                .iter()
                .map(|b| b.target_midi.max(b.detected_midi).ceil() as i32)
                .max()
                .unwrap_or(72)
                + 3;
            let semi_span = (max_midi - min_midi).max(12) as f32;
            let row_h = wrect.height() / semi_span;

            // Semitone swimlanes with Session Scale highlighting
            for m in min_midi..max_midi {
                let r_idx = (max_midi - 1 - m) as f32;
                let y0 = wrect.top() + r_idx * row_h;
                let r_rect =
                    Rect::from_min_size(Pos2::new(wrect.left(), y0), Vec2::new(wrect.width(), row_h));
                let in_scale = scale.contains_pitch(root_pc, m.clamp(0, 127) as u8);
                ui.painter().rect_filled(
                    r_rect,
                    0,
                    if in_scale {
                        Color32::from_rgb(22, 26, 35)
                    } else {
                        Color32::from_rgb(14, 15, 20)
                    },
                );
                ui.painter().line_segment(
                    [r_rect.left_bottom(), r_rect.right_bottom()],
                    Stroke::new(0.5_f32, Color32::from_rgb(30, 34, 44)),
                );
                if m % 12 == (root_pc as i32 % 12) || m % 12 == 0 {
                    let nm = NOTE_PITCH_CLASSES[(m.rem_euclid(12)) as usize];
                    ui.painter().text(
                        Pos2::new(wrect.left() + 4.0, r_rect.center().y),
                        Align2::LEFT_CENTER,
                        format!("{}{}", nm, (m / 12) - 1),
                        FontId::monospace(8.5),
                        TEXT_DIM,
                    );
                }
            }

            let clip_len = clip.length_beats.max(0.25);
            for b_idx in 0..clip.pitch_blobs.len() {
                let blob_id = clip.pitch_blobs[b_idx].id;
                let sb = clip.pitch_blobs[b_idx].start_beat;
                let eb = clip.pitch_blobs[b_idx].end_beat;
                let cur_midi =
                    clip.pitch_blobs[b_idx].target_midi + clip.pitch_blobs[b_idx].fine_cents * 0.01;
                let orig_midi = clip.pitch_blobs[b_idx].detected_midi;
                let rms = clip.pitch_blobs[b_idx].rms_norm;

                let x0 = wrect.left() + (sb / clip_len) as f32 * wrect.width();
                let x1 = (wrect.left() + (eb / clip_len) as f32 * wrect.width()).max(x0 + 8.0);

                // Ghost outline at original detected pitch if shifted!
                if (cur_midi - orig_midi).abs() > 0.15 {
                    let gy = wrect.bottom() - ((orig_midi - min_midi as f32 + 0.5) / semi_span) * wrect.height();
                    let grect = Rect::from_min_max(
                        Pos2::new(x0, gy - row_h * 0.35),
                        Pos2::new(x1, gy + row_h * 0.35),
                    );
                    ui.painter().rect_stroke(
                        grect,
                        4,
                        Stroke::new(1.0_f32, Color32::from_gray(85)),
                        StrokeKind::Inside,
                    );
                }

                let cy =
                    wrect.bottom() - ((cur_midi - min_midi as f32 + 0.5) / semi_span) * wrect.height();
                let half_thick = (row_h * (0.30 + 0.35 * rms)).clamp(4.0, 16.0);
                let brect = Rect::from_min_max(
                    Pos2::new(x0, cy - half_thick),
                    Pos2::new(x1, cy + half_thick),
                );

                ui.painter()
                    .rect_filled(brect, 5, ACCENT_FERRIC.gamma_multiply(0.75));
                ui.painter().rect_stroke(
                    brect,
                    5,
                    Stroke::new(1.2_f32, Color32::WHITE.gamma_multiply(0.85)),
                    StrokeKind::Inside,
                );

                // Draw fine vocal vibrato pitch-contour curve right through the Melodyne blob!
                let contour = &clip.pitch_blobs[b_idx].contour_semitones;
                if contour.len() >= 2 {
                    let mut c_pts = Vec::with_capacity(contour.len());
                    for (ci, &c_off) in contour.iter().enumerate() {
                        let u = ci as f32 / (contour.len() - 1) as f32;
                        let px = x0 + u * (x1 - x0);
                        let py = cy - (c_off / semi_span) * wrect.height();
                        c_pts.push(Pos2::new(px, py.clamp(wrect.top(), wrect.bottom())));
                    }
                    ui.painter()
                        .add(egui::Shape::line(c_pts, Stroke::new(1.4_f32, ACCENT_TEAL)));
                }

                let note_i = cur_midi.round().clamp(0.0, 127.0) as usize;
                ui.painter().text(
                    Pos2::new(brect.left() + 4.0, brect.center().y),
                    Align2::LEFT_CENTER,
                    format!("{}{}", NOTE_PITCH_CLASSES[note_i % 12], (note_i as i32 / 12) - 1),
                    FontId::monospace(9.0),
                    Color32::WHITE,
                );

                let b_resp = ui
                    .interact(
                        brect,
                        ui.id().with(("pitch_blob", blob_id)),
                        Sense::click_and_drag(),
                    )
                    .on_hover_text(
                        "Drag Up/Down to repitch individual note (Shift+Drag for fine cents, Double-click to reset to original pitch!)",
                    );
                if b_resp.double_clicked() {
                    let det = clip.pitch_blobs[b_idx].detected_midi;
                    clip.pitch_blobs[b_idx].target_midi = det.round();
                    clip.pitch_blobs[b_idx].fine_cents = (det - det.round()) * 100.0;
                    actions.push(DockAction::ProjectMutated("Reset Pitch Blob"));
                } else if b_resp.drag_started() {
                    self.drag_blob = Some(blob_id);
                }
                if self.drag_blob == Some(blob_id) && b_resp.dragged() {
                    let dy = -b_resp.drag_delta().y;
                    let d_semi = (dy / row_h).clamp(-4.0, 4.0);
                    if ui.input(|i| i.modifiers.shift) {
                        clip.pitch_blobs[b_idx].fine_cents =
                            (clip.pitch_blobs[b_idx].fine_cents + d_semi * 25.0).clamp(-100.0, 100.0);
                    } else {
                        let raw_next =
                            (clip.pitch_blobs[b_idx].target_midi + d_semi).clamp(24.0, 108.0);
                        clip.pitch_blobs[b_idx].target_midi = raw_next;
                        clip.pitch_blobs[b_idx].fine_cents = 0.0;
                    }
                    actions.push(DockAction::ProjectMutated("Drag Pitch Blob"));
                }
            }
            if self.drag_blob.is_some() && !ui.input(|i| i.pointer.any_down()) {
                if let Some(did) = self.drag_blob.take() {
                    if let Some(b) = clip.pitch_blobs.iter_mut().find(|x| x.id == did) {
                        if self.scale_lock {
                            b.target_midi = scale.snap_continuous_midi(root_pc, b.target_midi);
                        } else {
                            b.target_midi = b.target_midi.round();
                        }
                    }
                }
            }

            // Playhead cursor inside Pitch-Blob editor
            let local_ph = telem.playhead_beat - clip.start_beat;
            if local_ph >= 0.0 && local_ph <= clip_len {
                let px = wrect.left() + (local_ph / clip_len) as f32 * wrect.width();
                ui.painter().line_segment(
                    [Pos2::new(px, wrect.top()), Pos2::new(px, wrect.bottom())],
                    Stroke::new(1.5_f32, ACCENT_FERRIC),
                );
            }
        } else {
            // MODE B: High-Res Waveform + WSOLA Warp Markers
            let cols = (wrect.width() as usize).clamp(32, 1200);
            let mid_y = wrect.center().y;
            let half_h = wrect.height() * 0.45;
            for px in 0..cols {
                let frac = px as f64 / cols as f64;
                let local_b = frac * clip.length_beats;
                let take_idx = clip.take_at_local_beat(local_b);
                if let Some(TakeContent::Audio { buffer }) =
                    clip.takes.get(take_idx).map(|t| &t.content)
                {
                    if !buffer.peaks.is_empty() {
                        let pk_i = ((frac * buffer.peaks.len() as f64) as usize)
                            .min(buffer.peaks.len() - 1);
                        let (mn, mx) = buffer.peaks[pk_i];
                        let env = clip.gain_envelope_at(local_b).min(2.5);
                        let x = wrect.left() + px as f32;
                        let y0 = (mid_y - mx * env * half_h).clamp(wrect.top(), wrect.bottom());
                        let y1 = (mid_y - mn * env * half_h).clamp(wrect.top(), wrect.bottom());
                        let seg_col = match take_idx % 3 {
                            0 => t_col,
                            1 => ACCENT_TEAL,
                            _ => ACCENT_PURPLE,
                        };
                        ui.painter().line_segment(
                            [Pos2::new(x, y0), Pos2::new(x, y1)],
                            Stroke::new(1.2_f32, seg_col),
                        );
                    }
                }
            }

            for m_idx in 0..clip.warp_markers.len() {
                let wm = clip.warp_markers[m_idx];
                let wx = wrect.left()
                    + (wm.clip_beat / clip.length_beats.max(0.25)) as f32 * wrect.width();
                if wx >= wrect.left() && wx <= wrect.right() {
                    ui.painter().line_segment(
                        [Pos2::new(wx, wrect.top()), Pos2::new(wx, wrect.bottom())],
                        Stroke::new(1.4_f32, ACCENT_AMBER),
                    );
                    let handle =
                        Rect::from_center_size(Pos2::new(wx, wrect.top() + 8.0), Vec2::new(12.0, 14.0));
                    ui.painter().rect_filled(handle, 2, ACCENT_AMBER);
                    let h_resp = ui.interact(
                        handle,
                        ui.id().with(("warp_mk", wm.id)),
                        Sense::click_and_drag(),
                    );
                    if h_resp.drag_started() {
                        self.drag_warp = Some(wm.id);
                    }
                    if self.drag_warp == Some(wm.id) && h_resp.dragged() {
                        let db =
                            (h_resp.drag_delta().x / wrect.width()) as f64 * clip.length_beats;
                        clip.warp_markers[m_idx].clip_beat =
                            (clip.warp_markers[m_idx].clip_beat + db).clamp(0.0, clip.length_beats);
                        actions.push(DockAction::ProjectMutated("Drag Warp Marker"));
                    }
                }
            }
            if self.drag_warp.is_some() && !ui.input(|i| i.pointer.any_down()) {
                self.drag_warp = None;
            }
        }

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Active Take:").size(11.0).color(TEXT_DIM));
            for i in 0..clip.takes.len() {
                let lbl = format!("{}: {}", i + 1, clip.takes[i].name);
                if pill_toggle(
                    ui,
                    &lbl,
                    clip.active_take == i && clip.comp_segments.is_empty(),
                    t_col,
                )
                .clicked()
                {
                    clip.active_take = i;
                    clip.comp_segments.clear();
                    actions.push(DockAction::ProjectMutated("Select Take"));
                }
            }
            if !clip.comp_segments.is_empty() && ui.chrome_small_button("Reset Swipe Comp").clicked() {
                clip.comp_segments.clear();
                actions.push(DockAction::ProjectMutated("Reset Swipe Comp"));
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_drum_step_sequencer(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        track_id: TrackId,
        clip_id: ClipId,
        clip_start: f64,
        clip_len: f64,
        accent: Color32,
        telem: &EngineTelemetry,
        actions: &mut Vec<DockAction>,
    ) {
        const DRUM_ROWS: &[(&str, u8)] = &[
            ("Kick (36)", 36),
            ("Rimshot (37)", 37),
            ("Snare (38)", 38),
            ("Clap (39)", 39),
            ("Closed Hat (42)", 42),
            ("Open Hat (46)", 46),
            ("Tom (43)", 43),
            ("Crash/Ride (49)", 49),
        ];

        let num_bars = ((clip_len / 4.0).ceil() as usize).clamp(1, 16);
        self.drum_bar_index = self.drum_bar_index.min(num_bars - 1);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Pattern Bar:").size(11.0).color(TEXT_DIM));
            for b in 0..num_bars {
                if pill_toggle(
                    ui,
                    &format!("Bar {}", b + 1),
                    self.drum_bar_index == b,
                    accent,
                )
                .clicked()
                {
                    self.drum_bar_index = b;
                }
            }
        });

        let bar_offset_beat = (self.drum_bar_index * 4) as f64;
        let local_ph = (telem.playhead_beat - clip_start).rem_euclid(clip_len.max(4.0));
        let active_step_opt = if telem.is_playing
            && local_ph >= bar_offset_beat
            && local_ph < bar_offset_beat + 4.0
        {
            Some(((local_ph - bar_offset_beat) * 4.0).floor() as usize)
        } else {
            None
        };

        let Some(clip_mut) = project
            .find_track_mut(track_id)
            .and_then(|t| t.find_clip_mut(clip_id))
        else {
            return;
        };
        let active_take = clip_mut.active_take.min(clip_mut.takes.len().saturating_sub(1));
        let Some(TakeContent::Midi { notes, .. }) =
            clip_mut.takes.get_mut(active_take).map(|t| &mut t.content)
        else {
            return;
        };

        egui::ScrollArea::vertical()
            .id_salt("drum_step_seq_scroll")
            .show(ui, |ui| {
                egui::Grid::new("drum_step_16_grid")
                    .spacing([4.0, 3.0])
                    .show(ui, |ui| {
                        for &(row_name, pitch) in DRUM_ROWS {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(row_name).size(10.0).color(TEXT_MAIN),
                                    )
                                    .fill(BUTTON_SURFACE)
                                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                    .corner_radius(BUTTON_RADIUS)
                                    .min_size(Vec2::new(98.0, 18.0)),
                                )
                                .on_hover_text("Click to audition drum pad")
                                .clicked()
                            {
                                actions.push(DockAction::AuditionMidi {
                                    track_id,
                                    note: pitch,
                                    is_note_on: true,
                                });
                            }

                            for step in 0..16usize {
                                let step_beat = bar_offset_beat + (step as f64) * 0.25;
                                let existing_idx = notes.iter().position(|n| {
                                    n.pitch == pitch && (n.start_beat - step_beat).abs() < 0.08
                                });
                                let is_on = existing_idx.is_some();
                                let is_ph = active_step_opt == Some(step);

                                let fill = if is_on {
                                    if is_ph {
                                        Color32::WHITE
                                    } else if (step / 4) % 2 == 0 {
                                        accent
                                    } else {
                                        ACCENT_TEAL
                                    }
                                } else if is_ph {
                                    Color32::from_rgb(55, 60, 74)
                                } else if (step / 4) % 2 == 0 {
                                    Color32::from_rgb(28, 31, 40)
                                } else {
                                    Color32::from_rgb(21, 23, 30)
                                };

                                if ui
                                    .add(
                                        egui::Button::new("")
                                            .fill(fill)
                                            .min_size(Vec2::new(22.0, 18.0)),
                                    )
                                    .clicked()
                                {
                                    if let Some(idx) = existing_idx {
                                        notes.remove(idx);
                                    } else {
                                        let vel = if step % 4 == 0 { 116 } else { 96 };
                                        notes.push(MidiNote::new(pitch, vel, step_beat, 0.22));
                                    }
                                    actions.push(DockAction::ProjectMutated("Toggle Drum Step"));
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    }
}

pub fn render_fx_and_modulators_panel(
    ui: &mut Ui,
    project: &mut Project,
    selected_track: Option<TrackId>,
    inspect_master: &mut bool,
    open_editor_ids: &[Uuid],
    telem: &EngineTelemetry,
    actions: &mut Vec<DockAction>,
) {
    let all_tracks: Vec<(TrackId, String)> = project
        .tracks
        .iter()
        .map(|t| (t.id, t.name.clone()))
        .collect();

    chrome_card(ui, ACCENT_FERRIC, |ui| {
    ui.horizontal(|ui| {
        chrome_section_title(ui, "FX Rack", ACCENT_FERRIC);
        if pill_toggle(ui, "Track FX & Modulators", !*inspect_master, ACCENT_FERRIC).clicked() {
            *inspect_master = false;
        }
        if pill_toggle(ui, "Master Bus Chain + Spectrum", *inspect_master, ACCENT_TEAL).clicked() {
            *inspect_master = true;
        }
        ui.separator();

        if *inspect_master {
            ui.menu_button("+ Add Master FX", |ui| {
                for &kind in FxKind::ALL {
                    if !kind.is_instrument()
                        && kind != FxKind::ExternalClap
                        && ui.chrome_button(kind.display_name()).clicked()
                    {
                        project.master.fx_chain.push(FxSlot::new(kind));
                        actions.push(DockAction::ProjectMutated("Add Master FX"));
                        ui.close_menu();
                    }
                }
            });
        } else if let Some(tid) = selected_track {
            ui.menu_button("+ Add Instrument / FX", |ui| {
                for &kind in FxKind::ALL {
                    if kind != FxKind::ExternalClap && ui.chrome_button(kind.display_name()).clicked() {
                        if let Some(trk) = project.find_track_mut(tid) {
                            trk.fx_chain.push(FxSlot::new(kind));
                            actions.push(DockAction::ProjectMutated("Add Track FX"));
                        }
                        ui.close_menu();
                    }
                }
            });
            ui.menu_button("+ Add Modulator", |ui| {
                if ui.chrome_button("🌊 Tempo-Synced LFO").clicked() {
                    if let Some(trk) = project.find_track_mut(tid) {
                        trk.modulators.push(ModulatorSlot::new_lfo("LFO"));
                        actions.push(DockAction::ProjectMutated("Add LFO Modulator"));
                    }
                    ui.close_menu();
                }
                if ui.chrome_button("📈 Audio Envelope Follower").clicked() {
                    if let Some(trk) = project.find_track_mut(tid) {
                        trk.modulators
                            .push(ModulatorSlot::new_env_follower("Env Follower"));
                        actions.push(DockAction::ProjectMutated("Add Envelope Follower"));
                    }
                    ui.close_menu();
                }
                if ui.chrome_button("🎛 Macro Knob").clicked() {
                    if let Some(trk) = project.find_track_mut(tid) {
                        trk.modulators
                            .push(ModulatorSlot::new_macro("Macro 1", 0.5));
                        actions.push(DockAction::ProjectMutated("Add Macro Modulator"));
                    }
                    ui.close_menu();
                }
            });

            if let Some(trk) = project.find_track_mut(tid) {
                egui::ComboBox::from_id_salt("trk_auto_mode")
                    .selected_text(format!("Auto: {}", trk.automation_mode.label()))
                    .width(98.0)
                    .show_ui(ui, |ui| {
                        for &am in ferric_model::AutomationMode::ALL {
                            ui.selectable_value(&mut trk.automation_mode, am, am.label());
                        }
                    });

                ui.separator();
                // Built-in Tempo-Synced MIDI Arpeggiator strip!
                let mut arp_ch = false;
                if pill_toggle(ui, "🎹 ARP", trk.arpeggiator.enabled, ACCENT_AMBER)
                    .on_hover_text(
                        "Enable tempo-synced MIDI arpeggiator / chord gate (live USB MIDI & clips)",
                    )
                    .clicked()
                {
                    trk.arpeggiator.enabled = !trk.arpeggiator.enabled;
                    arp_ch = true;
                }
                if trk.arpeggiator.enabled {
                    egui::ComboBox::from_id_salt("trk_arp_mode")
                        .selected_text(trk.arpeggiator.mode.label())
                        .width(82.0)
                        .show_ui(ui, |ui| {
                            for &m in ferric_model::ArpMode::ALL {
                                arp_ch |= ui
                                    .selectable_value(&mut trk.arpeggiator.mode, m, m.label())
                                    .changed();
                            }
                        });
                    egui::ComboBox::from_id_salt("trk_arp_rate")
                        .selected_text(match (trk.arpeggiator.rate_beats * 100.0).round() as i32 {
                            25 => "1/16",
                            33 => "1/8T",
                            50 => "1/8",
                            _ => "1/4",
                        })
                        .width(58.0)
                        .show_ui(ui, |ui| {
                            for (lbl, r) in [("1/16", 0.25), ("1/8T", 1.0 / 3.0), ("1/8", 0.5), ("1/4", 1.0)] {
                                arp_ch |= ui
                                    .selectable_value(&mut trk.arpeggiator.rate_beats, r, lbl)
                                    .changed();
                            }
                        });
                    arp_ch |= ui
                        .add(
                            egui::DragValue::new(&mut trk.arpeggiator.octaves)
                                .range(1..=3)
                                .prefix("Oct "),
                        )
                        .changed();
                }
                if arp_ch {
                    actions.push(DockAction::ProjectMutated("Adjust Track Arpeggiator"));
                }
            }
        }
    });
    });

    ui.separator();

    egui::ScrollArea::horizontal()
        .id_salt("fx_rack_hscroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                if *inspect_master {
                    draw_live_spectrum_card(ui, telem);
                    render_fx_slot_cards(
                        ui,
                        None,
                        &all_tracks,
                        &mut project.master.fx_chain,
                        open_editor_ids,
                        ACCENT_TEAL,
                        telem,
                        actions,
                    );
                } else if let Some(tid) = selected_track {
                    let Some(trk) = project.find_track_mut(tid) else {
                        return;
                    };
                    let t_col = track_color32(trk.color);

                    let fx_snapshot: Vec<(Uuid, String, Vec<String>)> = trk
                        .fx_chain
                        .iter()
                        .map(|f| {
                            let badge = f.effective_badge().to_string();
                            let names = (0..f.param_count())
                                .map(|pi| f.param_name(pi).to_string())
                                .collect();
                            (f.id, badge, names)
                        })
                        .collect();
                    let mut remove_mod = None;
                    for (m_idx, m) in trk.modulators.iter_mut().enumerate() {
                        if render_modulator_card(ui, m, &fx_snapshot, actions) {
                            remove_mod = Some(m_idx);
                        }
                    }
                    if let Some(ri) = remove_mod {
                        trk.modulators.remove(ri);
                        actions.push(DockAction::ProjectMutated("Remove Modulator"));
                    }

                    render_fx_slot_cards(
                        ui,
                        Some(tid),
                        &all_tracks,
                        &mut trk.fx_chain,
                        open_editor_ids,
                        t_col,
                        telem,
                        actions,
                    );
                } else {
                    ui.vertical(|ui| {
                        ui.add_space(24.0);
                        ui.label(
                            egui::RichText::new("No track selected")
                                .size(14.0)
                                .strong()
                                .color(TEXT_MAIN),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Select a track in the arrangement, or switch to Master Bus above.",
                            )
                            .size(12.0)
                            .color(TEXT_DIM),
                        );
                        ui.label(
                            egui::RichText::new("Tip: click FX on a track header to open this panel.")
                                .size(11.0)
                                .color(TEXT_DIM),
                        );
                    });
                }
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn render_fx_slot_cards(
    ui: &mut Ui,
    owner_track: Option<TrackId>,
    all_tracks: &[(TrackId, String)],
    chain: &mut Vec<FxSlot>,
    open_editor_ids: &[Uuid],
    accent: Color32,
    telem: &EngineTelemetry,
    actions: &mut Vec<DockAction>,
) {
    let mut remove_idx = None;
    let mut move_swap: Option<(usize, usize)> = None;
    let len = chain.len();

    for (idx, fx) in chain.iter_mut().enumerate() {
        egui::Frame::new()
            .fill(BG_ELEVATED)
            .stroke(Stroke::new(
                1.0_f32,
                if fx.enabled {
                    accent.gamma_multiply(0.65)
                } else {
                    BORDER_SUBTLE
                },
            ))
            .corner_radius(6)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_min_height(160.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if pill_toggle(ui, "ON", fx.enabled, accent).clicked() {
                            fx.enabled = !fx.enabled;
                            actions.push(DockAction::ProjectMutated("Bypass FX"));
                        }
                        let title = fx
                            .clap_plugin_name
                            .as_deref()
                            .unwrap_or_else(|| fx.kind.display_name());
                        if fx.is_external_plugin() {
                            ui.label(
                                egui::RichText::new(format!("[{}]", fx.plugin_format.badge()))
                                    .size(10.0)
                                    .monospace()
                                    .color(ACCENT_AMBER),
                            );
                        }
                        ui.label(
                            egui::RichText::new(title)
                                .size(11.5)
                                .strong()
                                .color(TEXT_MAIN),
                        );
                        if fx.is_external_plugin() {
                            let is_open = open_editor_ids.contains(&fx.id);
                            let gui_lbl = if is_open { "🗕 Close UI" } else { "🗔 Open UI" };
                            if pill_toggle(ui, gui_lbl, is_open, ACCENT_TEAL)
                                .on_hover_text(
                                    "Open/Close Floating Native Win32 HWND Graphical Plugin Editor!",
                                )
                                .clicked()
                            {
                                actions.push(DockAction::ToggleExternalPluginEditor(fx.id));
                            }
                            if ui
                                .add(
                                    egui::DragValue::new(&mut fx.plugin_output_trim_db)
                                        .range(-24.0..=18.0)
                                        .speed(0.15)
                                        .prefix("Trim ")
                                        .suffix(" dB"),
                                )
                                .changed()
                            {
                                actions.push(DockAction::ProjectMutated("Adjust Plugin Trim"));
                            }
                            let sandbox_lbl = if fx.sandbox_out_of_process {
                                "🛡 Sandbox"
                            } else {
                                "In-Process"
                            };
                            if pill_toggle(
                                ui,
                                sandbox_lbl,
                                fx.sandbox_out_of_process,
                                ACCENT_TEAL,
                            )
                            .on_hover_text(
                                "When enabled, loads the plugin in an isolated ferric-plugin-bridge worker process (reload FX to apply).",
                            )
                            .clicked()
                            {
                                fx.sandbox_out_of_process = !fx.sandbox_out_of_process;
                                actions.push(DockAction::ProjectMutated("Toggle Plugin Sandbox"));
                            }
                        }
                        if fx.kind == FxKind::HostDna {
                            let fx_id = fx.id;
                            let seed_now = fx.host_dna_seed;
                            ui.label(
                                egui::RichText::new(format!(
                                    "Seed {}",
                                    crate::host_fingerprint::format_host_seed(seed_now)
                                ))
                                .size(10.0)
                                .monospace()
                                .color(ACCENT_TEAL),
                            );
                            if ui
                                .chrome_small_button("Reseed PC")
                                .on_hover_text("Hash this machine's coarse traits into a new Init")
                                .clicked()
                            {
                                actions.push(DockAction::HostDnaReseed(fx_id));
                            }
                            if ui.chrome_small_button("Randomize").clicked() {
                                actions.push(DockAction::HostDnaRandomize(fx_id));
                            }
                            for (sib, label) in [(0u8, "A"), (1, "B"), (2, "C")] {
                                if ui
                                    .chrome_small_button(label)
                                    .on_hover_text("Sibling Init from same seed")
                                    .clicked()
                                {
                                    actions.push(DockAction::HostDnaSibling(fx_id, sib));
                                }
                            }
                            let paste_id = egui::Id::new(("host_dna_paste", fx_id));
                            let mut paste = ui.data_mut(|d| {
                                d.get_temp_mut_or_default::<String>(paste_id).clone()
                            });
                            ui.add(
                                egui::TextEdit::singleline(&mut paste)
                                    .hint_text("Paste seed hex")
                                    .desired_width(110.0),
                            );
                            if ui.chrome_small_button("Paste seed").clicked() {
                                if let Some(s) = crate::host_fingerprint::parse_host_seed(&paste) {
                                    actions.push(DockAction::HostDnaPaste(fx_id, s));
                                }
                            }
                            ui.data_mut(|d| *d.get_temp_mut_or_default::<String>(paste_id) = paste);
                        }
                        if !fx.kind.is_instrument() {
                            if ui
                                .add(
                                    egui::DragValue::new(&mut fx.wet_mix)
                                        .range(0.0..=1.0)
                                        .speed(0.01)
                                        .prefix("Wet "),
                                )
                                .changed()
                            {
                                actions.push(DockAction::ProjectMutated("Adjust FX Wet/Dry"));
                            }
                        }

                        // External Sidechain Source Selector for Compressor & Gate!
                        if fx.kind.supports_sidechain() {
                            let sc_lbl = fx
                                .sidechain_source
                                .and_then(|id| all_tracks.iter().find(|(tid, _)| *tid == id))
                                .map(|(_, n)| format!("SC: {}", n))
                                .unwrap_or_else(|| "SC: Internal".into());
                            egui::ComboBox::from_id_salt(("fx_sc_src", fx.id))
                                .selected_text(sc_lbl)
                                .width(105.0)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(fx.sidechain_source.is_none(), "Internal")
                                        .clicked()
                                    {
                                        fx.sidechain_source = None;
                                        actions.push(DockAction::ProjectMutated("Set Sidechain"));
                                    }
                                    for (tid, tname) in all_tracks {
                                        if Some(*tid) != owner_track
                                            && ui
                                                .selectable_label(
                                                    fx.sidechain_source == Some(*tid),
                                                    tname,
                                                )
                                                .clicked()
                                        {
                                            fx.sidechain_source = Some(*tid);
                                            actions.push(DockAction::ProjectMutated(
                                                "Set Sidechain",
                                            ));
                                        }
                                    }
                                });
                        }

                        if idx > 0 && ui.chrome_small_button("◀").clicked() {
                            move_swap = Some((idx, idx - 1));
                        }
                        if idx + 1 < len && ui.chrome_small_button("▶").clicked() {
                            move_swap = Some((idx, idx + 1));
                        }
                        if ui.chrome_small_button("×").clicked() {
                            remove_idx = Some(idx);
                        }
                    });

                    ui.separator();

                    // Special visualizers per FX type
                    match fx.kind {
                        FxKind::ParametricEq => draw_eq_curve_mini(ui, &fx.params, telem),
                        FxKind::ChromaticTuner => draw_strobe_tuner_display(ui, &telem.tuner),
                        FxKind::VocalPitch => {
                            draw_vocal_pitch_display(ui, &fx.params, &telem.vocal_pitch)
                        }
                        FxKind::WavetableSynth => {
                            draw_wavetable_3d_scope(ui, fx, telem.sample_rate, actions);
                        }
                        FxKind::ConvolutionIr => {
                            draw_convolution_ir_header(ui, fx, telem.sample_rate, actions);
                        }
                        FxKind::MultibandOtt => {
                            draw_ott_multiband_visualizer(ui, &fx.params, telem);
                        }
                        FxKind::SamplePlayer => {
                            ui.horizontal(|ui| {
                                let sample_lbl = format!(
                                    "Sample: {} ({} slices)",
                                    fx.preset_name,
                                    fx.sampler_slices.len()
                                );
                                ui.label(
                                    egui::RichText::new(sample_lbl)
                                        .size(10.5)
                                        .color(ACCENT_TEAL),
                                );
                                if ui.chrome_small_button("📂 Load Audio...").clicked() {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .add_filter(
                                            "Audio",
                                            &["wav", "flac", "mp3", "ogg", "aiff"],
                                        )
                                        .pick_file()
                                    {
                                        if let Ok(buf) = import_audio_file(&p, telem.sample_rate) {
                                            fx.preset_name = p
                                                .file_stem()
                                                .and_then(|s| s.to_str())
                                                .unwrap_or("Sample")
                                                .to_string();
                                            fx.sampler_slices = buf.detect_transients(16);
                                            fx.sampler_buffer = Some(buf);
                                            actions.push(DockAction::ReloadSamplerFx(fx.clone()));
                                            actions.push(DockAction::ProjectMutated(
                                                "Load Sampler File",
                                            ));
                                        }
                                    }
                                }
                            });
                        }
                        _ => {}
                    }

                    // Render either Built-In Param Specs OR Dynamically Enumerated CLAP/VST3 Plugin Knobs!
                    let total_params = fx.param_count().min(24);
                    if fx.params.len() < fx.param_count() {
                        let c = fx.param_count();
                        fx.params.resize_with(c, || 0.0);
                    }
                    let badge_str = fx.effective_badge();
                    for chunk_start in (0..total_params).step_by(6) {
                        ui.horizontal(|ui| {
                            let end = (chunk_start + 6).min(total_params);
                            for p_i in chunk_start..end {
                                let Some((min_v, max_v, def_v, unit_str)) = fx.param_bounds(p_i)
                                else {
                                    continue;
                                };
                                let p_name = fx.param_name(p_i).to_string();
                                let short_lbl: String = p_name.chars().take(12).collect();
                                ui.vertical(|ui| {
                                    if studio_knob(
                                        ui,
                                        &short_lbl,
                                        &mut fx.params[p_i],
                                        min_v,
                                        max_v,
                                        def_v,
                                        unit_str,
                                        30.0,
                                        accent,
                                    ) {
                                        actions.push(DockAction::ProjectMutated("Tweak FX Param"));
                                    }
                                    if let Some(tid) = owner_track {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new("Learn").size(8.5),
                                                )
                                                .fill(BUTTON_SURFACE)
                                                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                                .corner_radius(BUTTON_RADIUS)
                                                .min_size(Vec2::new(34.0, 13.0)),
                                            )
                                            .on_hover_text(format!(
                                                "{} — 1-Click MIDI CC Learn + Show Bezier Automation Lane",
                                                p_name
                                            ))
                                            .clicked()
                                        {
                                            actions.push(DockAction::ArmMidiLearn {
                                                track_id: tid,
                                                target: AutomationTarget::FxParam {
                                                    fx_id: fx.id,
                                                    param_index: p_i,
                                                },
                                                label: format!("{} → {}", badge_str, p_name),
                                            });
                                        }
                                    }
                                });
                            }
                        });
                    }
                    if fx.param_count() > 24 {
                        ui.label(
                            egui::RichText::new(format!(
                                "+ {} additional plugin parameters available inModulators & native GUI",
                                fx.param_count() - 24
                            ))
                            .size(9.5)
                            .color(TEXT_DIM),
                        );
                    }
                });
            });
    }

    if let Some((a, b)) = move_swap {
        chain.swap(a, b);
        actions.push(DockAction::ProjectMutated("Reorder FX Chain"));
    }
    if let Some(ri) = remove_idx {
        chain.remove(ri);
        actions.push(DockAction::ProjectMutated("Remove FX"));
    }
}

fn render_modulator_card(
    ui: &mut Ui,
    m: &mut ModulatorSlot,
    fx_list: &[(Uuid, String, Vec<String>)],
    actions: &mut Vec<DockAction>,
) -> bool {
    let mut remove = false;
    let mod_col = ACCENT_PURPLE;
    egui::Frame::new()
        .fill(Color32::from_rgb(28, 24, 38))
        .stroke(Stroke::new(1.2_f32, mod_col))
        .corner_radius(6)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(225.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if pill_toggle(ui, "MOD", m.enabled, mod_col).clicked() {
                        m.enabled = !m.enabled;
                        actions.push(DockAction::ProjectMutated("Toggle Modulator"));
                    }
                    ui.add(egui::TextEdit::singleline(&mut m.name).desired_width(115.0));
                    if ui.chrome_small_button("×").clicked() {
                        remove = true;
                    }
                });
                ui.separator();

                let mut ch = false;
                match &mut m.kind {
                    ModulatorKind::Lfo {
                        shape,
                        rate_hz,
                        tempo_sync,
                        sync_beats,
                        bipolar,
                    } => {
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt(("lfo_sh", m.id))
                                .selected_text(shape.label())
                                .width(88.0)
                                .show_ui(ui, |ui| {
                                    for &s in LfoShape::ALL {
                                        ch |= ui.selectable_value(shape, s, s.label()).changed();
                                    }
                                });
                            ch |= ui.checkbox(tempo_sync, "Sync").changed();
                            ch |= ui.checkbox(bipolar, "±").changed();
                        });
                        if *tempo_sync {
                            ch |= ui
                                .add(egui::Slider::new(sync_beats, 0.25..=16.0).text("Beats"))
                                .changed();
                        } else {
                            ch |= ui
                                .add(egui::Slider::new(rate_hz, 0.05..=18.0).text("Hz"))
                                .changed();
                        }
                    }
                    ModulatorKind::EnvelopeFollower {
                        attack_ms,
                        release_ms,
                        sensitivity_db,
                    } => {
                        ch |= ui
                            .add(egui::Slider::new(sensitivity_db, -12.0..=30.0).text("Sens dB"))
                            .changed();
                        ch |= ui
                            .add(egui::Slider::new(attack_ms, 1.0..=100.0).text("Atk ms"))
                            .changed();
                        ch |= ui
                            .add(egui::Slider::new(release_ms, 20.0..=600.0).text("Rel ms"))
                            .changed();
                    }
                    ModulatorKind::Macro { value } => {
                        ch |= ui
                            .add(egui::Slider::new(value, 0.0..=1.0).text("Macro"))
                            .changed();
                    }
                    ModulatorKind::StepSequencer {
                        steps, step_beats, ..
                    } => {
                        ch |= ui
                            .add(egui::Slider::new(step_beats, 0.125..=2.0).text("Step b"))
                            .changed();
                        ui.horizontal(|ui| {
                            for st in steps.iter_mut() {
                                ch |= ui
                                    .add(egui::DragValue::new(st).range(-1.0..=1.0).speed(0.05))
                                    .changed();
                            }
                        });
                    }
                }

                ui.menu_button("+ Assign Target Parameter", |ui| {
                    if ui.chrome_button("Track Volume").clicked() {
                        m.routings.push(ModRouting {
                            target: ModTarget::TrackGain,
                            depth: 0.25,
                        });
                        actions.push(DockAction::ProjectMutated("Assign Mod Target"));
                        ui.close_menu();
                    }
                    if ui.chrome_button("Track Pan").clicked() {
                        m.routings.push(ModRouting {
                            target: ModTarget::TrackPan,
                            depth: 0.5,
                        });
                        actions.push(DockAction::ProjectMutated("Assign Mod Target"));
                        ui.close_menu();
                    }
                    for (fx_id, badge, p_names) in fx_list {
                        ui.separator();
                        ui.label(egui::RichText::new(badge).strong());
                        for (pi, p_name) in p_names.iter().enumerate() {
                            if ui.chrome_button(format!("{} → {}", badge, p_name)).clicked() {
                                m.routings.push(ModRouting {
                                    target: ModTarget::FxParam {
                                        fx_id: *fx_id,
                                        param_index: pi,
                                    },
                                    depth: 0.30,
                                });
                                actions.push(DockAction::ProjectMutated("Assign Mod Target"));
                                ui.close_menu();
                            }
                        }
                    }
                });

                let mut del_r = None;
                for (ri, r) in m.routings.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let t_lbl = match r.target {
                            ModTarget::TrackGain => "Vol".to_string(),
                            ModTarget::TrackPan => "Pan".to_string(),
                            ModTarget::FxWet { .. } => "FX Wet".to_string(),
                            ModTarget::FxParam { fx_id, param_index } => fx_list
                                .iter()
                                .find(|(id, _, _)| *id == fx_id)
                                .and_then(|(_, b, names)| {
                                    names.get(param_index).map(|n| format!("{}:{}", b, n))
                                })
                                .unwrap_or_else(|| "Param".into()),
                        };
                        ui.label(egui::RichText::new(t_lbl).size(10.0).color(mod_col));
                        ch |= ui
                            .add(
                                egui::DragValue::new(&mut r.depth)
                                    .range(-1.0..=1.0)
                                    .speed(0.01)
                                    .prefix("±"),
                            )
                            .changed();
                        if ui.chrome_small_button("×").clicked() {
                            del_r = Some(ri);
                        }
                    });
                }
                if let Some(dr) = del_r {
                    m.routings.remove(dr);
                    ch = true;
                }
                if ch {
                    actions.push(DockAction::ProjectMutated("Adjust Modulator"));
                }
            });
        });
    remove
}

fn draw_vocal_pitch_display(
    ui: &mut Ui,
    params: &[f32],
    vp: &ferric_dsp::VocalPitchTelemetry,
) {
    let root = params.first().copied().unwrap_or(9.0).round().clamp(0.0, 11.0) as usize;
    let sc_idx = params.get(1).copied().unwrap_or(2.0).round().clamp(0.0, 7.0) as usize;
    let sc = MusicalScale::ALL
        .get(sc_idx)
        .copied()
        .unwrap_or(MusicalScale::NaturalMinor);

    let (rect, _) = ui.allocate_exact_size(Vec2::new(230.0, 48.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4, BG_DEEP);
    p.rect_stroke(
        rect,
        4,
        Stroke::new(1.0_f32, BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let key_str = format!("Key: {} {}", NOTE_PITCH_CLASSES[root], sc.label());
    p.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        key_str,
        FontId::proportional(10.0),
        ACCENT_TEAL,
    );

    let status = if vp.active {
        let tgt = vp.target_midi.round().clamp(0.0, 127.0) as usize;
        format!(
            "Target: {}{}  ({:+.0} ct shift)",
            NOTE_PITCH_CLASSES[tgt % 12],
            (tgt as i32 / 12) - 1,
            vp.correction_cents
        )
    } else {
        "Sing or play audio clip...".into()
    };
    p.text(
        Pos2::new(rect.left() + 8.0, rect.bottom() - 12.0),
        Align2::LEFT_CENTER,
        status,
        FontId::monospace(10.5),
        TEXT_MAIN,
    );
}

fn draw_eq_curve_mini(ui: &mut Ui, params: &[f32], telem: &EngineTelemetry) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(280.0, 56.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4, BG_DEEP);

    let bw = rect.width() / SPECTRUM_BANDS as f32;
    for b in 0..SPECTRUM_BANDS {
        let db = telem.master.spectrum_db[b];
        let norm = ((db + 84.0) / 90.0).clamp(0.0, 1.0);
        let h = norm * rect.height();
        let br = Rect::from_min_max(
            Pos2::new(rect.left() + b as f32 * bw, rect.bottom() - h),
            Pos2::new(rect.left() + (b + 1) as f32 * bw - 0.5, rect.bottom()),
        );
        p.rect_filled(br, 0, Color32::from_rgba_unmultiplied(66, 196, 176, 38));
    }

    p.line_segment(
        [
            Pos2::new(rect.left(), rect.center().y),
            Pos2::new(rect.right(), rect.center().y),
        ],
        Stroke::new(0.8_f32, BORDER_SUBTLE),
    );

    let sr = telem.sample_rate.max(44_100) as f32;
    let pts: Vec<Pos2> = (0..=80)
        .map(|i| {
            let t = i as f32 / 80.0;
            let freq = 20.0 * (1000.0_f32).powf(t);
            let db = ParametricEqProcessor::response_db_at(sr, params, freq).clamp(-18.0, 18.0);
            let y = rect.center().y - (db / 18.0) * (rect.height() * 0.46);
            Pos2::new(rect.left() + t * rect.width(), y)
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.8_f32, ACCENT_TEAL)));
}

fn draw_strobe_tuner_display(ui: &mut Ui, reading: &ferric_dsp::TunerReading) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(185.0, 52.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4, BG_DEEP);
    p.rect_stroke(
        rect,
        4,
        Stroke::new(1.0_f32, BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let center_x = rect.center().x;
    p.line_segment(
        [
            Pos2::new(center_x, rect.top() + 4.0),
            Pos2::new(center_x, rect.bottom() - 4.0),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(70, 76, 90)),
    );

    if reading.active {
        let in_tune = reading.cents.abs() < 4.0;
        let col = if in_tune {
            ACCENT_TEAL
        } else if reading.cents.abs() < 15.0 {
            ACCENT_AMBER
        } else {
            ACCENT_RED
        };
        let needle_x = center_x + (reading.cents / 50.0) * (rect.width() * 0.42);
        p.circle_filled(Pos2::new(needle_x, rect.bottom() - 12.0), 5.0, col);
        p.text(
            Pos2::new(center_x, rect.top() + 16.0),
            Align2::CENTER_CENTER,
            format!(
                "{}  ({:+.1} ct • {:.1} Hz)",
                reading.note_label(),
                reading.cents,
                reading.freq_hz
            ),
            FontId::monospace(12.0),
            col,
        );
    } else {
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "TUNER READY — Play Note",
            FontId::monospace(10.5),
            TEXT_DIM,
        );
    }
}

fn draw_live_spectrum_card(ui: &mut Ui, telem: &EngineTelemetry) {
    egui::Frame::new()
        .fill(BG_ELEVATED)
        .stroke(Stroke::new(1.0_f32, ACCENT_TEAL.gamma_multiply(0.5)))
        .corner_radius(6)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("64-Band FFT + 2D Lissajous Vectorscope")
                            .size(11.5)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "M: {:.1} LUFS | Corr: {:+.2} | PDC: {} smp",
                            telem.master.momentary_lufs,
                            telem.master.phase_correlation,
                            telem.total_pdc_samples
                        ))
                        .size(10.0)
                        .monospace()
                        .color(ACCENT_TEAL),
                    );
                });
                ui.horizontal(|ui| {
                    // 1. 64-Band Logarithmic FFT Analyzer
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(255.0, 125.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(rect, 4, BG_DEEP);
                    let bw = rect.width() / SPECTRUM_BANDS as f32;
                    for b in 0..SPECTRUM_BANDS {
                        let db = telem.master.spectrum_db[b];
                        let norm = ((db + 84.0) / 90.0).clamp(0.0, 1.0);
                        let h = norm * (rect.height() - 6.0);
                        let br = Rect::from_min_max(
                            Pos2::new(rect.left() + b as f32 * bw + 0.5, rect.bottom() - h),
                            Pos2::new(rect.left() + (b + 1) as f32 * bw - 0.5, rect.bottom()),
                        );
                        let col = Color32::from_rgb(
                            (45.0 + norm * 185.0) as u8,
                            (180.0 - norm * 50.0) as u8,
                            (175.0 - norm * 90.0) as u8,
                        );
                        p.rect_filled(br, 1, col);
                    }

                    // 2. 2D Lissajous Stereo Goniometer / Vectorscope!
                    let (vrect, _) =
                        ui.allocate_exact_size(Vec2::new(125.0, 125.0), Sense::hover());
                    let vp = ui.painter();
                    vp.rect_filled(vrect, 4, BG_DEEP);
                    vp.rect_stroke(
                        vrect,
                        4,
                        Stroke::new(1.0_f32, BORDER_SUBTLE),
                        StrokeKind::Inside,
                    );
                    let vc = vrect.center();
                    let rad = vrect.width() * 0.44;
                    vp.circle_stroke(vc, rad, Stroke::new(0.8_f32, Color32::from_rgb(32, 36, 46)));
                    // L / R diagonal axes + Mid vertical axis
                    vp.line_segment(
                        [Pos2::new(vc.x, vrect.top() + 4.0), Pos2::new(vc.x, vrect.bottom() - 4.0)],
                        Stroke::new(0.8_f32, Color32::from_rgb(38, 42, 54)),
                    );
                    vp.line_segment(
                        [
                            Pos2::new(vc.x - rad * 0.7, vc.y - rad * 0.7),
                            Pos2::new(vc.x + rad * 0.7, vc.y + rad * 0.7),
                        ],
                        Stroke::new(0.6_f32, Color32::from_rgb(34, 38, 48)),
                    );
                    vp.line_segment(
                        [
                            Pos2::new(vc.x + rad * 0.7, vc.y - rad * 0.7),
                            Pos2::new(vc.x - rad * 0.7, vc.y + rad * 0.7),
                        ],
                        Stroke::new(0.6_f32, Color32::from_rgb(34, 38, 48)),
                    );

                    let pts: Vec<Pos2> = telem
                        .master
                        .vectorscope_xy
                        .iter()
                        .map(|&(sx, my)| Pos2::new(vc.x + sx * rad, vc.y - my * rad))
                        .collect();
                    vp.add(egui::Shape::line(
                        pts,
                        Stroke::new(1.3_f32, ACCENT_TEAL.gamma_multiply(0.85)),
                    ));
                    vp.text(
                        Pos2::new(vc.x, vrect.top() + 8.0),
                        Align2::CENTER_CENTER,
                        "M",
                        FontId::monospace(8.5),
                        TEXT_DIM,
                    );
                });
            });
        });
}

fn draw_wavetable_3d_scope(
    ui: &mut Ui,
    fx: &mut FxSlot,
    sample_rate: u32,
    actions: &mut Vec<DockAction>,
) {
    let bank = fx.param(0).round().clamp(0.0, 3.0) as usize;
    let morph_norm = (fx.param(1) * 0.01).clamp(0.0, 1.0);
    let bank_name = match bank {
        0 => "0: Analog Morph",
        1 => "1: Vocal Formant",
        2 => "2: Digital FM Bell",
        _ => "3: Custom User WAV",
    };

    ui.horizontal(|ui| {
        // 3D Stacked Wavetable Isometric Scope!
        let (rect, _) = ui.allocate_exact_size(Vec2::new(225.0, 58.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 4, BG_DEEP);
        p.rect_stroke(
            rect,
            4,
            Stroke::new(1.0_f32, BORDER_SUBTLE),
            StrokeKind::Inside,
        );

        let mut wave = [0.0_f32; 128];
        // Draw 5 background depth slices for Serum-style 3D perspective
        for slice in 0..5 {
            let sl_norm = slice as f32 / 4.0;
            WavetableSynthProcessor::preview_waveform_static(bank, sl_norm, &mut wave);
            let y_base = rect.bottom() - 10.0 - slice as f32 * 7.5;
            let x_off = slice as f32 * 4.0;
            let w_span = rect.width() - 24.0;
            let pts: Vec<Pos2> = (0..64)
                .map(|k| {
                    let u = k as f32 / 63.0;
                    Pos2::new(
                        rect.left() + 4.0 + x_off + u * w_span,
                        y_base - wave[k * 2] * 9.5,
                    )
                })
                .collect();
            p.add(egui::Shape::line(
                pts,
                Stroke::new(0.8_f32, Color32::from_rgba_unmultiplied(90, 105, 135, 70)),
            ));
        }

        // Draw bright foreground active morphed waveform!
        WavetableSynthProcessor::preview_waveform_static(bank, morph_norm, &mut wave);
        let fg_y = rect.bottom() - 10.0 - morph_norm * 30.0;
        let fg_x = morph_norm * 16.0;
        let w_span = rect.width() - 24.0;
        let fg_pts: Vec<Pos2> = (0..128)
            .map(|k| {
                let u = k as f32 / 127.0;
                Pos2::new(
                    rect.left() + 4.0 + fg_x + u * w_span,
                    (fg_y - wave[k] * 11.5).clamp(rect.top() + 2.0, rect.bottom() - 2.0),
                )
            })
            .collect();
        p.add(egui::Shape::line(fg_pts, Stroke::new(1.8_f32, ACCENT_TEAL)));

        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(bank_name)
                    .size(10.5)
                    .strong()
                    .color(ACCENT_TEAL),
            );
            if ui
                .chrome_small_button("📂 Import WAV as Wavetable...")
                .on_hover_text(
                    "Convert any .WAV/.FLAC into a 64-frame custom wavetable",
                )
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Audio", &["wav", "flac", "mp3", "ogg"])
                    .pick_file()
                {
                    if let Ok(buf) = import_audio_file(&path, sample_rate) {
                        fx.preset_name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Custom WT")
                            .to_string();
                        fx.sampler_buffer = Some(buf);
                        fx.set_param(0, 3.0); // Switch to Bank 3 (Custom WAV)!
                        actions.push(DockAction::ReloadSamplerFx(fx.clone()));
                        actions.push(DockAction::ProjectMutated("Import Custom Wavetable"));
                    }
                }
            }
        });
    });
}

fn draw_convolution_ir_header(
    ui: &mut Ui,
    fx: &mut FxSlot,
    sample_rate: u32,
    actions: &mut Vec<DockAction>,
) {
    let mode = fx.param(0).round().clamp(0.0, 4.0) as i32;
    let space_lbl = match mode {
        0 => "0: Symphony Concert Hall IR",
        1 => "1: Vintage EMT 140 Plate IR",
        2 => "2: Studio Spring Tank IR",
        3 => "3: 4x12 Celestion V30 Cab IR",
        _ => "4: Custom Loaded WAV/FLAC IR",
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(space_lbl)
                .size(10.5)
                .strong()
                .color(ACCENT_AMBER),
        );
        if ui
            .chrome_small_button("📂 Load Custom IR (.wav/.flac)...")
            .on_hover_text(
                "Load any Acoustic Room/Hall Impulse Response or Guitar Speaker Cabinet IR!",
            )
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Impulse Response", &["wav", "flac", "aiff"])
                .pick_file()
            {
                if let Ok(buf) = import_audio_file(&path, sample_rate) {
                    fx.preset_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Custom IR")
                        .to_string();
                    fx.sampler_buffer = Some(buf);
                    fx.set_param(0, 4.0); // Switch to Custom IR slot!
                    actions.push(DockAction::ReloadSamplerFx(fx.clone()));
                    actions.push(DockAction::ProjectMutated("Load Custom Convolution IR"));
                }
            }
        }
    });
}

fn draw_ott_multiband_visualizer(ui: &mut Ui, params: &[f32], telem: &EngineTelemetry) {
    let depth = (params.first().copied().unwrap_or(55.0) * 0.01).clamp(0.0, 1.0);
    let low_x = params.get(2).copied().unwrap_or(120.0);
    let hi_x = params.get(3).copied().unwrap_or(2500.0);
    let low_g = params.get(4).copied().unwrap_or(0.5);
    let mid_g = params.get(5).copied().unwrap_or(0.0);
    let hi_g = params.get(6).copied().unwrap_or(1.5);

    let (rect, _) = ui.allocate_exact_size(Vec2::new(265.0, 48.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4, BG_DEEP);
    p.rect_stroke(
        rect,
        4,
        Stroke::new(1.0_f32, BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    // Compute live Low / Mid / High energy from 32-band analyzer for animated OTT band bars
    let avg_slice = |s: &[f32]| -> f32 {
        if s.is_empty() {
            -72.0
        } else {
            s.iter().sum::<f32>() / s.len() as f32
        }
    };
    let e_low = avg_slice(&telem.master.spectrum_db[0..7]) + low_g;
    let e_mid = avg_slice(&telem.master.spectrum_db[7..19]) + mid_g;
    let e_hi = avg_slice(&telem.master.spectrum_db[19..32]) + hi_g;

    let bands = [
        (format!("LOW <{:.0}Hz", low_x), e_low, ACCENT_FERRIC),
        ("MID".to_string(), e_mid, ACCENT_TEAL),
        (format!("HIGH >{:.1}k", hi_x * 0.001), e_hi, ACCENT_AMBER),
    ];
    let row_h = (rect.height() - 8.0) / 3.0;
    for (idx, (lbl, db, col)) in bands.iter().enumerate() {
        let y0 = rect.top() + 4.0 + idx as f32 * row_h;
        let y1 = y0 + row_h - 2.0;
        let norm = ((*db + 72.0) / 72.0).clamp(0.06, 1.0);
        // OTT pulls quiet signals UP toward ceiling and clamps hot peaks DOWN
        let ott_norm = norm + (0.72 - norm) * (depth * 0.65);
        let bar_l = rect.left() + 74.0;
        let bar_max_w = rect.width() - 80.0;
        p.text(
            Pos2::new(rect.left() + 5.0, 0.5 * (y0 + y1)),
            Align2::LEFT_CENTER,
            lbl,
            FontId::monospace(8.5),
            TEXT_DIM,
        );
        // Target OTT threshold window
        let win_l = bar_l + bar_max_w * (0.48 - 0.18 * (1.0 - depth));
        let win_r = bar_l + bar_max_w * (0.82 + 0.10 * (1.0 - depth));
        p.rect_filled(
            Rect::from_min_max(Pos2::new(win_l, y0), Pos2::new(win_r, y1)),
            2,
            Color32::from_rgba_unmultiplied(255, 255, 255, 18),
        );
        let fill_r = bar_l + bar_max_w * ott_norm.clamp(0.05, 1.0);
        p.rect_filled(
            Rect::from_min_max(Pos2::new(bar_l, y0 + 1.5), Pos2::new(fill_r, y1 - 1.5)),
            2,
            col.gamma_multiply(0.85),
        );
    }
}
