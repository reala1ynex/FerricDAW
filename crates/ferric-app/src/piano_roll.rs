use crate::theme::{
    pill_toggle, studio_knob, track_color32, ACCENT_AMBER, ACCENT_FERRIC, ACCENT_RED, ACCENT_TEAL,
    BG_DEEP, BG_ELEVATED, BORDER_SUBTLE, TEXT_DIM, TEXT_MAIN,
};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use ferric_dsp::{ParametricEqProcessor, SPECTRUM_BANDS};
use ferric_engine::{import_audio_file, EngineTelemetry};
use ferric_model::{
    export_smf_clip, AutomationTarget, ChordStamp, ClipId, FxKind, FxSlot, LfoShape, MidiCcPoint,
    MidiCcTarget, MidiNote, ModRouting, ModTarget, ModulatorKind, ModulatorSlot, MusicalScale,
    Project, TakeContent, TimeStretchMode, TrackId, NOTE_PITCH_CLASSES,
};
use uuid::Uuid;

pub enum DockAction {
    ProjectMutated(&'static str),
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
}

pub struct PianoRollState {
    pub zoom_x: f32,
    pub lowest_pitch: u8,
    pub visible_semitones: u8,
    pub default_note_len: f64,
    pub scale_lock: bool,
    pub drum_step_view: bool,
    pub drum_bar_index: usize,
    pub chord_stamp: ChordStamp,
    pub active_cc_lane: Option<MidiCcTarget>,
    drag_note: Option<(Uuid, bool)>,
    drag_warp: Option<Uuid>,
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
            chord_stamp: ChordStamp::SingleNote,
            active_cc_lane: None,
            drag_note: None,
            drag_warp: None,
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
                ui.label(
                    egui::RichText::new(
                        "Select or double-click any Clip on the timeline to edit MIDI Notes, Scales/Chords, CC Lanes, WSOLA Warp Markers & Per-Clip FX.",
                    )
                    .color(TEXT_DIM),
                );
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
            self.render_audio_clip_inspector(ui, project, track_id, clip_id, actions);
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

        // Top Pro MIDI Toolbar: 16-Step Drum Grid toggle, Scale Lock, Chord Stamp, Quantize, Humanize, CC Lane, SMF Export!
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if let Some(clip) = project
                .find_track_mut(track_id)
                .and_then(|t| t.find_clip_mut(clip_id))
            {
                ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(105.0));
            }

            if pill_toggle(ui, "🥁 16-Step Grid", self.drum_step_view, ACCENT_AMBER)
                .on_hover_text("Toggle TR-808 / DS-10 Style 16-Step Drum Pattern Sequencer!")
                .clicked()
            {
                self.drum_step_view = !self.drum_step_view;
            }

            if pill_toggle(ui, "Scale Lock", self.scale_lock, ACCENT_TEAL)
                .on_hover_text("Highlight & snap drawn notes to Session Key & Scale")
                .clicked()
            {
                self.scale_lock = !self.scale_lock;
            }
            egui::ComboBox::from_id_salt("pr_root")
                .selected_text(NOTE_PITCH_CLASSES[(project.root_pitch_class as usize) % 12])
                .width(48.0)
                .show_ui(ui, |ui| {
                    for (i, &nm) in NOTE_PITCH_CLASSES.iter().enumerate() {
                        ui.selectable_value(&mut project.root_pitch_class, i as u8, nm);
                    }
                });
            egui::ComboBox::from_id_salt("pr_scale")
                .selected_text(project.musical_scale.label())
                .width(128.0)
                .show_ui(ui, |ui| {
                    for &sc in MusicalScale::ALL {
                        ui.selectable_value(&mut project.musical_scale, sc, sc.label());
                    }
                });

            egui::ComboBox::from_id_salt("pr_chord_stamp")
                .selected_text(self.chord_stamp.label())
                .width(115.0)
                .show_ui(ui, |ui| {
                    for &cs in ChordStamp::ALL {
                        ui.selectable_value(&mut self.chord_stamp, cs, cs.label());
                    }
                });

