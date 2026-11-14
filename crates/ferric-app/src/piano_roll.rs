use crate::theme::{
    pill_toggle, studio_knob, track_color32, ACCENT_AMBER, ACCENT_FERRIC, ACCENT_RED, ACCENT_TEAL,
    BG_DEEP, BG_ELEVATED, BORDER_SUBTLE, TEXT_DIM, TEXT_MAIN,
};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use ferric_dsp::{ParametricEqProcessor, SPECTRUM_BANDS};
use ferric_engine::EngineTelemetry;
use ferric_model::{
    ClipId, FadeShape, FxKind, FxSlot, LfoShape, MidiNote, ModRouting, ModTarget, ModulatorKind,
    ModulatorSlot, Project, TakeContent, TrackId,
};
use uuid::Uuid;

pub enum DockAction {
    ProjectMutated(&'static str),
    AuditionMidi {
        track_id: TrackId,
        note: u8,
        is_note_on: bool,
    },
}

pub struct PianoRollState {
    pub zoom_x: f32,
    pub lowest_pitch: u8,
    pub visible_semitones: u8,
    pub default_note_len: f64,
    drag_note: Option<(Uuid, bool)>, // (note_id, resize_right)
    auditioning_pitch: Option<u8>,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            zoom_x: 56.0,
            lowest_pitch: 36, // C2
            visible_semitones: 36,
            default_note_len: 0.5,
            drag_note: None,
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
                        "Select or double-click any Clip on the timeline to edit MIDI Notes, Audio Waveforms, Fades & Per-Clip FX.",
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
            Self::render_audio_clip_inspector(ui, project, track_id, clip_id, actions);
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

        // Top toolbar: Clip Name, Quantize, Octave Scroll, Note Length, Per-Clip Gain/Transpose
        ui.horizontal(|ui| {
            if let Some(clip) = project
                .find_track_mut(track_id)
                .and_then(|t| t.find_clip_mut(clip_id))
            {
                ui.label(egui::RichText::new("🎹").size(14.0));
                ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(140.0));
                ui.separator();
                let r_pitch = ui.add(
                    egui::DragValue::new(&mut clip.pitch_semitones)
                        .range(-24.0..=24.0)
                        .speed(0.2)
                        .prefix("Transpose ")
                        .suffix(" st"),
                );
                if r_pitch.changed() {
                    actions.push(DockAction::ProjectMutated("Transpose Clip"));
                }
            }

            if ui.button("Quantize 1/16").clicked() {
                project.quantize_midi_clip(track_id, clip_id, 0.25);
                actions.push(DockAction::ProjectMutated("Quantize 1/16"));
            }
            if ui.button("Quantize 1/8").clicked() {
                project.quantize_midi_clip(track_id, clip_id, 0.5);
                actions.push(DockAction::ProjectMutated("Quantize 1/8"));
            }
            ui.separator();
            ui.label(egui::RichText::new("Octave:").size(11.0).color(TEXT_DIM));
            if ui.small_button("▼").clicked() {
                self.lowest_pitch = self.lowest_pitch.saturating_sub(12).max(12);
            }
            if ui.small_button("▲").clicked() {
                self.lowest_pitch = (self.lowest_pitch + 12).min(84);
            }
            ui.label(egui::RichText::new("Grid Zoom:").size(11.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.zoom_x, 24.0..=140.0).show_value(false));
            ui.label(
                egui::RichText::new("Left-click: Add/Move Note • Right-click: Delete Note")
                    .size(10.5)
                    .color(TEXT_DIM),
            );
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

        egui::ScrollArea::horizontal()
            .id_salt("piano_roll_hscroll")
            .show(ui, |ui| {
                let keys_w = 48.0_f32;
                let total_w = keys_w + (clip_len as f32 * self.zoom_x).max(ui.available_width());
                let avail_h = ui.available_height().max(150.0);
                let vel_h = 34.0_f32;
                let grid_h = (avail_h - vel_h - 4.0).max(108.0);
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

                // Piano Keyboard Audition Strip
                let keys_resp = ui.interact(
                    keys_rect,
                    ui.id().with("piano_keys_strip"),
                    Sense::click_and_drag(),
                );
                for r in 0..rows {
                    let pitch = self.lowest_pitch + (rows - 1 - r) as u8;
                    let y0 = keys_rect.top() + r as f32 * row_h;
                    let kr = Rect::from_min_size(Pos2::new(keys_rect.left(), y0), Vec2::new(keys_w, row_h));
                    let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                    let key_col = if self.auditioning_pitch == Some(pitch) {
                        t_col
                    } else if is_black {
                        Color32::from_rgb(24, 26, 32)
                    } else {
                        Color32::from_rgb(205, 210, 220)
                    };
                    ui.painter().rect_filled(kr.shrink(0.4), 1, key_col);
                    if pitch % 12 == 0 {
                        let oct = (pitch as i32 / 12) - 1;
                        ui.painter().text(
                            Pos2::new(kr.right() - 4.0, kr.center().y),
                            Align2::RIGHT_CENTER,
                            format!("C{}", oct),
                            FontId::monospace(9.0),
                            Color32::from_rgb(30, 32, 40),
                        );
                    }

                    // Row background on MIDI grid
                    let gr = Rect::from_min_size(
                        Pos2::new(grid_rect.left(), y0),
                        Vec2::new(grid_rect.width(), row_h),
                    );
                    ui.painter().rect_filled(
                        gr,
                        0,
                        if is_black {
                            Color32::from_rgb(17, 19, 25)
                        } else {
                            Color32::from_rgb(22, 25, 32)
                        },
                    );
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

                // Vertical 1/16 & Beat Grid lines
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

                // Access notes of active take
                let Some(clip_mut) = project
                    .find_track_mut(track_id)
                    .and_then(|t| t.find_clip_mut(clip_id))
                else {
                    return;
                };
                let active_take = clip_mut.active_take.min(clip_mut.takes.len().saturating_sub(1));
                let Some(TakeContent::Midi { notes }) =
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

                    // Velocity stalk at bottom
                    let vh = (note.velocity as f32 / 127.0) * (vel_rect.height() - 4.0);
                    ui.painter().line_segment(
                        [
                            Pos2::new(nx0 + 2.0, vel_rect.bottom()),
                            Pos2::new(nx0 + 2.0, vel_rect.bottom() - vh),
                        ],
                        Stroke::new(3.0_f32, t_col),
                    );

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
                                    notes[n_idx].pitch =
                                        (notes[n_idx].pitch as i32 + dp).clamp(0, 127) as u8;
                                }
                            }
                            actions.push(DockAction::ProjectMutated("Edit MIDI Note"));
                        }
                    }
                }

