use crate::theme::{
    pill_toggle, track_color32, StudioUiExt, ACCENT_AMBER, ACCENT_FERRIC, ACCENT_RED, ACCENT_TEAL,
    ACCENT_PURPLE, ACCENT_CYAN, ACCENT_BLUE, BG_DEEP, BG_ELEVATED, BG_PANEL, BG_TRACK_EVEN, BG_TRACK_ODD,
    BORDER_SUBTLE, BUTTON_RADIUS, BUTTON_SURFACE, TEXT_DIM, TEXT_MAIN,
};
use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use ferric_engine::EngineTelemetry;
use ferric_model::{
    ChordQuality, ClipId, InputSource, MidiNote, ProgressionPreset, Project, SessionSlot, SnapGrid,
    Take, TakeContent, TimelineClip, TrackId, VoicingStyle, CIRCLE_OF_FIFTHS_MAJOR,
    CIRCLE_OF_FIFTHS_MINOR, NOTE_PITCH_CLASSES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditTool {
    SmartPointer,
    SplitRazor,
    DrawPencil,
}

impl EditTool {
    pub fn label(self) -> &'static str {
        match self {
            EditTool::SmartPointer => "Pointer",
            EditTool::SplitRazor => "Razor",
            EditTool::DrawPencil => "Draw",
        }
    }
}

pub enum ArrangementAction {
    Seek(f64),
    SelectTrack(TrackId),
    SelectClip {
        track_id: TrackId,
        clip_id: ClipId,
    },
    /// Chord Storyboard generate — main snapshots first, then applies.
    GenerateArr {
        spawn_pad: bool,
        spawn_bass: bool,
        spawn_arp: bool,
        section_idx: Option<usize>,
    },
    UndoGenerateArr,
    ProjectMutated(&'static str),
    SplitClip {
        track_id: TrackId,
        clip_id: ClipId,
        beat: f64,
    },
    DuplicateClip {
        track_id: TrackId,
        clip_id: ClipId,
    },
    DeleteClip {
        track_id: TrackId,
        clip_id: ClipId,
    },
    MoveClipToTrack {
        from_track: TrackId,
        to_track: TrackId,
        clip_id: ClipId,
    },
    OpenPianoRoll {
        track_id: TrackId,
        clip_id: ClipId,
    },
    /// Select track and jump lower dock to FX / modulators.
    OpenFxPanel(TrackId),
    ImportAudioToTrack(TrackId),
    ToggleFreezeTrack(TrackId),
    SplitAudioToStems {
        track_id: TrackId,
        clip_id: ClipId,
    },
    DeleteTrack(TrackId),
    DuplicateTrack(TrackId),
    MoveTrack {
        track_id: TrackId,
        delta: isize,
    },
    AddTrack(NewTrackKind),
}

/// First-class track presets — pick what you want to record / play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewTrackKind {
    Vocal,
    Midi,
    Instrument,
    AudioInterface,
    Universal,
}

impl NewTrackKind {
    pub const ALL: &'static [NewTrackKind] = &[
        NewTrackKind::Vocal,
        NewTrackKind::Midi,
        NewTrackKind::Instrument,
        NewTrackKind::AudioInterface,
        NewTrackKind::Universal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NewTrackKind::Vocal => "Vocal (Mic)",
            NewTrackKind::Midi => "MIDI",
            NewTrackKind::Instrument => "Instrument (Synth)",
            NewTrackKind::AudioInterface => "Audio Interface",
            NewTrackKind::Universal => "Universal (Audio + MIDI)",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            NewTrackKind::Vocal => "Mono mic input · gate · vocal pitch · compressor · armed",
            NewTrackKind::Midi => "MIDI-only input · no mic · ready to record notes",
            NewTrackKind::Instrument => "MIDI + PolySynth · no mic bleed · armed for notes",
            NewTrackKind::AudioInterface => "Mono line/mic input · EQ · compressor · armed",
            NewTrackKind::Universal => "Stereo in + MIDI — catch anything on one track",
        }
    }
}

pub struct ArrangementState {
    pub pixels_per_beat: f32,
    pub scroll_beat: f64,
    pub tool: EditTool,
    pub ripple_edit: bool,
    pub show_session_launcher: bool,
    /// Chord Storyboard: selected scene section for scoped Generate Arr.
    pub storyboard_section: Option<usize>,
    /// Last Generate Arr patch card (Undo = one history restore).
    pub generate_patch_card: Option<String>,
    pub renaming_track: Option<TrackId>,
    pub multi_selected_clips: Vec<(TrackId, ClipId)>,
    marquee_start_pos: Option<Pos2>,
    marquee_current_pos: Option<Pos2>,
    clip_hit_rects: Vec<(TrackId, ClipId, Rect)>,
    drag_clip: Option<(TrackId, ClipId, DragKind, f64)>,
    track_row_bounds: Vec<(TrackId, f32, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Move,
    ResizeLeft,
    ResizeRight,
}

impl Default for ArrangementState {
    fn default() -> Self {
        Self {
            pixels_per_beat: 42.0,
            scroll_beat: 0.0,
            tool: EditTool::SmartPointer,
            ripple_edit: false,
            show_session_launcher: true,
            storyboard_section: None,
            generate_patch_card: None,
            renaming_track: None,
            multi_selected_clips: Vec::new(),
            marquee_start_pos: None,
            marquee_current_pos: None,
            clip_hit_rects: Vec::new(),
            drag_clip: None,
            track_row_bounds: Vec::new(),
        }
    }
}

impl ArrangementState {
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        selected_track: Option<TrackId>,
        selected_clip: Option<(TrackId, ClipId)>,
        telem: &EngineTelemetry,
        actions: &mut Vec<ArrangementAction>,
    ) {
        // Row 1 — lanes, tools, snap, zoom, add track (kept short so nothing clips)
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if pill_toggle(ui, "Session", self.show_session_launcher, ACCENT_TEAL)
                .on_hover_text("Session clip launcher columns")
                .clicked()
            {
                self.show_session_launcher = !self.show_session_launcher;
            }
            if pill_toggle(ui, "Tempo Map", project.show_conductor_lane, ACCENT_AMBER)
                .on_hover_text("Conductor lane — BPM ramps & meter changes")
                .clicked()
            {
                project.show_conductor_lane = !project.show_conductor_lane;
                if project.show_conductor_lane && project.tempo_map.is_empty() {
                    project.upsert_tempo_point(0.0, project.bpm, true);
                }
            }
            if pill_toggle(
                ui,
                "Chords",
                project.show_chord_lane,
                ACCENT_PURPLE,
            )
            .on_hover_text("Chord lane + Circle of Fifths")
            .clicked()
            {
                project.show_chord_lane = !project.show_chord_lane;
            }
            ui.separator();
            for t in [EditTool::SmartPointer, EditTool::SplitRazor, EditTool::DrawPencil] {
                if pill_toggle(ui, t.label(), self.tool == t, ACCENT_FERRIC)
                    .on_hover_text(match t {
                        EditTool::SmartPointer => "Select / move / marquee",
                        EditTool::SplitRazor => "Click clips to split at cursor",
                        EditTool::DrawPencil => "Click empty lane to draw a MIDI clip",
                    })
                    .clicked()
                {
                    self.tool = t;
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("Snap").size(10.5).color(TEXT_DIM));
            egui::ComboBox::from_id_salt("snap_grid_combo")
                .selected_text(project.snap_grid.label())
                .width(90.0)
                .show_ui(ui, |ui| {
                    for &g in SnapGrid::ALL {
                        ui.selectable_value(&mut project.snap_grid, g, g.label());
                    }
                });
            if pill_toggle(ui, "Ripple", self.ripple_edit, ACCENT_AMBER)
                .on_hover_text("Deleting clips shifts later clips left on the track")
                .clicked()
            {
                self.ripple_edit = !self.ripple_edit;
            }
            ui.separator();
            ui.spacing_mut().slider_width = 72.0;
            ui.label(egui::RichText::new("Zoom").size(10.5).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.pixels_per_beat, 14.0..=140.0).show_value(false))
                .on_hover_text("Ctrl+Wheel also zooms");
            let max_scroll = (project.project_end_beat() + 32.0).max(64.0);
            ui.label(egui::RichText::new("Scroll").size(10.5).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.scroll_beat, 0.0..=max_scroll).show_value(false))
                .on_hover_text("Shift+Wheel / middle-drag / Alt-drag to pan");