            if ui.small_button("Q 1/16").clicked() {
                project.quantize_midi_clip(track_id, clip_id, 0.25);
                actions.push(DockAction::ProjectMutated("Quantize 1/16"));
            }
            if ui
                .small_button("Humanize")
                .on_hover_text("Add subtle organic studio timing & velocity groove")
                .clicked()
            {
                project.humanize_midi_clip(track_id, clip_id);
                actions.push(DockAction::ProjectMutated("Humanize MIDI"));
            }

            // Velocity / CC Lane Selector
            let lane_lbl = self
                .active_cc_lane
                .map(|c| c.label())
                .unwrap_or("Velocity Stalks");
            egui::ComboBox::from_id_salt("pr_cc_lane_sel")
                .selected_text(lane_lbl)
                .width(115.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.active_cc_lane, None, "Velocity Stalks");
                    for &tgt in MidiCcTarget::ALL {
                        ui.selectable_value(&mut self.active_cc_lane, Some(tgt), tgt.label());
                    }
                });

            if ui
                .small_button("💾 .MID")
                .on_hover_text("Export Clip as Standard MIDI File (.mid)")
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
                            let _ = export_smf_clip(&path, project.bpm, notes, cc_points);
                        }
                    }
                }
            }

            if ui.small_button("▼Oct").clicked() {
                self.lowest_pitch = self.lowest_pitch.saturating_sub(12).max(12);
            }
            if ui.small_button("▲Oct").clicked() {
                self.lowest_pitch = (self.lowest_pitch + 12).min(84);
            }
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

        egui::ScrollArea::horizontal()
            .id_salt("piano_roll_hscroll")
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

                let mut delete_note_id = None;
                let mut clicked_existing = false;

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
                    let vel_alpha = (note.velocity as f32 / 127.0).clamp(0.35, 1.0);
                    ui.painter()
                        .rect_filled(nrect, 2, t_col.gamma_multiply(vel_alpha));
                    ui.painter().rect_stroke(
                        nrect,
                        2,
                        Stroke::new(1.0_f32, Color32::WHITE.gamma_multiply(0.7)),
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

                    if n_resp.secondary_clicked() {
                        delete_note_id = Some(note.id);
                        clicked_existing = true;
                    } else if n_resp.clicked() {
                        clicked_existing = true;
                    } else if n_resp.drag_started() {
                        clicked_existing = true;
                        let near_right = n_resp
                            .interact_pointer_pos()
                            .map(|p| (nrect.right() - p.x).abs() < 7.0)
                            .unwrap_or(false);
                        self.drag_note = Some((note.id, near_right));
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
                                notes[n_idx].start_beat =
                                    (notes[n_idx].start_beat + (d.x / self.zoom_x) as f64).max(0.0);
                                if d.y.abs() >= row_h * 0.85 {
                                    let dp = (-d.y / row_h).round() as i32;
                                    let cand =
                                        (notes[n_idx].pitch as i32 + dp).clamp(0, 127) as u8;
                                    notes[n_idx].pitch = if self.scale_lock {
                                        scale.snap_pitch(root_pc, cand)
                                    } else {
                                        cand
                                    };
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

                if let Some(del_id) = delete_note_id {
                    notes.retain(|n| n.id != del_id);
                    actions.push(DockAction::ProjectMutated("Delete MIDI Note"));
                }

                if self.drag_note.is_some() && !ui.input(|i| i.pointer.any_down()) {
                    self.drag_note = None;
                }

                // Click empty grid cell -> Insert Single Note OR Full Chord Stamp (Scale-Aware)!
                let grid_bg_resp =
                    ui.interact(grid_rect, ui.id().with("midi_grid_bg"), Sense::click());
                if grid_bg_resp.clicked() && !clicked_existing {
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

                        for &semi_offset in self.chord_stamp.intervals() {
                            let p = (base_pitch.saturating_add(semi_offset)).min(127);
                            notes.push(MidiNote::new(p, 102, start_b, self.default_note_len));
                        }
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
    }

    fn render_audio_clip_inspector(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        track_id: TrackId,
        clip_id: ClipId,
        actions: &mut Vec<DockAction>,
    ) {
        let t_col = project
            .find_track(track_id)
            .map(|t| track_color32(t.color))
            .unwrap_or(ACCENT_TEAL);
        let bpm = project.bpm;

        let Some(clip) = project
            .find_track_mut(track_id)
            .and_then(|t| t.find_clip_mut(clip_id))
        else {
            return;
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(130.0));
            let mut ch = false;

            egui::ComboBox::from_id_salt("stretch_algo_combo")
                .selected_text(clip.stretch_mode.label())
                .width(138.0)
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
            ch |= ui
                .add(
                    egui::DragValue::new(&mut clip.gain_db)
                        .range(-36.0..=18.0)
                        .speed(0.15)
                        .prefix("Gain ")
                        .suffix(" dB"),
                )
                .changed();

            if ui
                .small_button("⚡ Auto-Warp Transients")
                .on_hover_text("Detect drum/guitar transients & place draggable WSOLA Stretch Markers!")
                .clicked()
            {
                clip.generate_transient_warp_markers(bpm);
                ch = true;
            }
            if !clip.warp_markers.is_empty() && ui.small_button("Clear Warps").clicked() {
                clip.warp_markers.clear();
                ch = true;
            }

            ui.menu_button("+ Clip FX", |ui| {
                for &kind in FxKind::ALL {
                    if !kind.is_instrument() && ui.button(kind.display_name()).clicked() {
                        clip.clip_fx.push(FxSlot::new(kind));
                        actions.push(DockAction::ProjectMutated("Add Per-Clip FX"));
                        ui.close_menu();
                    }
                }
            });

            if ch {
                actions.push(DockAction::ProjectMutated("Adjust Audio Warp/Clip"));
            }
        });

        let wave_h = (ui.available_height() - 38.0).max(88.0);
        let (wrect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), wave_h), Sense::hover());
        ui.painter().rect_filled(wrect, 4, BG_DEEP);
        ui.painter().rect_stroke(
            wrect,
            4,
            Stroke::new(1.0_f32, BORDER_SUBTLE),
            StrokeKind::Inside,
        );

        let cols = (wrect.width() as usize).clamp(32, 1200);
        let mid_y = wrect.center().y;
        let half_h = wrect.height() * 0.45;
        for px in 0..cols {
            let frac = px as f64 / cols as f64;
            let local_b = frac * clip.length_beats;
            let take_idx = clip.take_at_local_beat(local_b);
            if let Some(TakeContent::Audio { buffer }) = clip.takes.get(take_idx).map(|t| &t.content)
            {
                if !buffer.peaks.is_empty() {
                    let pk_i =
                        ((frac * buffer.peaks.len() as f64) as usize).min(buffer.peaks.len() - 1);
                    let (mn, mx) = buffer.peaks[pk_i];
                    let env = clip.gain_envelope_at(local_b).min(2.5);
                    let x = wrect.left() + px as f32;
                    let y0 = (mid_y - mx * env * half_h).clamp(wrect.top(), wrect.bottom());
                    let y1 = (mid_y - mn * env * half_h).clamp(wrect.top(), wrect.bottom());
                    let seg_col = match take_idx % 3 {
                        0 => t_col,
                        1 => ACCENT_TEAL,
                        _ => Color32::from_rgb(186, 108, 226),
                    };
                    ui.painter().line_segment(
                        [Pos2::new(x, y0), Pos2::new(x, y1)],
                        Stroke::new(1.2_f32, seg_col),
                    );
                }
            }
        }

        // Interactive Draggable WSOLA Stretch/Warp Markers right on the waveform!
        for m_idx in 0..clip.warp_markers.len() {
            let wm = clip.warp_markers[m_idx];
            let wx = wrect.left() + (wm.clip_beat / clip.length_beats.max(0.25)) as f32 * wrect.width();
            if wx >= wrect.left() && wx <= wrect.right() {
                ui.painter().line_segment(
                    [Pos2::new(wx, wrect.top()), Pos2::new(wx, wrect.bottom())],
                    Stroke::new(1.4_f32, ACCENT_AMBER),
                );
                let handle = Rect::from_center_size(Pos2::new(wx, wrect.top() + 8.0), Vec2::new(12.0, 14.0));
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
                    let db = (h_resp.drag_delta().x / wrect.width()) as f64 * clip.length_beats;
                    clip.warp_markers[m_idx].clip_beat =
                        (clip.warp_markers[m_idx].clip_beat + db).clamp(0.0, clip.length_beats);
                    actions.push(DockAction::ProjectMutated("Drag Warp Marker"));
                }
            }
        }
        if self.drag_warp.is_some() && !ui.input(|i| i.pointer.any_down()) {
            self.drag_warp = None;
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
            if !clip.comp_segments.is_empty() && ui.small_button("Reset Swipe Comp").clicked() {
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
    telem: &EngineTelemetry,
    actions: &mut Vec<DockAction>,
) {
    let all_tracks: Vec<(TrackId, String)> = project
        .tracks
        .iter()
        .map(|t| (t.id, t.name.clone()))
        .collect();

    ui.horizontal(|ui| {
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
                        && ui.button(kind.display_name()).clicked()
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
                    if kind != FxKind::ExternalClap && ui.button(kind.display_name()).clicked() {
                        if let Some(trk) = project.find_track_mut(tid) {
                            trk.fx_chain.push(FxSlot::new(kind));
                            actions.push(DockAction::ProjectMutated("Add Track FX"));
                        }
                        ui.close_menu();
                    }
                }
            });
            ui.menu_button("+ Add Modulator", |ui| {
                if ui.button("🌊 Tempo-Synced LFO").clicked() {
                    if let Some(trk) = project.find_track_mut(tid) {
                        trk.modulators.push(ModulatorSlot::new_lfo("LFO"));
                        actions.push(DockAction::ProjectMutated("Add LFO Modulator"));
                    }
                    ui.close_menu();
                }
                if ui.button("📈 Audio Envelope Follower").clicked() {
                    if let Some(trk) = project.find_track_mut(tid) {
                        trk.modulators
                            .push(ModulatorSlot::new_env_follower("Env Follower"));
                        actions.push(DockAction::ProjectMutated("Add Envelope Follower"));
                    }
                    ui.close_menu();
                }
                if ui.button("🎛 Macro Knob").clicked() {
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
                        "Enable Tempo-Synced MIDI Arpeggiator / Chord Gate (works live with Akai LPK25 & clips!)",
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
                        ACCENT_TEAL,
                        telem,
                        actions,
                    );
                } else if let Some(tid) = selected_track {
                    let Some(trk) = project.find_track_mut(tid) else {
                        return;
                    };
                    let t_col = track_color32(trk.color);

                    let fx_snapshot: Vec<(Uuid, FxKind)> =
                        trk.fx_chain.iter().map(|f| (f.id, f.kind)).collect();
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
                        t_col,
                        telem,
                        actions,
                    );
                }
            });
        });
}