                if let Some(del_id) = delete_note_id {
                    notes.retain(|n| n.id != del_id);
                    actions.push(DockAction::ProjectMutated("Delete MIDI Note"));
                }

                if self.drag_note.is_some() && !ui.input(|i| i.pointer.any_down()) {
                    self.drag_note = None;
                }

                // Click empty grid cell -> insert new quantized MIDI note!
                let grid_bg_resp =
                    ui.interact(grid_rect, ui.id().with("midi_grid_bg"), Sense::click());
                if grid_bg_resp.clicked() && !clicked_existing {
                    if let Some(pos) = grid_bg_resp.interact_pointer_pos() {
                        let raw_b = ((pos.x - grid_rect.left()) / self.zoom_x) as f64;
                        let start_b = (raw_b * 4.0).floor() / 4.0;
                        let r = (((pos.y - grid_rect.top()) / row_h).floor() as usize).min(rows - 1);
                        let pitch = self.lowest_pitch + (rows - 1 - r) as u8;
                        notes.push(MidiNote::new(pitch, 104, start_b, self.default_note_len));
                        actions.push(DockAction::ProjectMutated("Insert MIDI Note"));
                    }
                }

                // Playhead indicator inside Piano Roll
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

        let Some(clip) = project
            .find_track_mut(track_id)
            .and_then(|t| t.find_clip_mut(clip_id))
        else {
            return;
        };

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("〰 Audio Clip & Take Comping:").strong());
            ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(160.0));
            ui.separator();
            let mut ch = false;
            ch |= ui
                .add(
                    egui::DragValue::new(&mut clip.gain_db)
                        .range(-36.0..=18.0)
                        .speed(0.15)
                        .prefix("Clip Gain ")
                        .suffix(" dB"),
                )
                .changed();
            ch |= ui
                .add(
                    egui::DragValue::new(&mut clip.pitch_semitones)
                        .range(-24.0..=24.0)
                        .speed(0.1)
                        .prefix("Varispeed ")
                        .suffix(" st"),
                )
                .changed();
            ch |= ui
                .add(
                    egui::DragValue::new(&mut clip.fade_in_beats)
                        .range(0.0..=8.0)
                        .speed(0.05)
                        .prefix("Fade In ")
                        .suffix(" b"),
                )
                .changed();
            ch |= ui
                .add(
                    egui::DragValue::new(&mut clip.fade_out_beats)
                        .range(0.0..=8.0)
                        .speed(0.05)
                        .prefix("Fade Out ")
                        .suffix(" b"),
                )
                .changed();

            egui::ComboBox::from_id_salt("fade_shape_combo")
                .selected_text(format!("{:?}", clip.fade_shape))
                .width(96.0)
                .show_ui(ui, |ui| {
                    for s in [
                        FadeShape::EqualPower,
                        FadeShape::Linear,
                        FadeShape::FastStart,
                        FadeShape::SlowStart,
                    ] {
                        ch |= ui
                            .selectable_value(&mut clip.fade_shape, s, format!("{:?}", s))
                            .changed();
                    }
                });

            // REAPER Per-Clip FX Quick Add!
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
                actions.push(DockAction::ProjectMutated("Adjust Audio Clip Envelope"));
            }
        });

        // High-res Waveform Preview + Active Comping Overlay
        let wave_h = (ui.available_height() - 42.0).max(90.0);
        let (wrect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), wave_h), Sense::hover());
        ui.painter().rect_filled(wrect, 4, BG_DEEP);
        ui.painter()
            .rect_stroke(wrect, 4, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);

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

        // Take switcher & Per-Clip FX Strip
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Active Take:").size(11.0).color(TEXT_DIM));
            for i in 0..clip.takes.len() {
                let lbl = format!("{}: {}", i + 1, clip.takes[i].name);
                if pill_toggle(ui, &lbl, clip.active_take == i && clip.comp_segments.is_empty(), t_col)
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
            if !clip.clip_fx.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new("Per-Clip FX:").size(11.0).color(ACCENT_FERRIC));
                let mut remove_idx = None;
                for (fi, fx) in clip.clip_fx.iter_mut().enumerate() {
                    if pill_toggle(ui, fx.kind.short_badge(), fx.enabled, ACCENT_FERRIC).clicked() {
                        fx.enabled = !fx.enabled;
                        actions.push(DockAction::ProjectMutated("Toggle Clip FX"));
                    }
                    if ui.small_button("×").clicked() {
                        remove_idx = Some(fi);
                    }
                }
                if let Some(ri) = remove_idx {
                    clip.clip_fx.remove(ri);
                    actions.push(DockAction::ProjectMutated("Remove Clip FX"));
                }
            }
        });
    }
}

