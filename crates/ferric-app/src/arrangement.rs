use crate::theme::{
    pill_toggle, track_color32, ACCENT_AMBER, ACCENT_FERRIC, ACCENT_RED, ACCENT_TEAL, BG_DEEP,
    BG_ELEVATED, BG_PANEL, BG_TRACK_EVEN, BG_TRACK_ODD, BORDER_SUBTLE, TEXT_DIM, TEXT_MAIN,
};
use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use ferric_engine::EngineTelemetry;
use ferric_model::{
    ClipId, MidiNote, Project, SessionSlot, SnapGrid, Take, TakeContent, TimelineClip, TrackId,
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
            EditTool::SplitRazor => "Razor Split",
            EditTool::DrawPencil => "Draw Clip",
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
    ImportAudioToTrack(TrackId),
    ToggleFreezeTrack(TrackId),
    DeleteTrack(TrackId),
    DuplicateTrack(TrackId),
    MoveTrack {
        track_id: TrackId,
        delta: isize,
    },
    AddTrack,
}

pub struct ArrangementState {
    pub pixels_per_beat: f32,
    pub scroll_beat: f64,
    pub tool: EditTool,
    pub ripple_edit: bool,
    pub show_session_launcher: bool,
    pub renaming_track: Option<TrackId>,
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
            renaming_track: None,
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
        // Sub-toolbar above timeline
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if pill_toggle(
                ui,
                "⊞ Session Clips",
                self.show_session_launcher,
                ACCENT_TEAL,
            )
            .on_hover_text("Toggle Ableton/Bitwig Hybrid Session Clip Launcher")
            .clicked()
            {
                self.show_session_launcher = !self.show_session_launcher;
            }
            ui.separator();

            for t in [EditTool::SmartPointer, EditTool::SplitRazor, EditTool::DrawPencil] {
                if pill_toggle(ui, t.label(), self.tool == t, ACCENT_FERRIC).clicked() {
                    self.tool = t;
                }
            }
            ui.separator();

            ui.label(egui::RichText::new("Snap:").size(11.0).color(TEXT_DIM));
            egui::ComboBox::from_id_salt("snap_grid_combo")
                .selected_text(project.snap_grid.label())
                .width(96.0)
                .show_ui(ui, |ui| {
                    for &g in SnapGrid::ALL {
                        ui.selectable_value(&mut project.snap_grid, g, g.label());
                    }
                });

            if pill_toggle(ui, "⛓ Ripple", self.ripple_edit, ACCENT_AMBER)
                .on_hover_text("Ripple Edit Mode: Deleting clips shifts later clips on the track left automatically!")
                .clicked()
            {
                self.ripple_edit = !self.ripple_edit;
            }

            ui.separator();
            ui.spacing_mut().slider_width = 76.0;
            ui.label(egui::RichText::new("Zoom:").size(11.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.pixels_per_beat, 14.0..=140.0).show_value(false));

            let max_scroll = (project.project_end_beat() + 32.0).max(64.0);
            ui.label(egui::RichText::new("Pos:").size(11.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.scroll_beat, 0.0..=max_scroll).show_value(false))
                .on_hover_text(
                    "Horizontal Song Scrollbar (or Shift + MouseWheel / Middle-Mouse Drag)",
                );