fn render_fx_slot_cards(
    ui: &mut Ui,
    owner_track: Option<TrackId>,
    all_tracks: &[(TrackId, String)],
    chain: &mut Vec<FxSlot>,
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
                        ui.label(
                            egui::RichText::new(title)
                                .size(11.5)
                                .strong()
                                .color(TEXT_MAIN),
                        );
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

                        if idx > 0 && ui.small_button("◀").clicked() {
                            move_swap = Some((idx, idx - 1));
                        }
                        if idx + 1 < len && ui.small_button("▶").clicked() {
                            move_swap = Some((idx, idx + 1));
                        }
                        if ui.small_button("×").clicked() {
                            remove_idx = Some(idx);
                        }
                    });

                    ui.separator();

                    // Special visualizers per FX type
                    match fx.kind {
                        FxKind::ParametricEq => draw_eq_curve_mini(ui, &fx.params, telem),
                        FxKind::ChromaticTuner => draw_strobe_tuner_display(ui, &telem.tuner),
                        FxKind::VocalPitch => draw_vocal_pitch_display(ui, &fx.params, &telem.vocal_pitch),
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
                                if ui.small_button("📂 Load Audio...").clicked() {
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

                    let specs = fx.kind.param_specs();
                    if fx.params.len() < specs.len() {
                        fx.params.resize_with(specs.len(), || 0.0);
                    }
                    for chunk_start in (0..specs.len()).step_by(6) {
                        ui.horizontal(|ui| {
                            let end = (chunk_start + 6).min(specs.len());
                            for p_i in chunk_start..end {
                                let sp = specs[p_i];
                                ui.vertical(|ui| {
                                    if studio_knob(
                                        ui,
                                        sp.name,
                                        &mut fx.params[p_i],
                                        sp.min,
                                        sp.max,
                                        sp.default,
                                        sp.unit,
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
                                                .min_size(Vec2::new(34.0, 13.0)),
                                            )
                                            .on_hover_text(
                                                "1-Click MIDI CC Learn (or Show Automation Lane)",
                                            )
                                            .clicked()
                                        {
                                            actions.push(DockAction::ArmMidiLearn {
                                                track_id: tid,
                                                target: AutomationTarget::FxParam {
                                                    fx_id: fx.id,
                                                    param_index: p_i,
                                                },
                                                label: format!(
                                                    "{} → {}",
                                                    fx.kind.short_badge(),
                                                    sp.name
                                                ),
                                            });
                                        }
                                    }
                                });
                            }
                        });
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
    fx_list: &[(Uuid, FxKind)],
    actions: &mut Vec<DockAction>,
) -> bool {
    let mut remove = false;
    let mod_col = Color32::from_rgb(186, 108, 226);
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
                    if ui.small_button("×").clicked() {
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
                    if ui.button("Track Volume").clicked() {
                        m.routings.push(ModRouting {
                            target: ModTarget::TrackGain,
                            depth: 0.25,
                        });
                        actions.push(DockAction::ProjectMutated("Assign Mod Target"));
                        ui.close_menu();
                    }
                    if ui.button("Track Pan").clicked() {
                        m.routings.push(ModRouting {
                            target: ModTarget::TrackPan,
                            depth: 0.5,
                        });
                        actions.push(DockAction::ProjectMutated("Assign Mod Target"));
                        ui.close_menu();
                    }
                    for &(fx_id, kind) in fx_list {
                        ui.separator();
                        ui.label(egui::RichText::new(kind.short_badge()).strong());
                        for (pi, sp) in kind.param_specs().iter().enumerate() {
                            if ui
                                .button(format!("{} → {}", kind.short_badge(), sp.name))
                                .clicked()
                            {
                                m.routings.push(ModRouting {
                                    target: ModTarget::FxParam {
                                        fx_id,
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
                                .find(|(id, _)| *id == fx_id)
                                .and_then(|(_, k)| {
                                    k.param_specs()
                                        .get(param_index)
                                        .map(|s| format!("{}:{}", k.short_badge(), s.name))
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
                        if ui.small_button("×").clicked() {
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
                        egui::RichText::new("64-Band FFT + BS.1770 Loudness")
                            .size(11.5)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "M: {:.1} LUFS  |  S: {:.1} LUFS  |  Corr: {:+.2}  |  PDC: {} smp",
                            telem.master.momentary_lufs,
                            telem.master.short_term_lufs,
                            telem.master.phase_correlation,
                            telem.total_pdc_samples
                        ))
                        .size(10.5)
                        .monospace()
                        .color(ACCENT_TEAL),
                    );
                });
                let (rect, _) = ui.allocate_exact_size(Vec2::new(310.0, 125.0), Sense::hover());
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
            });
        });
}
