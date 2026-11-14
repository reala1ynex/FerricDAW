use crate::piano_roll::DockAction;
use crate::theme::{
    pill_toggle, stereo_level_meter, studio_knob, track_color32, vertical_fader, ACCENT_AMBER,
    ACCENT_FERRIC, ACCENT_RED, ACCENT_TEAL, BG_DEEP, BG_ELEVATED, BORDER_SUBTLE, TEXT_DIM,
    TEXT_MAIN,
};
use egui::{Color32, Stroke, Ui};
use ferric_engine::EngineTelemetry;
use ferric_model::{InputSource, Project, SendTap, TrackId};

pub fn render_mixer_console(
    ui: &mut Ui,
    project: &mut Project,
    selected_track: &mut Option<TrackId>,
    inspect_master: &mut bool,
    telem: &EngineTelemetry,
    actions: &mut Vec<DockAction>,
) {
    egui::ScrollArea::horizontal()
        .id_salt("mixer_console_hscroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let all_track_targets: Vec<(TrackId, String)> = project
                    .tracks
                    .iter()
                    .map(|t| (t.id, t.name.clone()))
                    .collect();

                for idx in 0..project.tracks.len() {
                    let tid = project.tracks[idx].id;
                    let t_col = track_color32(project.tracks[idx].color);
                    let is_sel = *selected_track == Some(tid);
                    let meter = telem.track_meters.get(idx).copied().unwrap_or_default();

                    egui::Frame::new()
                        .fill(if is_sel {
                            Color32::from_rgb(33, 37, 48)
                        } else {
                            BG_ELEVATED
                        })
                        .stroke(Stroke::new(if is_sel { 1.5_f32 } else { 1.0_f32 }, if is_sel { t_col } else { BORDER_SUBTLE }))
                        .corner_radius(5)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.set_width(88.0);
                            ui.vertical_centered(|ui| {
                                // Color bar + Track Title
                                let (bar_r, _) =
                                    ui.allocate_exact_size(egui::Vec2::new(82.0, 4.0), egui::Sense::hover());
                                ui.painter().rect_filled(bar_r, 2, t_col);

                                if ui
                                    .add(
                                        egui::Label::new(
                                            egui::RichText::new(&project.tracks[idx].name)
                                                .size(11.0)
                                                .strong(),
                                        )
                                        .truncate(),
                                    )
                                    .clicked()
                                {
                                    *selected_track = Some(tid);
                                    *inspect_master = false;
                                }

                                // Input Source selector
                                egui::ComboBox::from_id_salt(("mix_in", tid))
                                    .selected_text(project.tracks[idx].input_source.label())
                                    .width(82.0)
                                    .show_ui(ui, |ui| {
                                        for src in [
                                            InputSource::AudioAndMidi { stereo_pair: 0 },
                                            InputSource::AudioMono { channel: 0 },
                                            InputSource::AudioMono { channel: 1 },
                                            InputSource::MidiAll,
                                            InputSource::ResampleTrackOutput,
                                        ] {
                                            if ui
                                                .selectable_value(
                                                    &mut project.tracks[idx].input_source,
                                                    src,
                                                    src.label(),
                                                )
                                                .changed()
                                            {
                                                actions.push(DockAction::ProjectMutated(
                                                    "Change Track Input",
                                                ));
                                            }
                                        }
                                    });

                                // M / S / R / Ø Phase Invert row
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    let trk = &mut project.tracks[idx];
                                    if pill_toggle(ui, "M", trk.mute, ACCENT_AMBER).clicked() {
                                        trk.mute = !trk.mute;
                                        actions.push(DockAction::ProjectMutated("Mixer Mute"));
                                    }
                                    if pill_toggle(ui, "S", trk.solo, Color32::from_rgb(92, 200, 242)).clicked() {
                                        trk.solo = !trk.solo;
                                        actions.push(DockAction::ProjectMutated("Mixer Solo"));
                                    }
                                    if pill_toggle(ui, "R", trk.record_arm, ACCENT_RED).clicked() {
                                        trk.record_arm = !trk.record_arm;
                                        actions.push(DockAction::ProjectMutated("Mixer Arm"));
                                    }
                                    if pill_toggle(ui, "Ø", trk.phase_invert, ACCENT_TEAL)
                                        .on_hover_text("Invert Polarity (180°)")
                                        .clicked()
                                    {
                                        trk.phase_invert = !trk.phase_invert;
                                        actions.push(DockAction::ProjectMutated("Invert Phase"));
                                    }
                                });

                                // Pan + Stereo Width compact row
                                if studio_knob(
                                    ui,
                                    "Pan",
                                    &mut project.tracks[idx].pan,
                                    -1.0,
                                    1.0,
                                    0.0,
                                    "",
                                    26.0,
                                    t_col,
                                ) {
                                    actions.push(DockAction::ProjectMutated("Adjust Pan"));
                                }

                                // Fader + Dual Stereo Meter + Compressor Gain Reduction indicator
                                ui.horizontal(|ui| {
                                    ui.add_space(10.0);
                                    if vertical_fader(
                                        ui,
                                        &mut project.tracks[idx].gain_db,
                                        86.0,
                                        t_col,
                                    )
                                    .changed()
                                    {
                                        actions.push(DockAction::ProjectMutated("Move Fader"));
                                    }
                                    stereo_level_meter(
                                        ui,
                                        meter.peak_l_db,
                                        meter.peak_r_db,
                                        86.0,
                                    );
                                    if meter.gain_reduction_db > 0.2 {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "-{:.0}",
                                                meter.gain_reduction_db
                                            ))
                                            .size(9.0)
                                            .color(ACCENT_AMBER),
                                        );
                                    }
                                });

                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:+.1} dB",
                                        project.tracks[idx].gain_db
                                    ))
                                    .size(10.0)
                                    .monospace()
                                    .color(TEXT_DIM),
                                );

                                // Quick Aux Send Adder
                                ui.menu_button(
                                    format!("Sends ({})", project.tracks[idx].sends.len()),
                                    |ui| {
                                        let mut rm_s = None;
                                        for (si, s) in project.tracks[idx].sends.iter_mut().enumerate()
                                        {
                                            let dst_name = all_track_targets
                                                .iter()
                                                .find(|(id, _)| *id == s.target_track_id)
                                                .map(|(_, n)| n.as_str())
                                                .unwrap_or("Bus");
                                            ui.horizontal(|ui| {
                                                ui.label(dst_name);
                                                if ui
                                                    .add(
                                                        egui::DragValue::new(&mut s.gain_db)
                                                            .range(-60.0..=6.0)
                                                            .speed(0.2)
                                                            .suffix(" dB"),
                                                    )
                                                    .changed()
                                                {
                                                    actions.push(DockAction::ProjectMutated(
                                                        "Adjust Send Level",
                                                    ));
                                                }
                                                egui::ComboBox::from_id_salt(("send_tap", s.id))
                                                    .selected_text(s.tap.label())
                                                    .width(72.0)
                                                    .show_ui(ui, |ui| {
                                                        for tap in [
                                                            SendTap::PostFader,
                                                            SendTap::PreFaderPostFx,
                                                            SendTap::PreFx,
                                                        ] {
                                                            if ui
                                                                .selectable_value(
                                                                    &mut s.tap,
                                                                    tap,
                                                                    tap.label(),
                                                                )
                                                                .changed()
                                                            {
                                                                actions.push(
                                                                    DockAction::ProjectMutated(
                                                                        "Change Send Tap",
                                                                    ),
                                                                );
                                                            }
                                                        }
                                                    });
                                                if ui.small_button("×").clicked() {
                                                    rm_s = Some(si);
                                                }
                                            });
                                        }
                                        if let Some(ri) = rm_s {
                                            project.tracks[idx].sends.remove(ri);
                                            actions.push(DockAction::ProjectMutated("Remove Send"));
                                        }
                                        ui.separator();
                                        ui.label(
                                            egui::RichText::new("+ Route Send To:")
                                                .size(10.0)
                                                .color(TEXT_DIM),
                                        );
                                        for (dst_id, dst_name) in &all_track_targets {
                                            if *dst_id != tid
                                                && project.can_route(tid, *dst_id)
                                                && ui.button(dst_name).clicked()
                                            {
                                                let _ = project.add_send(tid, *dst_id, -9.0);
                                                actions.push(DockAction::ProjectMutated(
                                                    "Add Track Send",
                                                ));
                                                ui.close_menu();
                                            }
                                        }
                                    },
                                );
                            });
                        });
                }

                ui.separator();

                // Dedicated MASTER BUS Strip on far right!
                egui::Frame::new()
                    .fill(BG_DEEP)
                    .stroke(Stroke::new(1.5_f32, ACCENT_TEAL))
                    .corner_radius(6)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.set_width(116.0);
                        ui.vertical_centered(|ui| {
                            if ui
                                .button(
                                    egui::RichText::new("MASTER BUS")
                                        .size(11.5)
                                        .strong()
                                        .color(ACCENT_TEAL),
                                )
                                .clicked()
                            {
                                *inspect_master = true;
                            }
                            ui.horizontal(|ui| {
                                if pill_toggle(ui, "MONO", project.master.mono_check, ACCENT_AMBER)
                                    .on_hover_text("Mono Compatibility Check")
                                    .clicked()
                                {
                                    project.master.mono_check = !project.master.mono_check;
                                    actions.push(DockAction::ProjectMutated("Toggle Master Mono"));
                                }
                                if pill_toggle(ui, "DIM", project.master.dim_enabled, ACCENT_FERRIC)
                                    .on_hover_text("-18 dB Control Room Dim")
                                    .clicked()
                                {
                                    project.master.dim_enabled = !project.master.dim_enabled;
                                    actions.push(DockAction::ProjectMutated("Toggle Master Dim"));
                                }
                            });
                            if pill_toggle(
                                ui,
                                "SAFETY LIMIT",
                                project.master.safety_limiter,
                                ACCENT_TEAL,
                            )
                            .clicked()
                            {
                                project.master.safety_limiter = !project.master.safety_limiter;
                                actions.push(DockAction::ProjectMutated("Toggle Safety Limiter"));
                            }

                            ui.horizontal(|ui| {
                                ui.add_space(22.0);
                                if vertical_fader(
                                    ui,
                                    &mut project.master.gain_db,
                                    96.0,
                                    ACCENT_TEAL,
                                )
                                .changed()
                                {
                                    actions.push(DockAction::ProjectMutated("Adjust Master Fader"));
                                }
                                stereo_level_meter(
                                    ui,
                                    telem.master.peak_l_db,
                                    telem.master.peak_r_db,
                                    96.0,
                                );
                            });

                            ui.label(
                                egui::RichText::new(format!(
                                    "{:.1} LUFS",
                                    telem.master.momentary_lufs
                                ))
                                .size(11.0)
                                .monospace()
                                .color(ACCENT_TEAL),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "Pk {:+.1} dB",
                                    telem.master.peak_l_db.max(telem.master.peak_r_db)
                                ))
                                .size(10.0)
                                .monospace()
                                .color(TEXT_MAIN),
                            );
                        });
                    });
            });
        });
}