            ui.menu_button(
                egui::RichText::new("+ Track").strong().color(ACCENT_TEAL),
                |ui| {
                    ui.set_min_width(220.0);
                    ui.label(
                        egui::RichText::new("What are you recording?")
                            .size(11.0)
                            .color(TEXT_DIM),
                    );
                    ui.separator();
                    for &kind in NewTrackKind::ALL {
                        if ui
                            .add_sized(
                                [210.0, 22.0],
                                egui::Button::new(kind.label())
                                    .fill(BUTTON_SURFACE)
                                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                    .corner_radius(BUTTON_RADIUS)
                                    .sense(Sense::click()),
                            )
                            .on_hover_text(kind.hint())
                            .clicked()
                        {
                            actions.push(ArrangementAction::AddTrack(kind));
                            ui.close_menu();
                        }
                    }
                },
            );
            if self.multi_selected_clips.len() > 1 {
                ui.label(
                    egui::RichText::new(format!("{} clips", self.multi_selected_clips.len()))
                        .size(10.5)
                        .strong()
                        .color(ACCENT_TEAL),
                );
            }
            if project.tracks.iter().any(|t| t.active_session_slot.is_some())
                && ui
                    .chrome_small_button("Stop Clips")
                    .on_hover_text("Stop all playing session clips")
                    .clicked()
            {
                project.stop_all_session_clips();
                actions.push(ArrangementAction::ProjectMutated("Stop All Session Clips"));
            }
        });

        // Row 2 — Chord Storyboard (separate so Gen buttons stay visible)
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(
                egui::RichText::new("Storyboard")
                    .size(10.5)
                    .strong()
                    .color(ACCENT_PURPLE),
            );
            for (i, scene) in project.scenes.iter().enumerate() {
                let sel = self.storyboard_section == Some(i);
                if pill_toggle(ui, &scene.name, sel, ACCENT_PURPLE)
                    .on_hover_text("Scope Generate to this section")
                    .clicked()
                {
                    self.storyboard_section = if sel { None } else { Some(i) };
                }
            }
            if ui
                .chrome_small_button("Pad")
                .on_hover_text("Generate pad for selected section (or whole chord lane)")
                .clicked()
            {
                actions.push(ArrangementAction::GenerateArr {
                    spawn_pad: true,
                    spawn_bass: false,
                    spawn_arp: false,
                    section_idx: self.storyboard_section,
                });
            }
            if ui
                .chrome_small_button("Bass")
                .on_hover_text("Generate bass for selected section")
                .clicked()
            {
                actions.push(ArrangementAction::GenerateArr {
                    spawn_pad: false,
                    spawn_bass: true,
                    spawn_arp: false,
                    section_idx: self.storyboard_section,
                });
            }
            if ui
                .chrome_small_button("Arp")
                .on_hover_text("Generate arp for selected section")
                .clicked()
            {
                actions.push(ArrangementAction::GenerateArr {
                    spawn_pad: false,
                    spawn_bass: false,
                    spawn_arp: true,
                    section_idx: self.storyboard_section,
                });
            }
            if self.generate_patch_card.is_some()
                && ui
                    .chrome_small_button("Undo Gen")
                    .on_hover_text("Restore snapshot from last Generate")
                    .clicked()
            {
                actions.push(ArrangementAction::UndoGenerateArr);
            }
            if let Some(ref card) = self.generate_patch_card {
                ui.label(
                    egui::RichText::new(card.as_str())
                        .size(10.0)
                        .color(TEXT_DIM)
                        .italics(),
                );
            }
        });

        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("arrangement_vertical_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.render_lanes(
                    ui,
                    project,
                    selected_track,
                    selected_clip,
                    telem,
                    actions,
                );
            });
    }

    fn render_lanes(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        selected_track: Option<TrackId>,
        selected_clip: Option<(TrackId, ClipId)>,
        telem: &EngineTelemetry,
        actions: &mut Vec<ArrangementAction>,
    ) {
        let header_w = 238.0_f32;
        let session_col_w = 92.0_f32;
        let num_scenes = project.scenes.len();
        let session_w = if self.show_session_launcher {
            (num_scenes as f32) * session_col_w + 86.0
        } else {
            0.0
        };
        let ruler_h = 28.0_f32;

        // Ctrl+wheel zoom · Shift/horizontal wheel pan time · Mid or Alt+drag pans time + tracks.
        if ui.rect_contains_pointer(ui.max_rect()) {
            let (ctrl, shift, alt, scroll, mid_down, primary_down, ptr_delta) = ui.input(|i| {
                (
                    i.modifiers.ctrl,
                    i.modifiers.shift,
                    i.modifiers.alt,
                    i.smooth_scroll_delta,
                    i.pointer.middle_down(),
                    i.pointer.primary_down(),
                    i.pointer.delta(),
                )
            });
            if ctrl && scroll.y.abs() > 0.1 {
                self.pixels_per_beat =
                    (self.pixels_per_beat * (1.0 + scroll.y * 0.003)).clamp(14.0, 160.0);
            } else if shift && (scroll.y.abs() > 0.1 || scroll.x.abs() > 0.1) {
                let delta_px = if scroll.x.abs() > 0.1 {
                    scroll.x
                } else {
                    scroll.y
                };
                self.scroll_beat =
                    (self.scroll_beat - (delta_px / self.pixels_per_beat) as f64).max(0.0);
            } else if scroll.x.abs() > 0.1 {
                self.scroll_beat =
                    (self.scroll_beat - (scroll.x / self.pixels_per_beat) as f64).max(0.0);
            }
            // Plain vertical wheel is handled by the parent ScrollArea (scroll across tracks).
            let pan_drag = mid_down || (alt && primary_down);
            if pan_drag && (ptr_delta.x.abs() > 0.1 || ptr_delta.y.abs() > 0.1) {
                self.scroll_beat =
                    (self.scroll_beat - (ptr_delta.x / self.pixels_per_beat) as f64).max(0.0);
                ui.scroll_with_delta(Vec2::new(0.0, ptr_delta.y));
            }
        }

        // Determine visible tracks (hide children of collapsed folders!)
        let mut visible_indices = Vec::with_capacity(project.tracks.len());
        for idx in 0..project.tracks.len() {
            let collapsed_ancestor = project.tracks[idx]
                .parent_folder_id
                .and_then(|pid| project.find_track(pid))
                .map(|p| p.folder_collapsed)
                .unwrap_or(false);
            if !collapsed_ancestor {
                visible_indices.push(idx);
            }
        }

        // Top Ruler Row (Track Header Label + Session Scene Headers + Bar/Beat Timeline Ruler)
        let avail_w = ui.available_width().max(640.0);
        let timeline_x0_rel = header_w + session_w;
        let timeline_w = (avail_w - timeline_x0_rel).max(220.0);

        let (ruler_row_rect, _) =
            ui.allocate_exact_size(Vec2::new(avail_w, ruler_h), Sense::hover());
        let p = ui.painter();
        p.rect_filled(ruler_row_rect, 0, BG_ELEVATED);
        p.line_segment(
            [ruler_row_rect.left_bottom(), ruler_row_rect.right_bottom()],
            Stroke::new(1.0_f32, BORDER_SUBTLE),
        );

        p.text(
            Pos2::new(ruler_row_rect.left() + 10.0, ruler_row_rect.center().y),
            Align2::LEFT_CENTER,
            format!("TRACKS ({})", project.tracks.len()),
            FontId::proportional(11.0),
            TEXT_DIM,
        );

        // Session Scene Launcher Headers
        if self.show_session_launcher {
            let sx0 = ruler_row_rect.left() + header_w;
            for s_idx in 0..num_scenes {
                let cell = Rect::from_min_size(
                    Pos2::new(sx0 + s_idx as f32 * session_col_w, ruler_row_rect.top()),
                    Vec2::new(session_col_w - 2.0, ruler_h),
                );
                let scene_resp = ui.interact(
                    cell,
                    ui.id().with(("scene_hdr", s_idx)),
                    Sense::click(),
                );
                let bg = if scene_resp.hovered() {
                    Color32::from_rgb(44, 52, 64)
                } else {
                    Color32::from_rgb(28, 32, 40)
                };
                ui.painter().rect_filled(cell.shrink(1.0), 3, bg);
                ui.painter().text(
                    cell.center(),
                    Align2::CENTER_CENTER,
                    format!("▶ {}", project.scenes[s_idx].name),
                    FontId::proportional(10.5),
                    ACCENT_TEAL,
                );
                if scene_resp.clicked() {
                    project.launch_scene(s_idx);
                    actions.push(ArrangementAction::ProjectMutated("Launch Scene"));
                }
                scene_resp.context_menu(|ui| {
                    if ui
                        .chrome_button("➔ Print Scene to Arrangement at Playhead")
                        .clicked()
                    {
                        let at = project.snap_grid.snap(telem.playhead_beat, project.time_signature);
                        project.print_scene_to_timeline(s_idx, at);
                        actions.push(ArrangementAction::ProjectMutated("Print Scene to Timeline"));
                        ui.close_menu();
                    }
                });
            }
            let master_lbl = Rect::from_min_size(
                Pos2::new(sx0 + num_scenes as f32 * session_col_w, ruler_row_rect.top()),
                Vec2::new(84.0, ruler_h),
            );
            ui.painter().text(
                master_lbl.center(),
                Align2::CENTER_CENTER,
                "ARRANGEMENT ➔",
                FontId::proportional(9.5),
                TEXT_DIM,
            );
        }

        // Timeline Beat Ruler + Loop Region + Click-to-Seek
        let ruler_tl_rect = Rect::from_min_size(
            Pos2::new(ruler_row_rect.left() + timeline_x0_rel, ruler_row_rect.top()),
            Vec2::new(timeline_w, ruler_h),
        );
        let ruler_resp = ui.interact(
            ruler_tl_rect,
            ui.id().with("timeline_ruler_seek"),
            Sense::click_and_drag(),
        );
        let ruler_resp = ruler_resp
            .on_hover_text("Click or drag to move the playhead · Shift = free (no snap)");
        if ruler_resp.clicked() || ruler_resp.dragged() {
            if let Some(pos) = ruler_resp.interact_pointer_pos() {
                let raw_beat =
                    self.scroll_beat + ((pos.x - ruler_tl_rect.left()) / self.pixels_per_beat) as f64;
                let beat = if ui.input(|i| i.modifiers.shift) {
                    raw_beat.max(0.0)
                } else {
                    project.snap_grid.snap(raw_beat, project.time_signature)
                };
                actions.push(ArrangementAction::Seek(beat));
            }
        }

        let bpb = project.time_signature.beats_per_bar();
        let max_vis_beat = self.scroll_beat + (timeline_w / self.pixels_per_beat) as f64;

        // Loop Region highlight + Interactive Left/Right Loop Brace Drag Handles on Ruler!
        let mut loop_handle_dragged = false;
        if project.loop_end_beat > project.loop_start_beat {
            let lx0 = ruler_tl_rect.left()
                + ((project.loop_start_beat - self.scroll_beat) as f32 * self.pixels_per_beat);
            let lx1 = ruler_tl_rect.left()
                + ((project.loop_end_beat - self.scroll_beat) as f32 * self.pixels_per_beat);
            let lrect = Rect::from_min_max(
                Pos2::new(lx0.max(ruler_tl_rect.left()), ruler_tl_rect.top()),
                Pos2::new(lx1.min(ruler_tl_rect.right()), ruler_tl_rect.top() + 8.0),
            );
            if lrect.width() > 1.0 {
                let col = if project.loop_enabled {
                    Color32::from_rgba_unmultiplied(232, 114, 62, 195)
                } else {
                    Color32::from_rgba_unmultiplied(110, 115, 130, 110)
                };
                ui.painter().rect_filled(lrect, 2, col);
            }

            // Draggable Loop Start Handle `[`
            if lx0 >= ruler_tl_rect.left() - 6.0 && lx0 <= ruler_tl_rect.right() {
                let h_in = Rect::from_center_size(
                    Pos2::new(lx0, ruler_tl_rect.top() + 5.0),
                    Vec2::new(10.0, 11.0),
                );
                ui.painter().rect_filled(h_in, 2, ACCENT_FERRIC);
                let r_in = ui
                    .interact(h_in, ui.id().with("loop_in_handle"), Sense::drag())
                    .on_hover_text("Drag Loop Start Brace");
                if r_in.dragged() {
                    loop_handle_dragged = true;
                    let db = (r_in.drag_delta().x / self.pixels_per_beat) as f64;
                    project.loop_start_beat = (project.loop_start_beat + db)
                        .clamp(0.0, (project.loop_end_beat - 1.0).max(0.0));
                    actions.push(ArrangementAction::ProjectMutated("Drag Loop Start"));
                }
            }

            // Draggable Loop End Handle `]`
            if lx1 >= ruler_tl_rect.left() && lx1 <= ruler_tl_rect.right() + 6.0 {
                let h_out = Rect::from_center_size(
                    Pos2::new(lx1, ruler_tl_rect.top() + 5.0),
                    Vec2::new(10.0, 11.0),
                );
                ui.painter().rect_filled(h_out, 2, ACCENT_FERRIC);
                let r_out = ui
                    .interact(h_out, ui.id().with("loop_out_handle"), Sense::drag())
                    .on_hover_text("Drag Loop End Brace");
                if r_out.dragged() {
                    loop_handle_dragged = true;
                    let db = (r_out.drag_delta().x / self.pixels_per_beat) as f64;
                    project.loop_end_beat =
                        (project.loop_end_beat + db).max(project.loop_start_beat + 1.0);
                    actions.push(ArrangementAction::ProjectMutated("Drag Loop End"));
                }
            }
        }
        let _ = loop_handle_dragged;

        // Right-Click Ruler Context Menu: Add Song Marker, Set Loop Start/End!
        ruler_resp.context_menu(|ui| {
            ui.set_min_width(220.0);
            let ph = project
                .snap_grid
                .snap(telem.playhead_beat, project.time_signature);
            if ui
                .chrome_button(format!("Move Playhead Here ({:.1})", ph))
                .clicked()
            {
                actions.push(ArrangementAction::Seek(ph));
                ui.close_menu();
            }
            if ui.chrome_button("Add Marker Here").clicked() {
                let n = project.markers.len() + 1;
                project.add_marker(format!("Marker {}", n), ph);
                actions.push(ArrangementAction::ProjectMutated("Add Song Marker"));
                ui.close_menu();
            }
            ui.separator();
            ui.label(egui::RichText::new("Loop").size(10.5).color(TEXT_DIM));
            if ui.chrome_button("Set Loop Start Here").clicked() {
                project.loop_start_beat = ph.min(project.loop_end_beat - 1.0).max(0.0);
                project.loop_enabled = true;
                actions.push(ArrangementAction::ProjectMutated("Set Loop Start"));
                ui.close_menu();
            }
            if ui.chrome_button("Set Loop End Here").clicked() {
                project.loop_end_beat = ph.max(project.loop_start_beat + 1.0);
                project.loop_enabled = true;
                actions.push(ArrangementAction::ProjectMutated("Set Loop End"));
                ui.close_menu();
            }
            let bpb = project.time_signature.beats_per_bar();
            if ui
                .chrome_button("Loop Next 4 Bars from Playhead")
                .on_hover_text("Sets a 4-bar loop starting at the playhead and enables LOOP")
                .clicked()
            {
                project.loop_start_beat = ph.max(0.0);
                project.loop_end_beat = ph + bpb * 4.0;
                project.loop_enabled = true;
                actions.push(ArrangementAction::ProjectMutated("Loop 4 Bars"));
                ui.close_menu();
            }
            if ui
                .checkbox(&mut project.loop_enabled, "Loop Enabled")
                .changed()
            {
                actions.push(ArrangementAction::ProjectMutated("Toggle Loop"));
            }
            ui.separator();
            ui.menu_button("Meter Change at Playhead", |ui| {
                for (num, den) in [(4, 4), (3, 4), (6, 8), (5, 4), (7, 8), (12, 8)] {
                    if ui.chrome_button(format!("{num}/{den}")).clicked() {
                        project.add_meter_change(ph.round().max(0.0) as u32, num, den);
                        project.show_conductor_lane = true;
                        actions.push(ArrangementAction::ProjectMutated("Add Meter Change"));
                        ui.close_menu();
                    }
                }
            });
            if ui
                .chrome_button(format!(
                    "Tempo Point ({:.1} BPM) Here",
                    project.bpm_at_beat(ph)
                ))
                .clicked()
            {
                let cur = project.bpm_at_beat(ph);
                project.upsert_tempo_point(ph, cur, true);
                project.show_conductor_lane = true;
                actions.push(ArrangementAction::ProjectMutated("Add Tempo Point"));
                ui.close_menu();
            }
        });

        // Auto Punch-In / Punch-Out Crimson Bracket on ruler!
        if project.punch_in_enabled && project.punch_end_beat > project.punch_start_beat {
            let px0 = ruler_tl_rect.left()
                + ((project.punch_start_beat - self.scroll_beat) as f32 * self.pixels_per_beat);
            let px1 = ruler_tl_rect.left()
                + ((project.punch_end_beat - self.scroll_beat) as f32 * self.pixels_per_beat);
            let prect = Rect::from_min_max(
                Pos2::new(px0.max(ruler_tl_rect.left()), ruler_tl_rect.top() + 7.0),
                Pos2::new(px1.min(ruler_tl_rect.right()), ruler_tl_rect.top() + 13.0),
            );
            if prect.width() > 1.0 {
                ui.painter().rect_filled(
                    prect,
                    2,
                    Color32::from_rgba_unmultiplied(232, 74, 86, 210),
                );
            }
        }

        // Bar numbers & ticks
        let mut bar_idx = (self.scroll_beat / bpb).floor() as i64;
        while (bar_idx as f64) * bpb <= max_vis_beat {
            let beat = (bar_idx as f64) * bpb;
            let x = ruler_tl_rect.left() + ((beat - self.scroll_beat) as f32 * self.pixels_per_beat);
            if x >= ruler_tl_rect.left() && x <= ruler_tl_rect.right() {
                ui.painter().line_segment(
                    [Pos2::new(x, ruler_tl_rect.top() + 8.0), Pos2::new(x, ruler_tl_rect.bottom())],
                    Stroke::new(1.0_f32, Color32::from_rgb(84, 90, 108)),
                );
                ui.painter().text(
                    Pos2::new(x + 4.0, ruler_tl_rect.bottom() - 3.0),
                    Align2::LEFT_BOTTOM,
                    format!("{}", bar_idx + 1),
                    FontId::monospace(10.5),
                    TEXT_MAIN,
                );
            }
            bar_idx += 1;
        }

        // Interactive Song Markers (Click to Jump, Right-Click to Rename/Delete!)
        let mut del_marker_id = None;
        for m_idx in 0..project.markers.len() {
            let m_id = project.markers[m_idx].id;
            let m_beat = project.markers[m_idx].beat;
            let mx =
                ruler_tl_rect.left() + ((m_beat - self.scroll_beat) as f32 * self.pixels_per_beat);
            if mx >= ruler_tl_rect.left() && mx <= ruler_tl_rect.right() - 24.0 {
                let mc = track_color32(project.markers[m_idx].color);
                let flag_rect = Rect::from_min_size(
                    Pos2::new(mx + 2.0, ruler_tl_rect.top() + 1.0),
                    Vec2::new(54.0, 13.0),
                );
                ui.painter()
                    .rect_filled(flag_rect, 2, mc.gamma_multiply(0.35));
                ui.painter().text(
                    Pos2::new(mx + 5.0, ruler_tl_rect.top() + 7.5),
                    Align2::LEFT_CENTER,
                    &project.markers[m_idx].name,
                    FontId::proportional(9.5),
                    mc,
                );
                let m_resp = ui
                    .interact(flag_rect, ui.id().with(("song_mk", m_id)), Sense::click())
                    .on_hover_text("Click to jump Playhead • Right-click to rename or delete");
                if m_resp.clicked() {
                    actions.push(ArrangementAction::Seek(m_beat));
                }
                m_resp.context_menu(|ui| {
                    ui.label("Marker Name:");
                    if ui
                        .text_edit_singleline(&mut project.markers[m_idx].name)
                        .changed()
                    {
                        actions.push(ArrangementAction::ProjectMutated("Rename Marker"));
                    }
                    if ui
                        .chrome_danger_button(egui::RichText::new("🗑 Delete Marker").color(ACCENT_RED))
                        .clicked()
                    {
                        del_marker_id = Some(m_id);
                        ui.close_menu();
                    }
                });
            }
        }
        if let Some(dmid) = del_marker_id {
            project.markers.retain(|m| m.id != dmid);
            actions.push(ArrangementAction::ProjectMutated("Delete Marker"));
        }

        // Render Optional Master Tempo & Time-Signature Conductor Lane right under the Ruler!
        let playhead_top = ruler_tl_rect.top();
        let mut playhead_bot = ruler_tl_rect.bottom();
        if project.show_conductor_lane {
            let cond_bot = self.render_conductor_lane(
                ui,
                avail_w,
                timeline_x0_rel,
                timeline_w,
                project,
                actions,
            );
            playhead_bot = playhead_bot.max(cond_bot);
        }
        if project.show_chord_lane {
            let chord_bot = self.render_chord_lane(
                ui,
                avail_w,
                timeline_x0_rel,
                timeline_w,
                project,
                telem.playhead_beat,
                actions,
            );
            playhead_bot = playhead_bot.max(chord_bot);
        }

        // Render Each Visible Track Row + Optional Multi-Take Comping Lanes!
        let prev_row_bounds = self.track_row_bounds.clone();
        self.track_row_bounds.clear();
        self.clip_hit_rects.clear();

        for (vis_pos, &t_idx) in visible_indices.iter().enumerate() {
            let track_id = project.tracks[t_idx].id;
            let is_folder = project.is_folder_track(track_id);
            let depth = project.track_depth(track_id);
            let base_h = project.tracks[t_idx].height_px.max(102.0);
            let max_extra_lanes = project.tracks[t_idx]
                .clips
                .iter()
                .filter(|c| c.show_take_lanes && c.takes.len() > 1)
                .map(|c| c.takes.len())
                .max()
                .unwrap_or(0);
            let take_lane_h = 24.0_f32;
            let comp_h = (max_extra_lanes as f32) * take_lane_h;
            let auto_lanes_count = if project.tracks[t_idx].show_automation {
                project.tracks[t_idx]
                    .automation_lanes
                    .iter()
                    .filter(|l| l.visible)
                    .count()
            } else {
                0
            };
            let auto_lane_h = 52.0_f32;
            let row_h = base_h + comp_h + (auto_lanes_count as f32) * auto_lane_h;

            let (row_rect, _) = ui.allocate_exact_size(Vec2::new(avail_w, row_h), Sense::hover());
            self.track_row_bounds
                .push((track_id, row_rect.top(), row_rect.bottom()));
            playhead_bot = playhead_bot.max(row_rect.bottom());

            let row_bg = if selected_track == Some(track_id) {
                Color32::from_rgb(31, 34, 44)
            } else if vis_pos % 2 == 0 {
                BG_TRACK_EVEN
            } else {
                BG_TRACK_ODD
            };
            ui.painter().rect_filled(row_rect, 0, row_bg);
            ui.painter().line_segment(
                [row_rect.left_bottom(), row_rect.right_bottom()],
                Stroke::new(1.0_f32, BORDER_SUBTLE),
            );

            // 1. Track Header (left)
            let hdr_rect = Rect::from_min_size(row_rect.min, Vec2::new(header_w, row_h));
            self.render_track_header(
                ui,
                hdr_rect,
                project,
                t_idx,
                is_folder,
                depth,
                selected_track == Some(track_id),
                telem,
                actions,
            );

            // 2. Session Clip Launcher Cells (middle-left, if visible)
            if self.show_session_launcher {
                let sess_rect = Rect::from_min_size(
                    Pos2::new(row_rect.left() + header_w, row_rect.top()),
                    Vec2::new(session_w, row_h),
                );
                self.render_session_cells(
                    ui,
                    sess_rect,
                    project,
                    t_idx,
                    session_col_w,
                    actions,
                );
            }

            // 3. Linear Arrangement Lane + Clips + Multi-Take Swipe Comping (right)
            let lane_rect = Rect::from_min_size(
                Pos2::new(row_rect.left() + timeline_x0_rel, row_rect.top()),
                Vec2::new(timeline_w, base_h),
            );
            let comp_area_rect = Rect::from_min_max(
                Pos2::new(lane_rect.left(), lane_rect.bottom()),
                Pos2::new(lane_rect.right(), lane_rect.bottom() + comp_h),
            );

            self.render_track_timeline_lane(
                ui,
                lane_rect,
                comp_area_rect,
                &prev_row_bounds,
                project,
                t_idx,
                selected_clip,
                telem,
                actions,
            );

            // 4. Foldable Curved Bezier Automation Lanes right beneath the track!
            if auto_lanes_count > 0 {
                let auto_top = comp_area_rect.bottom();
                self.render_track_automation_lanes(
                    ui,
                    Pos2::new(lane_rect.left(), auto_top),
                    timeline_w,
                    auto_lane_h,
                    project,
                    t_idx,
                    actions,
                );
            }
        }

        // Release active clip drag & finalize Rubber-Band Marquee Multi-Selection!
        if self.drag_clip.is_some() && !ui.input(|i| i.pointer.any_down()) {
            self.drag_clip = None;
        }
        if let (Some(m0), Some(m1)) = (self.marquee_start_pos, self.marquee_current_pos) {
            let mrect = Rect::from_two_pos(m0, m1);
            if mrect.width() > 4.0 || mrect.height() > 4.0 {
                ui.painter().rect_filled(
                    mrect,
                    2,
                    Color32::from_rgba_unmultiplied(66, 196, 176, 36),
                );
                ui.painter().rect_stroke(
                    mrect,
                    2,
                    Stroke::new(1.2_f32, ACCENT_TEAL),
                    StrokeKind::Inside,
                );
                self.multi_selected_clips.clear();
                for &(tid, cid, crect) in &self.clip_hit_rects {
                    if mrect.intersects(crect) {
                        self.multi_selected_clips.push((tid, cid));
                    }
                }
                if let Some(&(first_t, first_c)) = self.multi_selected_clips.first() {
                    actions.push(ArrangementAction::SelectClip {
                        track_id: first_t,
                        clip_id: first_c,
                    });
                }
            }
            if !ui.input(|i| i.pointer.any_down()) {
                self.marquee_start_pos = None;
                self.marquee_current_pos = None;
            }
        }

        // Draw crisp Playhead Line across entire ruler + all tracks!
        let ph_x = ruler_tl_rect.left()
            + ((telem.playhead_beat - self.scroll_beat) as f32 * self.pixels_per_beat);
        if ph_x >= ruler_tl_rect.left() - 8.0 && ph_x <= ruler_tl_rect.right() + 8.0 {
            let ph_col = if telem.is_recording {
                ACCENT_RED
            } else {
                ACCENT_FERRIC
            };
            // Draggable playhead head + tall grab strip (easy scrub).
            let handle = Rect::from_center_size(
                Pos2::new(ph_x, playhead_top + 7.0),
                Vec2::new(18.0, 20.0),
            );
            let strip = Rect::from_min_max(
                Pos2::new(ph_x - 5.0, playhead_top),
                Pos2::new(ph_x + 5.0, playhead_bot.min(playhead_top + 120.0)),
            );
            let ph_resp = ui.interact(
                handle.union(strip),
                ui.id().with("playhead_scrub"),
                Sense::click_and_drag(),
            );
            let ph_resp = ph_resp.on_hover_text("Drag to move playhead");
            if ph_resp.dragged() || ph_resp.clicked() {
                if let Some(pos) = ph_resp.interact_pointer_pos() {
                    let raw_beat = self.scroll_beat
                        + ((pos.x - ruler_tl_rect.left()) / self.pixels_per_beat) as f64;
                    let beat = if ui.input(|i| i.modifiers.shift) {
                        raw_beat.max(0.0)
                    } else {
                        project.snap_grid.snap(raw_beat, project.time_signature)
                    };
                    actions.push(ArrangementAction::Seek(beat));
                }
            }

            ui.painter().line_segment(
                [Pos2::new(ph_x, playhead_top), Pos2::new(ph_x, playhead_bot)],
                Stroke::new(1.8_f32, ph_col),
            );
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(ph_x - 6.5, playhead_top),
                    Pos2::new(ph_x + 6.5, playhead_top),
                    Pos2::new(ph_x, playhead_top + 10.0),
                ],
                ph_col,
                Stroke::NONE,
            ));
            // Grab notch so the head reads as a scrub handle.
            ui.painter().circle_filled(
                Pos2::new(ph_x, playhead_top + 3.0),
                2.2,
                Color32::WHITE.gamma_multiply(0.85),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_track_header(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        project: &mut Project,
        t_idx: usize,
        is_folder: bool,
        depth: usize,
        is_selected: bool,
        telem: &EngineTelemetry,
        actions: &mut Vec<ArrangementAction>,
    ) {
        let indent = (depth as f32) * 12.0;
        let track_id = project.tracks[t_idx].id;
        let t_col = track_color32(project.tracks[t_idx].color);

        let bg_resp = ui.interact(rect, ui.id().with(("trk_hdr_sel", track_id)), Sense::click());
        if bg_resp.clicked() {
            actions.push(ArrangementAction::SelectTrack(track_id));
        }
        if bg_resp.double_clicked() {
            self.renaming_track = Some(track_id);
        }

        // Right-Click Track Header Context Menu
        bg_resp.context_menu(|ui| {
            ui.set_min_width(200.0);
            ui.label(egui::RichText::new("Track name").size(10.5).color(TEXT_DIM));
            if ui
                .text_edit_singleline(&mut project.tracks[t_idx].name)
                .changed()
            {
                actions.push(ArrangementAction::ProjectMutated("Rename Track"));
            }
            ui.separator();
            ui.label(egui::RichText::new("Color").size(10.5).color(TEXT_DIM));
            ui.horizontal_wrapped(|ui| {
                for &rgb in ferric_model::STUDIO_TRACK_COLORS {
                    let c32 = track_color32(rgb);
                    if ui
                        .add(egui::Button::new("").fill(c32).min_size(Vec2::new(16.0, 16.0)))
                        .clicked()
                    {
                        project.tracks[t_idx].color = rgb;
                        actions.push(ArrangementAction::ProjectMutated("Change Track Color"));
                        ui.close_menu();
                    }
                }
            });
            ui.separator();
            ui.label(egui::RichText::new("Input preset").size(10.5).color(TEXT_DIM));
            if ui
                .chrome_button("MIDI only (no mic)")
                .on_hover_text("Instrument / MIDI recording — blocks mic bleed")
                .clicked()
            {
                project.tracks[t_idx].input_source = InputSource::MidiAll;
                actions.push(ArrangementAction::ProjectMutated("Input → MIDI"));
                ui.close_menu();
            }
            if ui
                .chrome_button("Mic / Line (mono 1)")
                .on_hover_text("Vocal / guitar / interface channel 1")
                .clicked()
            {
                project.tracks[t_idx].input_source = InputSource::AudioMono { channel: 0 };
                project.tracks[t_idx].input_monitoring = true;
                actions.push(ArrangementAction::ProjectMutated("Input → Mic"));
                ui.close_menu();
            }
            if ui
                .chrome_button("Stereo In 1/2 + MIDI")
                .on_hover_text("Universal capture — audio and MIDI together")
                .clicked()
            {
                project.tracks[t_idx].input_source =
                    InputSource::AudioAndMidi { stereo_pair: 0 };
                actions.push(ArrangementAction::ProjectMutated("Input → Universal"));
                ui.close_menu();
            }
            ui.separator();
            let armed = project.tracks[t_idx].record_arm;
            if ui
                .chrome_button(if armed {
                    "Disarm Record"
                } else {
                    "Arm Record"
                })
                .clicked()
            {
                project.tracks[t_idx].record_arm = !armed;
                actions.push(ArrangementAction::ProjectMutated("Toggle Record Arm"));
                ui.close_menu();
            }
            if ui.chrome_button("Import Audio onto Track…").clicked() {
                actions.push(ArrangementAction::ImportAudioToTrack(track_id));
                ui.close_menu();
            }
            ui.separator();
            if ui.chrome_button("Move Up").clicked() {
                actions.push(ArrangementAction::MoveTrack {
                    track_id,
                    delta: -1,
                });
                ui.close_menu();
            }
            if ui.chrome_button("Move Down").clicked() {
                actions.push(ArrangementAction::MoveTrack {
                    track_id,
                    delta: 1,
                });
                ui.close_menu();
            }
            if ui.chrome_button("Duplicate Track").clicked() {
                actions.push(ArrangementAction::DuplicateTrack(track_id));
                ui.close_menu();
            }
            if project.tracks.len() > 1 {
                ui.separator();
                if ui
                    .chrome_danger_button(egui::RichText::new("Delete Track").color(ACCENT_RED))
                    .clicked()
                {
                    actions.push(ArrangementAction::DeleteTrack(track_id));
                    ui.close_menu();
                }
            }
        });

        // Color strip on left
        let strip = Rect::from_min_size(
            Pos2::new(rect.left() + indent, rect.top()),
            Vec2::new(5.0, rect.height()),
        );
        ui.painter().rect_filled(strip, 0, t_col);
        if is_selected {
            ui.painter()
                .rect_stroke(rect.shrink(0.5), 0, Stroke::new(1.2_f32, t_col), StrokeKind::Inside);
        }

        let inner = Rect::from_min_max(
            Pos2::new(rect.left() + indent + 9.0, rect.top() + 5.0),
            Pos2::new(rect.right() - 6.0, rect.bottom() - 4.0),
        );
        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        child_ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if is_folder {
                    let icon = if project.tracks[t_idx].folder_collapsed {
                        "▶"
                    } else {
                        "▼"
                    };
                    if ui
                        .chrome_small_button(icon)
                        .on_hover_text("Collapse/Expand REAPER Folder Submix")
                        .clicked()
                    {
                        project.tracks[t_idx].folder_collapsed =
                            !project.tracks[t_idx].folder_collapsed;
                    }
                }
                if self.renaming_track == Some(track_id) {
                    let te = ui.text_edit_singleline(&mut project.tracks[t_idx].name);
                    if te.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.renaming_track = None;
                        actions.push(ArrangementAction::ProjectMutated("Rename Track"));
                    } else {
                        te.request_focus();
                    }
                } else {
                    let lbl = egui::RichText::new(&project.tracks[t_idx].name)
                        .size(12.0)
                        .strong()
                        .color(TEXT_MAIN);
                    let l_resp = ui
                        .add(egui::Label::new(lbl).truncate().sense(Sense::click()))
                        .on_hover_text("Click to select • Double-click or Right-click to rename, color, reorder, or delete");
                    if l_resp.double_clicked() {
                        self.renaming_track = Some(track_id);
                    } else if l_resp.clicked() {
                        actions.push(ArrangementAction::SelectTrack(track_id));
                    }
                }
            });

            // Row 2: M / S / R / I + FX count + WAV import
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let trk = &mut project.tracks[t_idx];
                if pill_toggle(ui, "M", trk.mute, ACCENT_AMBER)
                    .on_hover_text("Mute Track")
                    .clicked()
                {
                    trk.mute = !trk.mute;
                    actions.push(ArrangementAction::ProjectMutated("Toggle Mute"));
                }
                if pill_toggle(ui, "S", trk.solo, ACCENT_BLUE)
                    .on_hover_text("Solo Track")
                    .clicked()
                {
                    trk.solo = !trk.solo;
                    actions.push(ArrangementAction::ProjectMutated("Toggle Solo"));
                }
                let arm_tip = match trk.input_source {
                    InputSource::MidiAll => "Record Arm — MIDI only (no mic)",
                    InputSource::AudioMono { .. } => "Record Arm — Mic / Line (mono)",
                    InputSource::AudioAndMidi { .. } => "Record Arm — Audio + MIDI",
                    InputSource::ResampleTrackOutput => "Record Arm — Resample this track's output",
                };
                if pill_toggle(ui, "R", trk.record_arm, ACCENT_RED)
                    .on_hover_text(arm_tip)
                    .clicked()
                {
                    trk.record_arm = !trk.record_arm;
                    actions.push(ArrangementAction::ProjectMutated("Toggle Record Arm"));
                }
                if pill_toggle(
                    ui,
                    "Ch",
                    trk.follow_chord_track,
                    ACCENT_PURPLE,
                )
                .on_hover_text("Follow Chord Track — snap MIDI pitches to active chord tones")
                .clicked()
                {
                    trk.follow_chord_track = !trk.follow_chord_track;
                    actions.push(ArrangementAction::ProjectMutated("Toggle Chord Follow"));
                }
                let fx_txt = format!("FX {}", trk.fx_chain.len());
                if pill_toggle(ui, &fx_txt, !trk.fx_chain.is_empty(), ACCENT_TEAL)
                    .on_hover_text("Open FX & Modulators for this track")
                    .clicked()
                {
                    actions.push(ArrangementAction::OpenFxPanel(track_id));
                }
                if pill_toggle(ui, "A", trk.show_automation, ACCENT_PURPLE)
                    .on_hover_text("Show / Add Curved Bezier Automation Lanes (Left-click: Add/Drag, Shift+Drag: Curve Tension, Right-click: Delete)")
                    .clicked()
                {
                    if !trk.show_automation && trk.automation_lanes.is_empty() {
                        trk.ensure_automation_lane(ferric_model::AutomationTarget::Volume);
                    } else {
                        trk.show_automation = !trk.show_automation;
                    }
                    actions.push(ArrangementAction::ProjectMutated("Toggle Automation Lanes"));
                }
                if pill_toggle(ui, "❄", trk.frozen, ACCENT_CYAN)
                    .on_hover_text("1-Click Reversible Track Freeze (Renders 32-bit Stem & frees 100% DSP CPU!)")
                    .clicked()
                {
                    actions.push(ArrangementAction::ToggleFreezeTrack(track_id));
                }
                if ui
                    .chrome_small_button("📂")
                    .on_hover_text("Import WAV/FLAC/MP3/OGG Audio Clip onto this Track")
                    .clicked()
                {
                    actions.push(ArrangementAction::ImportAudioToTrack(track_id));
                }
            });

            // Row 3: Input source (mic / MIDI / interface) — obvious on every track
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.label(egui::RichText::new("In").size(10.0).color(TEXT_DIM));
                let mut changed = false;
                let short = project.tracks[t_idx].input_source.short_label();
                egui::ComboBox::from_id_salt(("trk_in", track_id))
                    .selected_text(short)
                    .width(96.0)
                    .show_ui(ui, |ui| {
                        ui.set_min_width(180.0);
                        for src in InputSource::all_hardware_choices() {
                            if ui
                                .selectable_value(
                                    &mut project.tracks[t_idx].input_source,
                                    src,
                                    src.label(),
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    })
                    .response
                    .on_hover_text(project.tracks[t_idx].input_source.label());
                if changed {
                    actions.push(ArrangementAction::ProjectMutated("Change Track Input"));
                }
                let mon = &mut project.tracks[t_idx].input_monitoring;
                if pill_toggle(ui, "Mon", *mon, ACCENT_TEAL)
                    .on_hover_text("Input monitoring — hear mic/MIDI through the track")
                    .clicked()
                {
                    *mon = !*mon;
                    actions.push(ArrangementAction::ProjectMutated("Toggle Input Monitor"));
                }
            });

            // Row 4: Compact Volume dB drag + Pan + Live horizontal peak bar
            ui.horizontal(|ui| {
                let trk = &mut project.tracks[t_idx];
                let r1 = ui.add(
                    egui::DragValue::new(&mut trk.gain_db)
                        .speed(0.15)
                        .range(-60.0..=12.0)
                        .suffix(" dB"),
                );
                let r2 = ui.add(
                    egui::DragValue::new(&mut trk.pan)
                        .speed(0.02)
                        .range(-1.0..=1.0)
                        .prefix("P "),
                );
                if r1.changed() || r2.changed() {
                    actions.push(ArrangementAction::ProjectMutated("Adjust Track Mix"));
                }

                // Live Track Peak mini-meter
                let pk = telem
                    .track_meters
                    .get(t_idx)
                    .map(|m| m.peak_l_db.max(m.peak_r_db))
                    .unwrap_or(-96.0);
                let (mrect, _) = ui.allocate_exact_size(Vec2::new(54.0, 7.0), Sense::hover());
                ui.painter().rect_filled(mrect, 2, BG_DEEP);
                let norm = ((pk + 60.0) / 66.0).clamp(0.0, 1.0);
                if norm > 0.01 {
                    let fill_r = Rect::from_min_size(
                        mrect.min,
                        Vec2::new(mrect.width() * norm, mrect.height()),
                    );
                    let col = if pk > -0.5 { ACCENT_RED } else { t_col };
                    ui.painter().rect_filled(fill_r, 2, col);
                }
            });
        });
    }

    fn render_session_cells(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        project: &mut Project,
        t_idx: usize,
        col_w: f32,
        actions: &mut Vec<ArrangementAction>,
    ) {
        ui.painter().rect_filled(rect, 0, BG_PANEL);
        let num_scenes = project.scenes.len();
        project.tracks[t_idx].ensure_scene_count(num_scenes);
        let t_col = track_color32(project.tracks[t_idx].color);
        let active_slot = project.tracks[t_idx].active_session_slot;

        for s_idx in 0..num_scenes {
            let cell = Rect::from_min_size(
                Pos2::new(rect.left() + s_idx as f32 * col_w + 2.0, rect.top() + 5.0),
                Vec2::new(col_w - 5.0, rect.height().min(68.0) - 10.0),
            );
            let resp = ui.interact(
                cell,
                ui.id().with(("sess_slot", project.tracks[t_idx].id, s_idx)),
                Sense::click(),
            );

            if let Some(slot) = &project.tracks[t_idx].session_slots[s_idx] {
                let is_playing_slot = active_slot == Some(s_idx);
                let fill = if is_playing_slot {
                    t_col.gamma_multiply(0.55)
                } else {
                    t_col.gamma_multiply(0.22)
                };
                ui.painter().rect_filled(cell, 4, fill);
                ui.painter().rect_stroke(
                    cell,
                    4,
                    Stroke::new(if is_playing_slot { 1.8_f32 } else { 1.0_f32 }, t_col),
                    StrokeKind::Inside,
                );
                let icon = if is_playing_slot { "⏹" } else { "▶" };
                ui.painter().text(
                    cell.center(),
                    Align2::CENTER_CENTER,
                    format!("{} {}", icon, slot.name),
                    FontId::proportional(10.0),
                    TEXT_MAIN,
                );
                if resp.clicked() {
                    project.tracks[t_idx].active_session_slot = if is_playing_slot {
                        None
                    } else {
                        Some(s_idx)
                    };
                    actions.push(ArrangementAction::ProjectMutated("Toggle Session Clip"));
                }
            } else {
                ui.painter()
                    .rect_filled(cell, 4, Color32::from_rgb(18, 20, 26));
                ui.painter().rect_stroke(
                    cell,
                    4,
                    Stroke::new(1.0_f32, Color32::from_rgb(35, 38, 48)),
                    StrokeKind::Inside,
                );
                if resp.hovered() {
                    ui.painter().text(
                        cell.center(),
                        Align2::CENTER_CENTER,
                        "+ Loop",
                        FontId::proportional(10.0),
                        TEXT_DIM,
                    );
                }
                if resp.clicked() {
                    // Create a fresh 4-bar MIDI loop slot for instant sketching!
                    project.tracks[t_idx].session_slots[s_idx] = Some(SessionSlot::new_midi(
                        format!("Sketch {}", s_idx + 1),
                        16.0,
                        vec![
                            MidiNote::new(60, 100, 0.0, 1.0),
                            MidiNote::new(64, 95, 1.0, 1.0),
                            MidiNote::new(67, 100, 2.0, 1.0),
                            MidiNote::new(72, 105, 3.0, 1.0),
                        ],
                    ));
                    actions.push(ArrangementAction::ProjectMutated("Create Session Sketch Loop"));
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_track_timeline_lane(
        &mut self,
        ui: &mut Ui,
        lane_rect: Rect,
        comp_area_rect: Rect,
        row_bounds: &[(TrackId, f32, f32)],
        project: &mut Project,
        t_idx: usize,
        selected_clip: Option<(TrackId, ClipId)>,
        telem: &EngineTelemetry,
        actions: &mut Vec<ArrangementAction>,
    ) {
        let bpb = project.time_signature.beats_per_bar();
        let max_vis_beat = self.scroll_beat + (lane_rect.width() / self.pixels_per_beat) as f64;
        let mut beat = self.scroll_beat.floor();
        while beat <= max_vis_beat {
            let x = lane_rect.left() + ((beat - self.scroll_beat) as f32 * self.pixels_per_beat);
            if x >= lane_rect.left() && x <= lane_rect.right() {
                let is_bar = (beat.rem_euclid(bpb)).abs() < 1e-4;
                let col = if is_bar {
                    Color32::from_rgb(44, 48, 60)
                } else {
                    Color32::from_rgb(29, 32, 41)
                };
                ui.painter().line_segment(
                    [Pos2::new(x, lane_rect.top()), Pos2::new(x, comp_area_rect.bottom())],
                    Stroke::new(1.0_f32, col),
                );
            }
            beat += 1.0;
        }

        let track_id = project.tracks[t_idx].id;
        let t_col = track_color32(project.tracks[t_idx].color);

        // Background click in DrawPencil mode -> create new 4-bar MIDI clip!
        // Background drag in SmartPointer mode -> Rubber-Band Marquee Multi-Clip Selection!
        let bg_resp = ui.interact(
            lane_rect,
            ui.id().with(("lane_bg", track_id)),
            Sense::click_and_drag(),
        );
        if bg_resp.double_clicked() || (bg_resp.clicked() && self.tool == EditTool::DrawPencil) {
            if let Some(pos) = bg_resp.interact_pointer_pos() {
                let raw_b =
                    self.scroll_beat + ((pos.x - lane_rect.left()) / self.pixels_per_beat) as f64;
                let start_b = project.snap_grid.snap(raw_b, project.time_signature);
                let new_clip = TimelineClip::new_midi("New MIDI Clip", start_b, 4.0, Vec::new());
                let cid = new_clip.id;
                project.tracks[t_idx].clips.push(new_clip);
                actions.push(ArrangementAction::ProjectMutated("Draw MIDI Clip"));
                actions.push(ArrangementAction::OpenPianoRoll {
                    track_id,
                    clip_id: cid,
                });
            }
        } else if bg_resp.drag_started() && self.tool == EditTool::SmartPointer {
            if let Some(pos) = bg_resp.interact_pointer_pos() {
                self.marquee_start_pos = Some(pos);
                self.marquee_current_pos = Some(pos);
            }
        } else if bg_resp.dragged() && self.marquee_start_pos.is_some() {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                self.marquee_current_pos = Some(pos);
            }
        } else if bg_resp.clicked() {
            // Empty lane click: move playhead here + select track.
            if let Some(pos) = bg_resp.interact_pointer_pos() {
                let raw_b =
                    self.scroll_beat + ((pos.x - lane_rect.left()) / self.pixels_per_beat) as f64;
                let beat = if ui.input(|i| i.modifiers.shift) {
                    raw_b.max(0.0)
                } else {
                    project.snap_grid.snap(raw_b, project.time_signature)
                };
                actions.push(ArrangementAction::Seek(beat));
            }
            if !ui.input(|i| i.modifiers.shift) {
                self.multi_selected_clips.clear();
            }
            actions.push(ArrangementAction::SelectTrack(track_id));
        }

        // Render each TimelineClip on this track
        let clip_count = project.tracks[t_idx].clips.len();
        for c_idx in 0..clip_count {
            let clip_id = project.tracks[t_idx].clips[c_idx].id;
            let start_b = project.tracks[t_idx].clips[c_idx].start_beat;
            let len_b = project.tracks[t_idx].clips[c_idx].length_beats;

            let x0 = lane_rect.left() + ((start_b - self.scroll_beat) as f32 * self.pixels_per_beat);
            let x1 = x0 + (len_b as f32 * self.pixels_per_beat);
            if x1 < lane_rect.left() || x0 > lane_rect.right() {
                continue;
            }

            let clip_rect = Rect::from_min_max(
                Pos2::new(x0.max(lane_rect.left()), lane_rect.top() + 3.0),
                Pos2::new(x1.min(lane_rect.right()), lane_rect.bottom() - 3.0),
            );
            self.clip_hit_rects.push((track_id, clip_id, clip_rect));
            let is_sel = selected_clip == Some((track_id, clip_id))
                || self.multi_selected_clips.contains(&(track_id, clip_id));

            let clip_resp = ui.interact(
                clip_rect,
                ui.id().with(("tl_clip", clip_id)),
                Sense::click_and_drag(),
            );

            // Paint Clip Card + Waveform / MIDI Notes + Fade Curves
            draw_clip_visual(
                ui.painter(),
                clip_rect,
                &project.tracks[t_idx].clips[c_idx],
                t_col,
                is_sel,
            );

            // Interactions: Select (Shift+Click adds to Multi-Selection!), Double-Click Piano Roll, Split Razor, Drag!
            if clip_resp.clicked() {
                if ui.input(|i| i.modifiers.shift) {
                    if let Some(pos) = self
                        .multi_selected_clips
                        .iter()
                        .position(|&x| x == (track_id, clip_id))
                    {
                        self.multi_selected_clips.remove(pos);
                    } else {
                        if let Some(prev) = selected_clip {
                            if !self.multi_selected_clips.contains(&prev) {
                                self.multi_selected_clips.push(prev);
                            }
                        }
                        self.multi_selected_clips.push((track_id, clip_id));
                    }
                } else if !self.multi_selected_clips.contains(&(track_id, clip_id)) {
                    self.multi_selected_clips.clear();
                }
                actions.push(ArrangementAction::SelectClip { track_id, clip_id });
                if self.tool == EditTool::SplitRazor {
                    if let Some(pos) = clip_resp.interact_pointer_pos() {
                        let raw_b = self.scroll_beat
                            + ((pos.x - lane_rect.left()) / self.pixels_per_beat) as f64;
                        let sb = project.snap_grid.snap(raw_b, project.time_signature);
                        actions.push(ArrangementAction::SplitClip {
                            track_id,
                            clip_id,
                            beat: sb,
                        });
                    }
                }
            }
            if clip_resp.double_clicked() && project.tracks[t_idx].clips[c_idx].is_midi() {
                actions.push(ArrangementAction::OpenPianoRoll { track_id, clip_id });
            }

            if clip_resp.drag_started() && self.tool == EditTool::SmartPointer {
                if let Some(pos) = clip_resp.interact_pointer_pos() {
                    let near_left = (pos.x - x0).abs() < 10.0;
                    let near_right = (x1 - pos.x).abs() < 10.0;
                    let kind = if near_left && len_b > 0.5 {
                        DragKind::ResizeLeft
                    } else if near_right {
                        DragKind::ResizeRight
                    } else {
                        DragKind::Move
                    };
                    let init_v = if near_right { len_b } else { start_b };
                    self.drag_clip = Some((track_id, clip_id, kind, init_v));
                    actions.push(ArrangementAction::SelectClip { track_id, clip_id });
                }
            }

            if let Some((dt, dc, kind, init_v)) = self.drag_clip {
                if dt == track_id && dc == clip_id && clip_resp.dragged() {
                    let d_beats = (clip_resp.drag_delta().x / self.pixels_per_beat) as f64;
                    match kind {
                        DragKind::Move => {
                            if self.multi_selected_clips.len() > 1
                                && self.multi_selected_clips.contains(&(track_id, clip_id))
                            {
                                // Group Move ALL Marquee-Selected Clips across all tracks simultaneously!
                                for &(mt, mc) in &self.multi_selected_clips {
                                    if let Some(c) =
                                        project.find_track_mut(mt).and_then(|t| t.find_clip_mut(mc))
                                    {
                                        c.start_beat = (c.start_beat + d_beats).max(0.0);
                                    }
                                }
                            } else {
                                let clip_mut = &mut project.tracks[t_idx].clips[c_idx];
                                clip_mut.start_beat = (clip_mut.start_beat + d_beats).max(0.0);

                                // Vertical Track-to-Track Clip Drag!
                                if let Some(ptr) = ui.input(|i| i.pointer.interact_pos()) {
                                    if let Some(&(hover_tid, _, _)) = row_bounds
                                        .iter()
                                        .find(|&&(_, y0, y1)| ptr.y >= y0 && ptr.y <= y1)
                                    {
                                        if hover_tid != track_id {
                                            self.drag_clip =
                                                Some((hover_tid, clip_id, kind, init_v));
                                            actions.push(ArrangementAction::MoveClipToTrack {
                                                from_track: track_id,
                                                to_track: hover_tid,
                                                clip_id,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        DragKind::ResizeLeft => {
                            let clip_mut = &mut project.tracks[t_idx].clips[c_idx];
                            let max_trim = (clip_mut.length_beats - 0.25).max(0.0);
                            let trim = d_beats.clamp(-clip_mut.start_beat, max_trim);
                            clip_mut.start_beat += trim;
                            clip_mut.length_beats = (clip_mut.length_beats - trim).max(0.25);
                            clip_mut.slip_offset_beats =
                                (clip_mut.slip_offset_beats + trim * clip_mut.stretch_rate as f64)
                                    .max(0.0);
                        }
                        DragKind::ResizeRight => {
                            let clip_mut = &mut project.tracks[t_idx].clips[c_idx];
                            clip_mut.length_beats = (clip_mut.length_beats + d_beats).max(0.25);
                        }
                    }
                    actions.push(ArrangementAction::ProjectMutated("Move/Trim Clip"));
                }
            }

            // Right-Click REAPER Media Item Context Menu
            let playhead = telem.playhead_beat;
            clip_resp.context_menu(|ui| {
                ui.set_min_width(220.0);
                ui.label(egui::RichText::new("Clip name").size(10.5).color(TEXT_DIM));
                if ui
                    .text_edit_singleline(&mut project.tracks[t_idx].clips[c_idx].name)
                    .changed()
                {
                    actions.push(ArrangementAction::ProjectMutated("Rename Clip"));
                }
                ui.separator();
                if project.tracks[t_idx].clips[c_idx].is_midi() {
                    if ui.chrome_button("Open Piano Roll").clicked() {
                        actions.push(ArrangementAction::OpenPianoRoll {
                            track_id,
                            clip_id,
                        });
                        ui.close_menu();
                    }
                }
                if ui.chrome_button("Split at Playhead").clicked() {
                    actions.push(ArrangementAction::SplitClip {
                        track_id,
                        clip_id,
                        beat: playhead,
                    });
                    ui.close_menu();
                }
                if ui.chrome_button("Duplicate Clip").clicked() {
                    actions.push(ArrangementAction::DuplicateClip { track_id, clip_id });
                    ui.close_menu();
                }
                let has_takes = project.tracks[t_idx].clips[c_idx].takes.len() > 1;
                if has_takes {
                    let showing = project.tracks[t_idx].clips[c_idx].show_take_lanes;
                    let txt = if showing {
                        "▲ Hide Multi-Take Comp Lanes"
                    } else {
                        "▼ Show Multi-Take Swipe-Comp Lanes"
                    };
                if ui.chrome_button(txt).clicked() {
                        project.tracks[t_idx].clips[c_idx].show_take_lanes = !showing;
                        ui.close_menu();
                    }
                }
                if ui.chrome_button("➕ Duplicate Active Take as New Take Variation").clicked() {
                    let c = &mut project.tracks[t_idx].clips[c_idx];
                    if let Some(cur) = c.takes.get(c.active_take).cloned() {
                        let new_num = c.takes.len() + 1;
                        let mut nt = Take {
                            id: uuid::Uuid::new_v4(),
                            name: format!("Take {}", new_num),
                            rating: 4,
                            content: cur.content,
                        };
                        if let TakeContent::Audio { buffer } = &mut nt.content {
                            *buffer = buffer.normalized(-1.5);
                        }
                        c.takes.push(nt);
                        c.show_take_lanes = true;
                        actions.push(ArrangementAction::ProjectMutated("Add Take Lane"));
                    }
                    ui.close_menu();
                }
                if !project.tracks[t_idx].clips[c_idx].is_midi() {
                    if ui.chrome_button("🔊 Normalize Audio Take (-1.0 dB)").clicked() {
                        let c = &mut project.tracks[t_idx].clips[c_idx];
                        let at = c.active_take;
                        if let Some(Take {
                            content: TakeContent::Audio { buffer },
                            ..
                        }) = c.takes.get_mut(at)
                        {
                            *buffer = buffer.normalized(-1.0);
                            actions.push(ArrangementAction::ProjectMutated("Normalize Audio"));
                        }
                        ui.close_menu();
                    }
                    if ui.chrome_button("⏪ Reverse Audio Take").clicked() {
                        let c = &mut project.tracks[t_idx].clips[c_idx];
                        let at = c.active_take;
                        if let Some(Take {
                            content: TakeContent::Audio { buffer },
                            ..
                        }) = c.takes.get_mut(at)
                        {
                            *buffer = buffer.reversed();
                            actions.push(ArrangementAction::ProjectMutated("Reverse Audio"));
                        }
                        ui.close_menu();
                    }
                    if ui
                        .chrome_button("✂ Split to 4 Stems (Drums / Bass / Mids / Highs)")
                        .on_hover_text(
                            "Offline HPSS + Linkwitz-Riley stem split into four new audio tracks",
                        )
                        .clicked()
                    {
                        actions.push(ArrangementAction::SplitAudioToStems {
                            track_id,
                            clip_id,
                        });
                        ui.close_menu();
                    }
                } else if ui.chrome_button("🎹 Quantize MIDI Notes (1/16)").clicked() {
                    project.quantize_midi_clip(track_id, clip_id, 0.25);
                    actions.push(ArrangementAction::ProjectMutated("Quantize MIDI 1/16"));
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .chrome_danger_button(egui::RichText::new("🗑 Delete Clip").color(ACCENT_RED))
                    .clicked()
                {
                    actions.push(ArrangementAction::DeleteClip { track_id, clip_id });
                    ui.close_menu();
                }
            });

            // Render Expandable Multi-Take Swipe-Comping Lanes right under the clip!
            if project.tracks[t_idx].clips[c_idx].show_take_lanes
                && project.tracks[t_idx].clips[c_idx].takes.len() > 1
            {
                let num_takes = project.tracks[t_idx].clips[c_idx].takes.len();
                for take_i in 0..num_takes {
                    let ly0 = comp_area_rect.top() + (take_i as f32) * 24.0;
                    let t_rect = Rect::from_min_max(
                        Pos2::new(x0.max(lane_rect.left()), ly0 + 1.0),
                        Pos2::new(x1.min(lane_rect.right()), ly0 + 23.0),
                    );
                    let t_resp = ui.interact(
                        t_rect,
                        ui.id().with(("comp_lane", clip_id, take_i)),
                        Sense::click_and_drag(),
                    );

                    ui.painter()
                        .rect_filled(t_rect, 3, Color32::from_rgb(16, 18, 24));

                    // Highlight active swipe-comp regions belonging to `take_i`!
                    let clip_ref = &project.tracks[t_idx].clips[c_idx];
                    if clip_ref.comp_segments.is_empty() {
                        if clip_ref.active_take == take_i {
                            ui.painter()
                                .rect_filled(t_rect, 3, t_col.gamma_multiply(0.38));
                        }
                    } else {
                        for seg in &clip_ref.comp_segments {
                            if seg.take_index == take_i {
                                let sx0 = x0 + seg.start_beat as f32 * self.pixels_per_beat;
                                let sx1 = x0 + seg.end_beat as f32 * self.pixels_per_beat;
                                let srect = Rect::from_min_max(
                                    Pos2::new(sx0.max(t_rect.left()), t_rect.top()),
                                    Pos2::new(sx1.min(t_rect.right()), t_rect.bottom()),
                                );
                                if srect.width() > 0.5 {
                                    ui.painter()
                                        .rect_filled(srect, 3, t_col.gamma_multiply(0.45));
                                    ui.painter().rect_stroke(
                                        srect,
                                        3,
                                        Stroke::new(1.2_f32, t_col),
                                        StrokeKind::Inside,
                                    );
                                }
                            }
                        }
                    }

                    let stars = "★".repeat(clip_ref.takes[take_i].rating as usize);
                    ui.painter().text(
                        Pos2::new(t_rect.left() + 6.0, t_rect.center().y),
                        Align2::LEFT_CENTER,
                        format!("{}  {}", clip_ref.takes[take_i].name, stars),
                        FontId::proportional(10.0),
                        TEXT_MAIN,
                    );

                    // Click or Swipe-Drag a beat phrase on this Take Lane to comp it in!
                    if t_resp.clicked() || t_resp.dragged() {
                        if let Some(pos) = t_resp.interact_pointer_pos() {
                            let local_b = ((pos.x - x0) / self.pixels_per_beat) as f64;
                            let bar_step = 2.0_f64;
                            let (cs, ce) = if ui.input(|i| i.modifiers.shift) {
                                ((local_b - 0.25).max(0.0), local_b + 0.25)
                            } else {
                                let s = (local_b / bar_step).floor() * bar_step;
                                (s, s + bar_step)
                            };
                            project.tracks[t_idx].clips[c_idx].set_comp_region(cs, ce, take_i);
                            actions.push(ArrangementAction::ProjectMutated("Swipe Comp Take"));
                        }
                    }
                }
            }
        }

        // Draw Visual Equal-Power Auto-Crossfade `X` Overlays wherever two Audio Clips overlap on this Track!
        for i in 0..clip_count {
            if project.tracks[t_idx].clips[i].muted || project.tracks[t_idx].clips[i].is_midi() {
                continue;
            }
            for j in (i + 1)..clip_count {
                if project.tracks[t_idx].clips[j].muted || project.tracks[t_idx].clips[j].is_midi()
                {
                    continue;
                }
                let ci = &project.tracks[t_idx].clips[i];
                let cj = &project.tracks[t_idx].clips[j];
                let ov0 = ci.start_beat.max(cj.start_beat);
                let ov1 = ci.end_beat().min(cj.end_beat());
                if ov1 - ov0 > 0.02 {
                    let ox0 =
                        lane_rect.left() + ((ov0 - self.scroll_beat) as f32 * self.pixels_per_beat);
                    let ox1 =
                        lane_rect.left() + ((ov1 - self.scroll_beat) as f32 * self.pixels_per_beat);
                    if ox1 >= lane_rect.left() && ox0 <= lane_rect.right() {
                        let xrect = Rect::from_min_max(
                            Pos2::new(ox0.max(lane_rect.left()), lane_rect.top() + 18.0),
                            Pos2::new(ox1.min(lane_rect.right()), lane_rect.bottom() - 4.0),
                        );
                        ui.painter().rect_filled(
                            xrect,
                            2,
                            Color32::from_rgba_unmultiplied(240, 182, 62, 34),
                        );
                        let steps = 16usize;
                        let mut fade_out_pts = Vec::with_capacity(steps + 1);
                        let mut fade_in_pts = Vec::with_capacity(steps + 1);
                        for s in 0..=steps {
                            let t = s as f32 / steps as f32;
                            let px = xrect.left() + t * xrect.width();
                            let g_out = (t * std::f32::consts::FRAC_PI_2).cos();
                            let g_in = (t * std::f32::consts::FRAC_PI_2).sin();
                            fade_out_pts.push(Pos2::new(px, xrect.bottom() - g_out * xrect.height()));
                            fade_in_pts.push(Pos2::new(px, xrect.bottom() - g_in * xrect.height()));
                        }
                        ui.painter().add(egui::Shape::line(
                            fade_out_pts,
                            Stroke::new(1.5_f32, ACCENT_AMBER),
                        ));
                        ui.painter().add(egui::Shape::line(
                            fade_in_pts,
                            Stroke::new(1.5_f32, ACCENT_TEAL),
                        ));
                    }
                }
            }
        }
    }

    fn render_chord_lane(
        &mut self,
        ui: &mut Ui,
        avail_w: f32,
        timeline_x0_rel: f32,
        timeline_w: f32,
        project: &mut Project,
        playhead_beat: f64,
        actions: &mut Vec<ArrangementAction>,
    ) -> f32 {
        let lane_h = 56.0_f32;
        let (lane_rect, lane_resp) = ui.allocate_exact_size(
            Vec2::new(avail_w, lane_h),
            Sense::click_and_drag(),
        );
        ui.painter().rect_filled(lane_rect, 0.0, Color32::from_rgb(24, 22, 32));
        ui.painter().hline(
            lane_rect.x_range(),
            lane_rect.bottom(),
            Stroke::new(1.0_f32, BORDER_SUBTLE),
        );

        let header = Rect::from_min_max(
            lane_rect.min,
            Pos2::new(lane_rect.left() + timeline_x0_rel, lane_rect.bottom()),
        );
        ui.painter().rect_filled(header, 0.0, BG_PANEL);
        ui.painter().text(
            Pos2::new(header.left() + 6.0, header.top() + 4.0),
            Align2::LEFT_TOP,
            "CHORDS",
            FontId::proportional(10.0),
            ACCENT_PURPLE,
        );

        // Inline header controls (not a floating Area — those drifted off the lane).
        let mut gen_voicing = VoicingStyle::OpenSpreadPad;
        let controls_rect = Rect::from_min_max(
            Pos2::new(header.left() + 4.0, header.top() + 16.0),
            Pos2::new(header.right() - 4.0, header.bottom() - 2.0),
        );
        ui.allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(controls_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.set_clip_rect(controls_rect);
                ui.spacing_mut().item_spacing.x = 3.0;
                if ui
                    .chrome_small_button("CoF+")
                    .on_hover_text("Insert next Circle-of-Fifths major chord at playhead")
                    .clicked()
                {
                    let beat = project
                        .snap_grid
                        .snap(playhead_beat.max(0.0), project.time_signature);
                    let idx = CIRCLE_OF_FIFTHS_MAJOR
                        .iter()
                        .position(|&p| p == project.root_pitch_class)
                        .unwrap_or(0);
                    let next = CIRCLE_OF_FIFTHS_MAJOR[(idx + 1) % 12];
                    project.upsert_chord_event(beat, 4.0, next, ChordQuality::Maj7);
                    actions.push(ArrangementAction::ProjectMutated("Insert CoF Chord"));
                }
                if ui
                    .chrome_small_button("CoFm")
                    .on_hover_text("Insert relative minor from Circle of Fifths")
                    .clicked()
                {
                    let beat = project
                        .snap_grid
                        .snap(playhead_beat.max(0.0), project.time_signature);
                    let idx = CIRCLE_OF_FIFTHS_MINOR
                        .iter()
                        .position(|&p| p == project.root_pitch_class)
                        .unwrap_or(0);
                    let next = CIRCLE_OF_FIFTHS_MINOR[(idx + 1) % 12];
                    project.upsert_chord_event(beat, 4.0, next, ChordQuality::Min9);
                    actions.push(ArrangementAction::ProjectMutated("Insert CoF Minor"));
                }
                egui::ComboBox::from_id_salt("chord_prog_preset")
                    .selected_text("Prog…")
                    .width(72.0)
                    .show_ui(ui, |ui| {
                        for preset in ProgressionPreset::ALL {
                            if ui.chrome_button(preset.label()).clicked() {
                                let start = project
                                    .snap_grid
                                    .snap(playhead_beat.max(0.0), project.time_signature);
                                let events = preset.generate(
                                    project.root_pitch_class,
                                    true,
                                    start,
                                    4.0,
                                );
                                for ev in events {
                                    project.upsert_chord_event(
                                        ev.start_beat,
                                        ev.length_beats,
                                        ev.root_pc,
                                        ev.quality,
                                    );
                                }
                                actions.push(ArrangementAction::ProjectMutated(
                                    "Apply Progression Preset",
                                ));
                            }
                        }
                    });
                if ui
                    .chrome_small_button("Gen")
                    .on_hover_text("Spawn Pad + Bass + Arp from chord lane (reversible)")
                    .clicked()
                {
                    actions.push(ArrangementAction::GenerateArr {
                        spawn_pad: true,
                        spawn_bass: true,
                        spawn_arp: true,
                        section_idx: self.storyboard_section,
                    });
                }
                let _ = &mut gen_voicing;
            },
        );

        let tl = Rect::from_min_max(
            Pos2::new(lane_rect.left() + timeline_x0_rel, lane_rect.top()),
            lane_rect.max,
        );
        let beat_to_x = |b: f64| tl.left() + ((b - self.scroll_beat) as f32) * self.pixels_per_beat;
        let x_to_beat = |x: f32| {
            self.scroll_beat + ((x - tl.left()) / self.pixels_per_beat) as f64
        };

        // Draw chord blocks
        let chords_snapshot: Vec<_> = project.chord_track.clone();
        for ch in &chords_snapshot {
            let x0 = beat_to_x(ch.start_beat);
            let x1 = beat_to_x(ch.end_beat());
            if x1 < tl.left() || x0 > tl.right() {
                continue;
            }
            let crect = Rect::from_min_max(
                Pos2::new(x0.max(tl.left()) + 1.0, tl.top() + 6.0),
                Pos2::new(x1.min(tl.right()) - 1.0, tl.bottom() - 6.0),
            );
            let hue = ch.root_pc as f32 / 12.0;
            let col = Color32::from_rgb(
                (120.0 + 100.0 * (hue * std::f32::consts::TAU).sin()) as u8,
                (90.0 + 80.0 * ((hue + 0.33) * std::f32::consts::TAU).sin()) as u8,
                (150.0 + 90.0 * ((hue + 0.66) * std::f32::consts::TAU).sin()) as u8,
            );
            ui.painter().rect_filled(crect, 3.0, col.gamma_multiply(0.55));
            ui.painter().rect_stroke(
                crect,
                3.0,
                Stroke::new(1.0_f32, col),
                StrokeKind::Outside,
            );
            ui.painter().text(
                crect.center(),
                Align2::CENTER_CENTER,
                ch.symbol(),
                FontId::proportional(12.0),
                TEXT_MAIN,
            );
        }

        // Click empty area to insert chord; right-click for quality menu on existing
        if lane_resp.clicked() {
            if let Some(pos) = lane_resp.interact_pointer_pos() {
                if tl.contains(pos) {
                    let beat = project
                        .snap_grid
                        .snap(x_to_beat(pos.x).max(0.0), project.time_signature);
                    // If clicking an existing chord, cycle quality; else insert
                    let hit = project
                        .chord_track
                        .iter()
                        .position(|c| beat >= c.start_beat && beat < c.end_beat());
                    if let Some(i) = hit {
                        let quals = ChordQuality::ALL;
                        let cur = project.chord_track[i].quality;
                        let idx = quals.iter().position(|q| *q == cur).unwrap_or(0);
                        project.chord_track[i].quality = quals[(idx + 1) % quals.len()];
                        actions.push(ArrangementAction::ProjectMutated("Cycle Chord Quality"));
                    } else {
                        project.upsert_chord_event(
                            beat,
                            4.0,
                            project.root_pitch_class,
                            ChordQuality::Min9,
                        );
                        actions.push(ArrangementAction::ProjectMutated("Insert Chord"));
                    }
                }
            }
        }
        if lane_resp.secondary_clicked() {
            if let Some(pos) = lane_resp.interact_pointer_pos() {
                if tl.contains(pos) {
                    let beat = x_to_beat(pos.x);
                    project
                        .chord_track
                        .retain(|c| !(beat >= c.start_beat && beat < c.end_beat()));
                    actions.push(ArrangementAction::ProjectMutated("Delete Chord"));
                }
            }
        }

        // Key label
        let key_lbl = format!(
            "{} {}",
            NOTE_PITCH_CLASSES[project.root_pitch_class as usize % 12],
            project.musical_scale.label()
        );
        ui.painter().text(
            Pos2::new(header.left() + 8.0, header.bottom() - 10.0),
            Align2::LEFT_CENTER,
            key_lbl,
            FontId::proportional(9.0),
            TEXT_DIM,
        );

        let _ = timeline_w;
        lane_rect.bottom()
    }

    fn render_conductor_lane(
        &mut self,
        ui: &mut Ui,
        avail_w: f32,
        timeline_x0_rel: f32,
        timeline_w: f32,
        project: &mut Project,
        actions: &mut Vec<ArrangementAction>,
    ) -> f32 {
        let cond_h = 56.0_f32;
        let (row_rect, _) = ui.allocate_exact_size(Vec2::new(avail_w, cond_h), Sense::hover());
        ui.painter()
            .rect_filled(row_rect, 0, Color32::from_rgb(24, 21, 18));
        ui.painter().line_segment(
            [row_rect.left_bottom(), row_rect.right_bottom()],
            Stroke::new(1.0_f32, ACCENT_AMBER.gamma_multiply(0.55)),
        );

        // Left Header Controls for Conductor Lane
        let hdr_rect = Rect::from_min_size(row_rect.min, Vec2::new(236.0, cond_h));
        let mut hdr_ui = ui.new_child(egui::UiBuilder::new().max_rect(hdr_rect.shrink(5.0)));
        hdr_ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("🎼 CONDUCTOR: TEMPO & METER")
                    .size(10.5)
                    .strong()
                    .color(ACCENT_AMBER),
            );
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Base Meter:").size(10.0).color(TEXT_DIM));
                let mut num = project.time_signature.numerator;
                let mut den = project.time_signature.denominator;
                if ui
                    .add(egui::DragValue::new(&mut num).range(1..=16).speed(0.1))
                    .changed()
                {
                    project.time_signature.numerator = num;
                    actions.push(ArrangementAction::ProjectMutated("Change Time Signature"));
                }
                ui.label("/");
                egui::ComboBox::from_id_salt("base_meter_den")
                    .selected_text(format!("{}", den))
                    .width(42.0)
                    .show_ui(ui, |ui| {
                        for d in [4u8, 8, 16, 2] {
                            if ui.selectable_value(&mut den, d, format!("{}", d)).changed() {
                                project.time_signature.denominator = den;
                                actions.push(ArrangementAction::ProjectMutated(
                                    "Change Time Signature",
                                ));
                            }
                        }
                    });
        if !project.tempo_map.is_empty() && ui.chrome_small_button("Reset").clicked() {
                    project.tempo_map.clear();
                    project.meter_map.clear();
                    actions.push(ArrangementAction::ProjectMutated("Reset Conductor Map"));
                }
            });
        });

        let tl_rect = Rect::from_min_size(
            Pos2::new(row_rect.left() + timeline_x0_rel, row_rect.top()),
            Vec2::new(timeline_w, cond_h - 1.0),
        );
        ui.painter()
            .rect_filled(tl_rect, 0, Color32::from_rgb(16, 15, 20));

        let min_bpm = 50.0_f64;
        let max_bpm = 220.0_f64;
        let bpm_to_y = |bpm: f64| -> f32 {
            let norm = ((bpm - min_bpm) / (max_bpm - min_bpm)).clamp(0.0, 1.0) as f32;
            tl_rect.bottom() - 6.0 - norm * (tl_rect.height() - 14.0)
        };
        let y_to_bpm = |y: f32| -> f64 {
            let norm = ((tl_rect.bottom() - 6.0 - y) / (tl_rect.height() - 14.0)).clamp(0.0, 1.0);
            min_bpm + (norm as f64) * (max_bpm - min_bpm)
        };

        // Sample & draw Tempo Curve across timeline
        let samples = (timeline_w as usize / 4).clamp(24, 480);
        let mut curve = Vec::with_capacity(samples + 1);
        for s in 0..=samples {
            let frac = s as f32 / samples as f32;
            let b = self.scroll_beat + (frac * timeline_w / self.pixels_per_beat) as f64;
            let bpm = project.bpm_at_beat(b);
            curve.push(Pos2::new(tl_rect.left() + frac * timeline_w, bpm_to_y(bpm)));
        }
        ui.painter()
            .add(egui::Shape::line(curve, Stroke::new(1.8_f32, ACCENT_AMBER)));

        // Interactive Tempo Point Handles (Drag to change Beat/BPM, Click+Shift or Right-Click to toggle Ramp/Step or Delete!)
        let mut del_tp = None;
        let mut hit_tp = false;
        for idx in 0..project.tempo_map.len() {
            let tp = project.tempo_map[idx];
            let px = tl_rect.left() + ((tp.beat - self.scroll_beat) as f32 * self.pixels_per_beat);
            if px < tl_rect.left() - 6.0 || px > tl_rect.right() + 6.0 {
                continue;
            }
            let py = bpm_to_y(tp.bpm);
            let hrect = Rect::from_center_size(Pos2::new(px, py), Vec2::new(12.0, 12.0));
            let resp = ui
                .interact(
                    hrect,
                    ui.id().with(("tempo_pt", tp.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "{:.1} BPM @ Beat {:.1} ({}) — Drag to adjust, Right-click for Ramp/Step or Delete",
                    tp.bpm,
                    tp.beat,
                    if tp.ramp_to_next { "Linear Ramp" } else { "Step" }
                ));

            ui.painter().circle_filled(Pos2::new(px, py), 4.5, ACCENT_AMBER);
            ui.painter().text(
                Pos2::new(px + 6.0, (py - 8.0).max(tl_rect.top() + 6.0)),
                Align2::LEFT_CENTER,
                format!("{:.0}", tp.bpm),
                FontId::monospace(9.0),
                ACCENT_AMBER,
            );

            if resp.dragged() {
                hit_tp = true;
                let d = resp.drag_delta();
                project.tempo_map[idx].beat =
                    (project.tempo_map[idx].beat + (d.x / self.pixels_per_beat) as f64).max(0.0);
                project.tempo_map[idx].bpm = y_to_bpm(py + d.y).clamp(30.0, 260.0);
                project.sort_conductor_maps();
                actions.push(ArrangementAction::ProjectMutated("Drag Tempo Point"));
            } else if resp.clicked() {
                hit_tp = true;
            }

            resp.context_menu(|ui| {
                if ui
                    .checkbox(
                        &mut project.tempo_map[idx].ramp_to_next,
                        "Linear Tempo Ramp to Next Point",
                    )
                    .changed()
                {
                    actions.push(ArrangementAction::ProjectMutated("Toggle Tempo Ramp"));
                }
                if project.tempo_map.len() > 1
                    && ui
                        .chrome_danger_button(egui::RichText::new("🗑 Delete Tempo Point").color(ACCENT_RED))
                        .clicked()
                {
                    del_tp = Some(idx);
                    ui.close_menu();
                }
            });
        }
        if let Some(di) = del_tp {
            project.tempo_map.remove(di);
            project.sort_conductor_maps();
            actions.push(ArrangementAction::ProjectMutated("Delete Tempo Point"));
        }

        // Render Mid-Song Time-Signature Badges (`3/4`, `7/8`, etc.)
        let mut del_mc = None;
        for (mi, mc) in project.meter_map.iter().enumerate() {
            let mx =
                tl_rect.left() + ((mc.beat as f64 - self.scroll_beat) as f32 * self.pixels_per_beat);
            if mx >= tl_rect.left() && mx <= tl_rect.right() - 28.0 {
                let brect = Rect::from_min_size(
                    Pos2::new(mx + 2.0, tl_rect.top() + 2.0),
                    Vec2::new(34.0, 14.0),
                );
                ui.painter()
                    .rect_filled(brect, 3, ACCENT_TEAL.gamma_multiply(0.30));
                ui.painter().text(
                    brect.center(),
                    Align2::CENTER_CENTER,
                    format!("{}/{}", mc.signature.numerator, mc.signature.denominator),
                    FontId::monospace(9.5),
                    ACCENT_TEAL,
                );
                let mr = ui.interact(brect, ui.id().with(("meter_chg", mc.id)), Sense::click());
                if mr.secondary_clicked() {
                    del_mc = Some(mi);
                }
            }
        }
        if let Some(dmi) = del_mc {
            project.meter_map.remove(dmi);
            actions.push(ArrangementAction::ProjectMutated("Delete Meter Change"));
        }

        // Click empty Conductor Lane -> Insert new Tempo Point!
        let bg_r = ui.interact(tl_rect, ui.id().with("cond_lane_bg"), Sense::click());
        if bg_r.clicked() && !hit_tp {
            if let Some(pos) = bg_r.interact_pointer_pos() {
                let raw_b =
                    self.scroll_beat + ((pos.x - tl_rect.left()) / self.pixels_per_beat) as f64;
                let snapped_b = project.snap_grid.snap(raw_b, project.time_signature);
                let new_bpm = (y_to_bpm(pos.y) * 2.0).round() * 0.5;
                project.upsert_tempo_point(snapped_b, new_bpm, true);
                actions.push(ArrangementAction::ProjectMutated("Insert Tempo Point"));
            }
        }

        row_rect.bottom()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_track_automation_lanes(
        &mut self,
        ui: &mut Ui,
        origin: Pos2,
        width: f32,
        lane_h: f32,
        project: &mut Project,
        t_idx: usize,
        actions: &mut Vec<ArrangementAction>,
    ) {
        let auto_col = ACCENT_PURPLE;
        let fx_meta: Vec<(uuid::Uuid, String, Vec<String>)> = project.tracks[t_idx]
            .fx_chain
            .iter()
            .map(|f| {
                let b = f.effective_badge().to_string();
                let names = (0..f.param_count())
                    .map(|pi| f.param_name(pi).to_string())
                    .collect();
                (f.id, b, names)
            })
            .collect();

        let mut y_cursor = origin.y;
        for l_idx in 0..project.tracks[t_idx].automation_lanes.len() {
            if !project.tracks[t_idx].automation_lanes[l_idx].visible {
                continue;
            }
            let lane_rect = Rect::from_min_size(Pos2::new(origin.x, y_cursor), Vec2::new(width, lane_h - 1.0));
            y_cursor += lane_h;

            ui.painter()
                .rect_filled(lane_rect, 0, Color32::from_rgb(15, 16, 23));
            ui.painter().line_segment(
                [lane_rect.left_bottom(), lane_rect.right_bottom()],
                Stroke::new(1.0_f32, BORDER_SUBTLE),
            );

            let lane = &mut project.tracks[t_idx].automation_lanes[l_idx];
            let target_lbl = match lane.target {
                ferric_model::AutomationTarget::Volume => "Auto: Volume".to_string(),
                ferric_model::AutomationTarget::Pan => "Auto: Pan".to_string(),
                ferric_model::AutomationTarget::StereoWidth => "Auto: Width".to_string(),
                ferric_model::AutomationTarget::FxWet { .. } => "Auto: FX Wet".to_string(),
                ferric_model::AutomationTarget::FxParam { fx_id, param_index } => fx_meta
                    .iter()
                    .find(|(id, _, _)| *id == fx_id)
                    .and_then(|(_, b, names)| {
                        names
                            .get(param_index)
                            .map(|nm| format!("Auto: {} → {}", b, nm))
                    })
                    .unwrap_or_else(|| "Auto: FX Param".into()),
            };

            // Sample & draw smooth curved Bezier automation envelope across visible timeline
            let samples = (width as usize / 4).clamp(24, 480);
            let mut poly = Vec::with_capacity(samples + 1);
            for s in 0..=samples {
                let frac = s as f32 / samples as f32;
                let beat = self.scroll_beat + (frac * width / self.pixels_per_beat) as f64;
                let norm = lane.eval_at(beat).unwrap_or(0.5);
                let x = lane_rect.left() + frac * width;
                let y = lane_rect.bottom() - 4.0 - norm * (lane_rect.height() - 8.0);
                poly.push(Pos2::new(x, y));
            }
            ui.painter()
                .add(egui::Shape::line(poly, Stroke::new(1.8_f32, auto_col)));

            ui.painter().text(
                Pos2::new(lane_rect.left() + 6.0, lane_rect.top() + 3.0),
                Align2::LEFT_TOP,
                &target_lbl,
                FontId::proportional(9.5),
                auto_col,
            );

            // Interactive Breakpoint Handles (Drag = Move, Shift+Drag = Bezier Curve Tension, Right-Click = Delete)
            let mut del_pt = None;
            let mut interacted_pt = false;
            for p_idx in 0..lane.points.len() {
                let pt = lane.points[p_idx];
                let px = lane_rect.left() + ((pt.beat - self.scroll_beat) as f32 * self.pixels_per_beat);
                if px < lane_rect.left() - 6.0 || px > lane_rect.right() + 6.0 {
                    continue;
                }
                let py = lane_rect.bottom() - 4.0 - pt.normalized_value * (lane_rect.height() - 8.0);
                let hit = Rect::from_center_size(Pos2::new(px, py), Vec2::new(11.0, 11.0));
                let p_resp = ui.interact(
                    hit,
                    ui.id().with(("auto_pt", pt.id)),
                    Sense::click_and_drag(),
                );
                ui.painter().circle_filled(Pos2::new(px, py), 4.2, auto_col);
                ui.painter().circle_stroke(
                    Pos2::new(px, py),
                    4.2,
                    Stroke::new(1.0_f32, Color32::WHITE),
                );

                if p_resp.secondary_clicked() && lane.points.len() > 1 {
                    del_pt = Some(p_idx);
                    interacted_pt = true;
                } else if p_resp.dragged() {
                    interacted_pt = true;
                    let d = p_resp.drag_delta();
                    if ui.input(|i| i.modifiers.shift) {
                        lane.points[p_idx].tension =
                            (lane.points[p_idx].tension + d.y * 0.015).clamp(-0.92, 0.92);
                    } else {
                        lane.points[p_idx].beat =
                            (lane.points[p_idx].beat + (d.x / self.pixels_per_beat) as f64).max(0.0);
                        lane.points[p_idx].normalized_value = (lane.points[p_idx].normalized_value
                            - d.y / (lane_rect.height() - 8.0))
                            .clamp(0.0, 1.0);
                    }
                    lane.sort_points();
                    actions.push(ArrangementAction::ProjectMutated("Edit Automation Curve"));
                } else if p_resp.clicked() {
                    interacted_pt = true;
                }
            }
            if let Some(di) = del_pt {
                lane.points.remove(di);
                actions.push(ArrangementAction::ProjectMutated("Delete Automation Point"));
            }

            // Click empty spot on lane -> insert new automation point!
            let bg_r = ui.interact(
                lane_rect,
                ui.id().with(("auto_lane_bg", lane.id)),
                Sense::click(),
            );
            if bg_r.clicked() && !interacted_pt {
                if let Some(pos) = bg_r.interact_pointer_pos() {
                    let b = self.scroll_beat + ((pos.x - lane_rect.left()) / self.pixels_per_beat) as f64;
                    let norm = ((lane_rect.bottom() - 4.0 - pos.y) / (lane_rect.height() - 8.0))
                        .clamp(0.0, 1.0);
                    lane.upsert_point(b, norm);
                    actions.push(ArrangementAction::ProjectMutated("Add Automation Point"));
                }
            }
        }
    }
}

fn draw_clip_visual(
    painter: &egui::Painter,
    rect: Rect,
    clip: &TimelineClip,
    track_col: Color32,
    selected: bool,
) {
    let base_col = clip
        .color_override
        .map(track_color32)
        .unwrap_or(track_col);
    painter.rect_filled(rect, 5, base_col.gamma_multiply(0.20));

    let hdr_h = 16.0_f32.min(rect.height());
    let hdr_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), hdr_h));
    painter.rect_filled(hdr_rect, 5, base_col.gamma_multiply(0.55));

    let take_badge = if clip.takes.len() > 1 {
        if !clip.comp_segments.is_empty() {
            format!(" [{} Takes • COMP]", clip.takes.len())
        } else {
            format!(" [T{}/{}]", clip.active_take + 1, clip.takes.len())
        }
    } else {
        String::new()
    };
    let fx_badge = if !clip.clip_fx.is_empty() {
        format!(" [FX:{}]", clip.clip_fx.len())
    } else {
        String::new()
    };

    painter.text(
        Pos2::new(hdr_rect.left() + 6.0, hdr_rect.center().y),
        Align2::LEFT_CENTER,
        format!("{}{}{}", clip.name, take_badge, fx_badge),
        FontId::proportional(10.5),
        Color32::WHITE,
    );

    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 2.0, hdr_rect.bottom() + 2.0),
        Pos2::new(rect.right() - 2.0, rect.bottom() - 2.0),
    );

    if body.height() > 8.0 && body.width() > 6.0 && !clip.takes.is_empty() {
        let cols = (body.width() as usize).clamp(8, 600);
        let first_take = &clip.takes[clip.active_take.min(clip.takes.len() - 1)];
        match &first_take.content {
            TakeContent::Audio { .. } => {
                let mid_y = body.center().y;
                let half_h = body.height() * 0.46;
                let gain_scale = 10.0_f32.powf(clip.gain_db / 20.0);

                for px in 0..cols {
                    let frac = px as f64 / cols as f64;
                    let local_beat = frac * clip.length_beats;
                    let take_idx = clip.take_at_local_beat(local_beat);
                    if let Some(TakeContent::Audio { buffer }) =
                        clip.takes.get(take_idx).map(|t| &t.content)
                    {
                        if !buffer.peaks.is_empty() {
                            let pk_idx = ((frac * buffer.peaks.len() as f64) as usize)
                                .min(buffer.peaks.len() - 1);
                            let (mn, mx) = buffer.peaks[pk_idx];
                            let env = clip.gain_envelope_at(local_beat) / gain_scale.max(0.01);
                            let y0 = (mid_y - mx * gain_scale * env * half_h)
                                .clamp(body.top(), body.bottom());
                            let y1 = (mid_y - mn * gain_scale * env * half_h)
                                .clamp(body.top(), body.bottom());
                            let x = body.left() + px as f32;
                            painter.line_segment(
                                [Pos2::new(x, y0), Pos2::new(x, y1)],
                                Stroke::new(1.1_f32, base_col.gamma_multiply(0.95)),
                            );
                        }
                    }
                }
            }
            TakeContent::Midi { notes, .. } => {
                if !notes.is_empty() {
                    let min_p = notes.iter().map(|n| n.pitch).min().unwrap_or(48) as f32 - 2.0;
                    let max_p = notes.iter().map(|n| n.pitch).max().unwrap_or(72) as f32 + 2.0;
                    let span_p = (max_p - min_p).max(12.0);
                    let loop_len = if clip.loop_enabled {
                        clip.loop_length_beats.max(0.5)
                    } else {
                        clip.length_beats.max(0.5)
                    };
                    let reps = (clip.length_beats / loop_len).ceil() as usize;
                    for rep in 0..reps.min(16) {
                        let base_b = rep as f64 * loop_len;
                        for n in notes {
                            let s = base_b + n.start_beat;
                            if s >= clip.length_beats {
                                continue;
                            }
                            let e = (base_b + n.end_beat()).min(clip.length_beats);
                            let nx0 =
                                body.left() + (s / clip.length_beats) as f32 * body.width();
                            let nx1 =
                                body.left() + (e / clip.length_beats) as f32 * body.width();
                            let ny = body.bottom()
                                - ((n.pitch as f32 - min_p) / span_p).clamp(0.05, 0.95)
                                    * body.height();
                            painter.line_segment(
                                [Pos2::new(nx0, ny), Pos2::new(nx1.max(nx0 + 3.0), ny)],
                                Stroke::new(3.2_f32, Color32::WHITE.gamma_multiply(0.88)),
                            );
                        }
                    }
                }
            }
        }
    }

    let stroke_col = if selected { Color32::WHITE } else { base_col };
    painter.rect_stroke(
        rect,
        5,
        Stroke::new(if selected { 1.8_f32 } else { 1.0_f32 }, stroke_col),
        StrokeKind::Inside,
    );
}