            if ui.button("+ Track").clicked() {
                actions.push(ArrangementAction::AddTrack);
            }
            if project.tracks.iter().any(|t| t.active_session_slot.is_some())
                && ui
                    .button(egui::RichText::new("⏹ Stop Session Clips").color(ACCENT_AMBER))
                    .clicked()
            {
                project.stop_all_session_clips();
                actions.push(ArrangementAction::ProjectMutated("Stop All Session Clips"));
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

        // Ctrl + MouseWheel horizontal zoom, Shift + MouseWheel or Middle-Mouse drag horizontal pan!
        if ui.rect_contains_pointer(ui.max_rect()) {
            let (ctrl, shift, scroll, mid_down, ptr_delta) = ui.input(|i| {
                (
                    i.modifiers.ctrl,
                    i.modifiers.shift,
                    i.smooth_scroll_delta,
                    i.pointer.middle_down(),
                    i.pointer.delta(),
                )
            });
            if ctrl && scroll.y.abs() > 0.1 {
                self.pixels_per_beat =
                    (self.pixels_per_beat * (1.0 + scroll.y * 0.003)).clamp(14.0, 160.0);
            } else if shift && (scroll.y.abs() > 0.1 || scroll.x.abs() > 0.1) {
                let delta_px = if scroll.x.abs() > 0.1 { scroll.x } else { scroll.y };
                self.scroll_beat =
                    (self.scroll_beat - (delta_px / self.pixels_per_beat) as f64).max(0.0);
            } else if scroll.x.abs() > 0.1 {
                self.scroll_beat =
                    (self.scroll_beat - (scroll.x / self.pixels_per_beat) as f64).max(0.0);
            }
            if mid_down && ptr_delta.x.abs() > 0.1 {
                self.scroll_beat =
                    (self.scroll_beat - (ptr_delta.x / self.pixels_per_beat) as f64).max(0.0);
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
                        .button("➔ Print Scene to Arrangement at Playhead")
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
            let ph = project
                .snap_grid
                .snap(telem.playhead_beat, project.time_signature);
            if ui
                .button(format!("🚩 Add Song Marker at Beat {:.1}", ph))
                .clicked()
            {
                let n = project.markers.len() + 1;
                project.add_marker(format!("Marker {}", n), ph);
                actions.push(ArrangementAction::ProjectMutated("Add Song Marker"));
                ui.close_menu();
            }
            if ui.button("⟦ Set Loop Start to Playhead").clicked() {
                project.loop_start_beat = ph.min(project.loop_end_beat - 1.0).max(0.0);
                actions.push(ArrangementAction::ProjectMutated("Set Loop Start"));
                ui.close_menu();
            }
            if ui.button("⟧ Set Loop End to Playhead").clicked() {
                project.loop_end_beat = ph.max(project.loop_start_beat + 1.0);
                actions.push(ArrangementAction::ProjectMutated("Set Loop End"));
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
                        .button(egui::RichText::new("🗑 Delete Marker").color(ACCENT_RED))
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

        // Render Each Visible Track Row + Optional Multi-Take Comping Lanes!
        let prev_row_bounds = self.track_row_bounds.clone();
        self.track_row_bounds.clear();
        let mut playhead_top = ruler_tl_rect.top();
        let mut playhead_bot = ruler_tl_rect.bottom();
        playhead_top = playhead_top.min(ruler_tl_rect.top());

        for (vis_pos, &t_idx) in visible_indices.iter().enumerate() {
            let track_id = project.tracks[t_idx].id;
            let is_folder = project.is_folder_track(track_id);
            let depth = project.track_depth(track_id);
            let base_h = project.tracks[t_idx].height_px.max(72.0);
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

        // Release active clip drag on mouse up
        if self.drag_clip.is_some() && !ui.input(|i| i.pointer.any_down()) {
            self.drag_clip = None;
        }

        // Draw crisp Playhead Line across entire ruler + all tracks!
        let ph_x = ruler_tl_rect.left()
            + ((telem.playhead_beat - self.scroll_beat) as f32 * self.pixels_per_beat);
        if ph_x >= ruler_tl_rect.left() && ph_x <= ruler_tl_rect.right() {
            let ph_col = if telem.is_recording {
                ACCENT_RED
            } else {
                ACCENT_FERRIC
            };
            ui.painter().line_segment(
                [Pos2::new(ph_x, playhead_top), Pos2::new(ph_x, playhead_bot)],
                Stroke::new(1.8_f32, ph_col),
            );
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(ph_x - 5.5, playhead_top),
                    Pos2::new(ph_x + 5.5, playhead_top),
                    Pos2::new(ph_x, playhead_top + 8.0),
                ],
                ph_col,
                Stroke::NONE,
            ));
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

        // Right-Click Track Header Context Menu: Rename, Color Swatches, Move Up/Down, Duplicate, Delete!
        bg_resp.context_menu(|ui| {
            ui.label(egui::RichText::new("Track Name:").size(10.5).color(TEXT_DIM));
            if ui
                .text_edit_singleline(&mut project.tracks[t_idx].name)
                .changed()
            {
                actions.push(ArrangementAction::ProjectMutated("Rename Track"));
            }
            ui.separator();
            ui.label(egui::RichText::new("Studio Color:").size(10.5).color(TEXT_DIM));
            ui.horizontal(|ui| {
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
            if ui.button("▲ Move Track Up").clicked() {
                actions.push(ArrangementAction::MoveTrack {
                    track_id,
                    delta: -1,
                });
                ui.close_menu();
            }
            if ui.button("▼ Move Track Down").clicked() {
                actions.push(ArrangementAction::MoveTrack {
                    track_id,
                    delta: 1,
                });
                ui.close_menu();
            }
            if ui.button("⧉ Duplicate Track").clicked() {
                actions.push(ArrangementAction::DuplicateTrack(track_id));
                ui.close_menu();
            }
            if project.tracks.len() > 1 {
                ui.separator();
                if ui
                    .button(egui::RichText::new("🗑 Delete Track").color(ACCENT_RED))
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
                        .small_button(icon)
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
                if pill_toggle(ui, "S", trk.solo, Color32::from_rgb(92, 200, 242))
                    .on_hover_text("Solo Track")
                    .clicked()
                {
                    trk.solo = !trk.solo;
                    actions.push(ArrangementAction::ProjectMutated("Toggle Solo"));
                }
                if pill_toggle(ui, "R", trk.record_arm, ACCENT_RED)
                    .on_hover_text("Record Arm (Audio + MIDI)")
                    .clicked()
                {
                    trk.record_arm = !trk.record_arm;
                    actions.push(ArrangementAction::ProjectMutated("Toggle Record Arm"));
                }
                let fx_txt = format!("FX {}", trk.fx_chain.len());
                if pill_toggle(ui, &fx_txt, !trk.fx_chain.is_empty(), ACCENT_TEAL)
                    .on_hover_text("Select Track to inspect FX & Modulators")
                    .clicked()
                {
                    actions.push(ArrangementAction::SelectTrack(track_id));
                }
                if pill_toggle(ui, "A", trk.show_automation, Color32::from_rgb(186, 108, 226))
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
                if pill_toggle(ui, "❄", trk.frozen, Color32::from_rgb(100, 210, 255))
                    .on_hover_text("1-Click Reversible Track Freeze (Renders 32-bit Stem & frees 100% DSP CPU!)")
                    .clicked()
                {
                    actions.push(ArrangementAction::ToggleFreezeTrack(track_id));
                }
                if ui
                    .small_button("📂")
                    .on_hover_text("Import WAV/FLAC/MP3/OGG Audio Clip onto this Track")
                    .clicked()
                {
                    actions.push(ArrangementAction::ImportAudioToTrack(track_id));
                }
            });

            // Row 3: Compact Volume dB drag + Pan + Live horizontal peak bar
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
        let bg_resp = ui.interact(
            lane_rect,
            ui.id().with(("lane_bg", track_id)),
            Sense::click(),
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
        } else if bg_resp.clicked() {
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
            let is_sel = selected_clip == Some((track_id, clip_id));

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

            // Interactions: Select, Double-Click Piano Roll, Split Razor, Drag Move/Resize!
            if clip_resp.clicked() {
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
                    let clip_mut = &mut project.tracks[t_idx].clips[c_idx];
                    match kind {
                        DragKind::Move => {
                            clip_mut.start_beat = (clip_mut.start_beat + d_beats).max(0.0);

                            // Vertical Track-to-Track Clip Drag!
                            if let Some(ptr) = ui.input(|i| i.pointer.interact_pos()) {
                                if let Some(&(hover_tid, _, _)) = row_bounds
                                    .iter()
                                    .find(|&&(_, y0, y1)| ptr.y >= y0 && ptr.y <= y1)
                                {
                                    if hover_tid != track_id {
                                        self.drag_clip = Some((hover_tid, clip_id, kind, init_v));
                                        actions.push(ArrangementAction::MoveClipToTrack {
                                            from_track: track_id,
                                            to_track: hover_tid,
                                            clip_id,
                                        });
                                    }
                                }
                            }
                        }
                        DragKind::ResizeLeft => {
                            let max_trim = (clip_mut.length_beats - 0.25).max(0.0);
                            let trim = d_beats.clamp(-clip_mut.start_beat, max_trim);
                            clip_mut.start_beat += trim;
                            clip_mut.length_beats = (clip_mut.length_beats - trim).max(0.25);
                            clip_mut.slip_offset_beats =
                                (clip_mut.slip_offset_beats + trim * clip_mut.stretch_rate as f64)
                                    .max(0.0);
                        }
                        DragKind::ResizeRight => {
                            clip_mut.length_beats = (clip_mut.length_beats + d_beats).max(0.25);
                        }
                    }
                    actions.push(ArrangementAction::ProjectMutated("Move/Trim Clip"));
                }
            }

            // Right-Click REAPER Media Item Context Menu
            let playhead = telem.playhead_beat;
            clip_resp.context_menu(|ui| {
                ui.label(
                    egui::RichText::new(&project.tracks[t_idx].clips[c_idx].name)
                        .strong()
                        .color(t_col),
                );
                ui.separator();
                if ui.button("✂ Split Clip at Playhead (S)").clicked() {
                    actions.push(ArrangementAction::SplitClip {
                        track_id,
                        clip_id,
                        beat: playhead,
                    });
                    ui.close_menu();
                }
                if ui.button("⧉ Duplicate Clip (Ctrl+D)").clicked() {
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
                    if ui.button(txt).clicked() {
                        project.tracks[t_idx].clips[c_idx].show_take_lanes = !showing;
                        ui.close_menu();
                    }
                }
                if ui.button("➕ Duplicate Active Take as New Take Variation").clicked() {
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
                    if ui.button("🔊 Normalize Audio Take (-1.0 dB)").clicked() {
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
                    if ui.button("⏪ Reverse Audio Take").clicked() {
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
                } else if ui.button("🎹 Quantize MIDI Notes (1/16)").clicked() {
                    project.quantize_midi_clip(track_id, clip_id, 0.25);
                    actions.push(ArrangementAction::ProjectMutated("Quantize MIDI 1/16"));
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .button(egui::RichText::new("🗑 Delete Clip").color(ACCENT_RED))
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
        let auto_col = Color32::from_rgb(198, 126, 246);
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