/// REAPER's beloved Routing Matrix Crosspoint Grid — route any track into any Folder or Aux Send with 1 click!
pub fn render_routing_matrix(
    ui: &mut Ui,
    project: &mut Project,
    actions: &mut Vec<DockAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("REAPER-Style Universal Routing Matrix")
                .strong()
                .color(ACCENT_FERRIC),
        );
        ui.label(
            egui::RichText::new(
                "Click crosspoints to toggle Post-Fader Aux Sends • Folder column assigns submix parent (cycle-safe DAG).",
            )
            .size(11.0)
            .color(TEXT_DIM),
        );
    });
    ui.separator();

    let tracks_meta: Vec<(TrackId, String)> = project
        .tracks
        .iter()
        .map(|t| (t.id, t.name.clone()))
        .collect();

    egui::ScrollArea::both()
        .id_salt("routing_matrix_scroll")
        .show(ui, |ui| {
            egui::Grid::new("reaper_routing_grid")
                .striped(true)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Source ╲ Dest").strong().size(11.0));
                    ui.label(egui::RichText::new("Master").strong().size(11.0).color(ACCENT_TEAL));
                    ui.label(egui::RichText::new("Folder Bus").strong().size(11.0).color(ACCENT_AMBER));
                    for (_, name) in &tracks_meta {
                        ui.label(egui::RichText::new(name).size(10.5));
                    }
                    ui.end_row();

                    for src_i in 0..tracks_meta.len() {
                        let (src_id, ref src_name) = tracks_meta[src_i];
                        ui.label(egui::RichText::new(src_name).strong().size(11.0));

                        // Master Out toggle
                        let mut master_on = project.tracks[src_i].send_to_master
                            && project.tracks[src_i].parent_folder_id.is_none();
                        if ui.checkbox(&mut master_on, "").changed() {
                            project.tracks[src_i].send_to_master = master_on;
                            if master_on {
                                project.tracks[src_i].parent_folder_id = None;
                            }
                            actions.push(DockAction::ProjectMutated("Toggle Master Send"));
                        }

                        // Folder Parent dropdown
                        let cur_parent_lbl = project.tracks[src_i]
                            .parent_folder_id
                            .and_then(|pid| tracks_meta.iter().find(|(i, _)| *i == pid))
                            .map(|(_, n)| n.as_str())
                            .unwrap_or("None");
                        egui::ComboBox::from_id_salt(("folder_parent", src_id))
                            .selected_text(cur_parent_lbl)
                            .width(120.0)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(
                                        project.tracks[src_i].parent_folder_id.is_none(),
                                        "None (Direct to Master)",
                                    )
                                    .clicked()
                                {
                                    project.tracks[src_i].parent_folder_id = None;
                                    actions.push(DockAction::ProjectMutated("Clear Folder Parent"));
                                }
                                for (cand_id, cand_name) in &tracks_meta {
                                    if *cand_id != src_id && project.can_route(src_id, *cand_id) {
                                        let active =
                                            project.tracks[src_i].parent_folder_id == Some(*cand_id);
                                        if ui.selectable_label(active, cand_name).clicked() {
                                            project.tracks[src_i].parent_folder_id = Some(*cand_id);
                                            actions.push(DockAction::ProjectMutated(
                                                "Assign Folder Parent",
                                            ));
                                        }
                                    }
                                }
                            });

                        // Track-to-Track Send Crosspoints
                        for (dst_id, _) in &tracks_meta {
                            if *dst_id == src_id {
                                ui.label(egui::RichText::new("—").color(TEXT_DIM));
                                continue;
                            }
                            let has_send = project.tracks[src_i]
                                .sends
                                .iter()
                                .any(|s| s.target_track_id == *dst_id);
                            let allowed = has_send || project.can_route(src_id, *dst_id);
                            if !allowed {
                                ui.label(
                                    egui::RichText::new("↺")
                                        .color(Color32::from_rgb(80, 82, 92)),
                                )
                                .on_hover_text("Blocked: would create feedback loop");
                            } else {
                                let lbl = if has_send { "● SEND" } else { "·" };
                                if pill_toggle(ui, lbl, has_send, ACCENT_FERRIC).clicked() {
                                    if has_send {
                                        project.tracks[src_i]
                                            .sends
                                            .retain(|s| s.target_track_id != *dst_id);
                                    } else {
                                        let _ = project.add_send(src_id, *dst_id, -9.0);
                                    }
                                    actions.push(DockAction::ProjectMutated("Toggle Routing Send"));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
        });
}