/// Renders Track (or Master) FX Rack + Interactive EQ/Spectrum Graph + Bitwig Modulator Matrix!
pub fn render_fx_and_modulators_panel(
    ui: &mut Ui,
    project: &mut Project,
    selected_track: Option<TrackId>,
    inspect_master: &mut bool,
    telem: &EngineTelemetry,
    actions: &mut Vec<DockAction>,
) {
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
                    if !kind.is_instrument() && ui.button(kind.display_name()).clicked() {
                        project.master.fx_chain.push(FxSlot::new(kind));
                        actions.push(DockAction::ProjectMutated("Add Master FX"));
                        ui.close_menu();
                    }
                }
            });
        } else if let Some(tid) = selected_track {
            ui.menu_button("+ Add Instrument / FX", |ui| {
                for &kind in FxKind::ALL {
                    if ui.button(kind.display_name()).clicked() {
                        if let Some(trk) = project.find_track_mut(tid) {
                            trk.fx_chain.push(FxSlot::new(kind));
                            actions.push(DockAction::ProjectMutated("Add Track FX"));
                        }
                        ui.close_menu();
                    }
                }
            });
            ui.menu_button("+ Add Modulator (LFO / Env / Macro)", |ui| {
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
                        trk.modulators.push(ModulatorSlot::new_macro("Macro 1", 0.5));
                        actions.push(DockAction::ProjectMutated("Add Macro Modulator"));
                    }
                    ui.close_menu();
                }
            });
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

                    // Bitwig-style Modulators Rack Cards first!
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

                    // Track FX Device Cards
                    render_fx_slot_cards(ui, &mut trk.fx_chain, t_col, telem, actions);

                    if trk.fx_chain.is_empty() && trk.modulators.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "Empty Universal Chain — click '+ Add Instrument / FX' above to add PolySynth, DrumKit, Tuner, AmpSim, EQ, Comp, Delay, or Reverb!",
                            )
                            .color(TEXT_DIM),
                        );
                    }
                } else {
                    ui.label(egui::RichText::new("Select a Track to view its FX & Modulators.").color(TEXT_DIM));
                }
            });
        });
}

fn render_fx_slot_cards(
    ui: &mut Ui,
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
            .stroke(Stroke::new(1.0_f32, if fx.enabled { accent.gamma_multiply(0.65) } else { BORDER_SUBTLE }))
            .corner_radius(6)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_min_height(155.0);
                ui.vertical(|ui| {
                    // Header: Enable, Title, Wet/Dry Mix, Reorder, Remove
                    ui.horizontal(|ui| {
                        if pill_toggle(ui, "ON", fx.enabled, accent).clicked() {
                            fx.enabled = !fx.enabled;
                            actions.push(DockAction::ProjectMutated("Bypass FX"));
                        }
                        ui.label(
                            egui::RichText::new(fx.kind.display_name())
                                .size(11.5)
                                .strong()
                                .color(TEXT_MAIN),
                        );
                        if !fx.kind.is_instrument() {
                            let r_wet = ui.add(
                                egui::DragValue::new(&mut fx.wet_mix)
                                    .range(0.0..=1.0)
                                    .speed(0.01)
                                    .prefix("Wet "),
                            );
                            if r_wet.changed() {
                                actions.push(DockAction::ProjectMutated("Adjust FX Wet/Dry"));
                            }
                        }
                        if idx > 0 && ui.small_button("◀").on_hover_text("Move FX Earlier").clicked() {
                            move_swap = Some((idx, idx - 1));
                        }
                        if idx + 1 < len && ui.small_button("▶").on_hover_text("Move FX Later").clicked() {
                            move_swap = Some((idx, idx + 1));
                        }
                        if ui.small_button("×").on_hover_text("Remove FX").clicked() {
                            remove_idx = Some(idx);
                        }
                    });

                    ui.separator();

                    // Special visual displays for EQ Curve + Spectrum or Chromatic Strobe Tuner!
                    if fx.kind == FxKind::ParametricEq {
                        draw_eq_curve_mini(ui, &fx.params, telem);
                    } else if fx.kind == FxKind::ChromaticTuner {
                        draw_strobe_tuner_display(ui, &telem.tuner);
                    }

                    // Parameter Rotary Knobs (up to 6 per row)
                    let specs = fx.kind.param_specs();
                    if fx.params.len() < specs.len() {
                        fx.params.resize_with(specs.len(), || 0.0);
                    }
                    for chunk_start in (0..specs.len()).step_by(6) {
                        ui.horizontal(|ui| {
                            let end = (chunk_start + 6).min(specs.len());
                            for p_i in chunk_start..end {
                                let sp = specs[p_i];
                                if studio_knob(
                                    ui,
                                    sp.name,
                                    &mut fx.params[p_i],
                                    sp.min,
                                    sp.max,
                                    sp.default,
                                    sp.unit,
                                    32.0,
                                    accent,
                                ) {
                                    actions.push(DockAction::ProjectMutated("Tweak FX Param"));
                                }
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
                        steps,
                        step_beats,
                        ..
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

                // Target Assignments
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
                            if ui.button(format!("{} → {}", kind.short_badge(), sp.name)).clicked()
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
                            ModTarget::FxParam {
                                fx_id,
                                param_index,
                            } => fx_list
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

fn draw_eq_curve_mini(ui: &mut Ui, params: &[f32], telem: &EngineTelemetry) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(280.0, 56.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4, BG_DEEP);

    // Subtle real-time FFT spectrum backdrop
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

    // 0 dB center line
    p.line_segment(
        [Pos2::new(rect.left(), rect.center().y), Pos2::new(rect.right(), rect.center().y)],
        Stroke::new(0.8_f32, BORDER_SUBTLE),
    );

    // Analytic Biquad Composite Response Curve (20 Hz .. 20 kHz)
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
    p.rect_stroke(rect, 4, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);

    let center_x = rect.center().x;
    p.line_segment(
        [Pos2::new(center_x, rect.top() + 4.0), Pos2::new(center_x, rect.bottom() - 4.0)],
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
                            "M: {:.1} LUFS  |  S: {:.1} LUFS  |  Corr: {:+.2}",
                            telem.master.momentary_lufs,
                            telem.master.short_term_lufs,
                            telem.master.phase_correlation
                        ))
                        .size(10.5)
                        .monospace()
                        .color(ACCENT_TEAL),
                    );
                });
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(310.0, 125.0), Sense::hover());
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
