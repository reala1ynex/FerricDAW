use crate::theme::{
    ACCENT_AMBER, ACCENT_FERRIC, ACCENT_TEAL, BG_DEEP, BG_ELEVATED, TEXT_DIM, TEXT_MAIN,
};
use egui::{Context, RichText, Stroke};

/// 64×64 RGBA app icon (reel + waveform).
pub fn build_app_icon() -> egui::IconData {
    let w = 64usize;
    let h = 64usize;
    let mut rgba = vec![0u8; w * h * 4];
    let cx = 31.5_f32;
    let cy = 31.5_f32;

    for y in 0..h {
        for x in 0..w {
            let fx = x as f32;
            let fy = y as f32;
            let dx = fx - cx;
            let dy = fy - cy;
            let r = (dx * dx + dy * dy).sqrt();

            let in_badge = {
                let bx = (dx.abs() - 22.0).max(0.0);
                let by = (dy.abs() - 22.0).max(0.0);
                (bx * bx + by * by).sqrt() <= 8.5
            };

            let idx = (y * w + x) * 4;
            if !in_badge {
                rgba[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            let mut col = [18_u8, 20, 27, 255];

            if (21.0..=26.5).contains(&r) {
                col = [232, 114, 62, 255];
            } else if (12.0..21.0).contains(&r) {
                let ang = dy.atan2(dx);
                let spoke = ((ang * 3.0).cos()).abs();
                if spoke > 0.65 {
                    col = [34, 38, 49, 255];
                } else {
                    col = [56, 34, 28, 255];
                }
            } else if r <= 7.5 {
                col = if r <= 3.2 {
                    [16, 18, 24, 255]
                } else {
                    [66, 196, 176, 255]
                };
            }

            let wave_y =
                32.0 - 9.0 * ((fx - 32.0) * 0.35).sin() * (-((fx - 32.0).abs() * 0.06)).exp();
            if (fy - wave_y).abs() < 1.35 && r < 24.0 {
                col = [245, 248, 255, 255];
            }

            rgba[idx..idx + 4].copy_from_slice(&col);
        }
    }

    egui::IconData {
        rgba,
        width: w as u32,
        height: h as u32,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum HelpTab {
    #[default]
    Quickstart,
    FeatureGuide,
    Shortcuts,
}

/// Returns `true` when the user clicks **Start Night Current Tutorial**.
pub fn render_quickstart_help_modal(ctx: &Context, open: &mut bool) -> bool {
    if !*open {
        return false;
    }
    let mut start_tutorial = false;
    egui::Window::new("FerricDAW Help")
        .open(open)
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_size([720.0, 560.0])
        .frame(
            egui::Frame::new()
                .fill(BG_ELEVATED)
                .stroke(Stroke::new(1.5_f32, ACCENT_FERRIC))
                .corner_radius(8)
                .inner_margin(14.0),
        )
        .show(ctx, |ui| {
            let tab_id = egui::Id::new("ferric_help_tab");
            let mut tab = *ui
                .ctx()
                .data(|d| d.get_temp::<HelpTab>(tab_id))
                .get_or_insert(HelpTab::Quickstart);

            ui.horizontal(|ui| {
                for (t, label) in [
                    (HelpTab::Quickstart, "Quickstart"),
                    (HelpTab::FeatureGuide, "Feature Guide"),
                    (HelpTab::Shortcuts, "Shortcuts"),
                ] {
                    if ui
                        .selectable_label(tab == t, RichText::new(label).strong())
                        .clicked()
                    {
                        tab = t;
                    }
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| match tab {
                HelpTab::Quickstart => render_quickstart_tab(ui, &mut start_tutorial),
                HelpTab::FeatureGuide => render_feature_guide_tab(ui),
                HelpTab::Shortcuts => render_shortcuts_tab(ui),
            });
        });
    if start_tutorial {
        *open = false;
    }
    start_tutorial
}

fn render_quickstart_tab(ui: &mut egui::Ui, start_tutorial: &mut bool) {
    ui.label(
        RichText::new("Quickstart")
            .size(16.0)
            .strong()
            .color(ACCENT_FERRIC),
    );
    ui.label(
        RichText::new(
            "Hybrid DAW — tracks hold audio, MIDI, instruments, and FX. Plugins run in a \
             sandbox so a crash doesn’t take the whole app down.",
        )
        .size(11.5)
        .color(TEXT_DIM),
    );
    ui.add_space(8.0);
    egui::Frame::new()
        .fill(BG_DEEP)
        .corner_radius(6)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.label(
                RichText::new("Build Night Current")
                    .strong()
                    .color(ACCENT_FERRIC),
            );
            ui.label(
                RichText::new(
                    "Short dark-techno tutorial — chords, generate, drums, hook, sidechain, \
                     mix, bounce. Feature Guide tab has the rest of the studio.",
                )
                .size(11.5)
                .color(TEXT_MAIN),
            );
            ui.add_space(6.0);
            if ui
                .button(
                    RichText::new("Start Night Current Tutorial")
                        .strong()
                        .color(ACCENT_TEAL),
                )
                .clicked()
            {
                *start_tutorial = true;
            }
        });
    ui.add_space(8.0);
    egui::Frame::new()
        .fill(BG_DEEP)
        .corner_radius(6)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.label(
                RichText::new("1. MIDI + Line-In")
                    .strong()
                    .color(ACCENT_TEAL),
            );
            ui.label("• Browser → Hardware Rig → Spawn MIDI + Line-In.");
            ui.label("• Speakers out, External Mic / Line-In in. Windows Mic Boost = 0%; use Ferric trim.");
            ui.label("• Buffer 64–1024. Loopback Ping sets overdub offset. Exclusive locks the out device when it can.");
            ui.label("• Tap Tempo · Line-In BPM · keyboard pads (letters = Pad A, numpad = Pad B).");
        });
    ui.add_space(6.0);
    egui::Frame::new()
        .fill(BG_DEEP)
        .corner_radius(6)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.label(
                RichText::new("2. Worth trying next")
                    .strong()
                    .color(ACCENT_AMBER),
            );
            ui.label("• DNA strip: Feel + Velocity on a flat hat grid.");
            ui.label("• Ctrl+K → HostDNA Track → Reseed / Paste seed / A·B·C.");
            ui.label("• Storyboard Gen Pad on a section → Undo Generate.");
            ui.label("• Ctrl+B → Golden Bounce vs the Night Current demo.");
            ui.label("• Lower dock → Launch Pad (quantize + Mix morph).");
            ui.label("• Plugins: ▶ Audition in the sandbox → Insert if it survives.");
        });
}

fn render_feature_guide_tab(ui: &mut egui::Ui) {
    ui.label(
        RichText::new("Feature Guide")
            .size(15.0)
            .strong()
            .color(ACCENT_FERRIC),
    );
    ui.label(
        RichText::new("Longer write-up: docs/FEATURE_GUIDE.md in the repo or release zip.")
            .size(11.0)
            .color(TEXT_DIM),
    );
    ui.add_space(6.0);

    guide_block(ui, "Groove DNA + Velocity DNA", ACCENT_TEAL, &[
        "Under the transport: Feel (timing pocket) + Velocity (accents).",
        "Grooves: Straight / Techno Pocket / Boom-Bap / Push. Accents: Flat / Offbeat / Downbeat / Hat Chatter.",
        "Apply to drums or all MIDI. Bounce matches what you hear live.",
        "Accents stay the same for a given project — not random spray.",
    ]);
    guide_block(ui, "HostDNA Synth", ACCENT_FERRIC, &[
        "Ctrl+K → Add HostDNA Track. Editable Init from a coarse PC fingerprint.",
        "Reseed · Randomize · Paste seed hex · A/B/C siblings.",
        "Only a Host Seed hex is shown. Travels with the patch. Not DRM.",
    ]);
    guide_block(ui, "Chord Storyboard", egui::Color32::from_rgb(186, 108, 226), &[
        "Section chips: Intro / Groove / Hook / Stack.",
        "Gen Pad / Bass / Arp only writes inside that section.",
        "Patch card shows what got made. Undo Generate restores the previous state.",
    ]);
    guide_block(ui, "Golden Bounce", ACCENT_AMBER, &[
        "After a master bounce: LUFS / true-peak / duration vs the demo or a file you pick.",
        "Loudness-matched A/B · rough waveforms · Δ LUFS.",
    ]);
    guide_block(ui, "Scene Launch Pad", ACCENT_TEAL, &[
        "Lower dock → Launch Pad. Fire scenes quantized to bar / beat / ¼.",
        "Optional Mix morph from the live mix toward Mix A–D.",
        "Keys 1–8 fire scenes while the Launch Pad tab is focused.",
    ]);
    guide_block(ui, "Plugin Playground", ACCENT_AMBER, &[
        "▶ Audition loads in a sandbox. Crash → quarantine. Survive → Insert.",
        "+ Insert still works (sandbox on by default). Crashes shouldn’t kill Ferric.",
    ]);
    guide_block(ui, "Hardware Rig", ACCENT_FERRIC, &[
        "Skins are labels only (Desk / Laptop Pads / Handheld Tip).",
        "Status shows Exclusive vs Shared, buffer, ping, MIDI / XY / Line-In.",
        "Any USB MIDI controller + any line source.",
    ]);
    guide_block(ui, "Always there", TEXT_MAIN, &[
        "Universal tracks · VCA · Mix A–D · sidechain · freeze · PDC · session clips.",
        "Built-in instruments + FX · CLAP/VST3 · Chord Lane · Kaoss · tempo maps.",
        "Bounce WAV/FLAC/MP3/OGG · −14 LUFS · true-peak · .ferric projects.",
    ]);
}

fn guide_block(ui: &mut egui::Ui, title: &str, color: egui::Color32, bullets: &[&str]) {
    ui.add_space(4.0);
    egui::Frame::new()
        .fill(BG_DEEP)
        .corner_radius(6)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.label(RichText::new(title).strong().color(color));
            for b in bullets {
                ui.label(format!("• {b}"));
            }
        });
}

fn render_shortcuts_tab(ui: &mut egui::Ui) {
    ui.label(
        RichText::new("Shortcuts")
            .strong()
            .color(TEXT_MAIN),
    );
    ui.add_space(4.0);
    egui::Grid::new("help_shortcuts_grid")
        .striped(true)
        .spacing([18.0, 5.0])
        .show(ui, |ui| {
            let rows = [
                ("Space", "Play / Pause"),
                ("Stop (transport)", "Stop playback — playhead stays"),
                ("R / L / C", "Record / Loop / Click (when Pads are off)"),
                ("S / Q", "Split clip at playhead / Quantize MIDI 1/16 (Pads off)"),
                ("◀Bar / Bar▶ / drag BBT", "Nudge or scrub the playhead"),
                ("Click ruler / empty lane", "Jump playhead"),
                ("Ctrl + K", "Command palette (add tracks, FX, tools)"),
                ("F1", "Help — Quickstart / Feature Guide / Shortcuts"),
                ("+ Track menu", "Vocal · MIDI · Instrument · Audio · Universal"),
                ("1–8", "Fire Launch Pad scenes (Launch tab focused)"),
                ("Shift + R", "Retroactive Capture (~15s MIDI / audio)"),
                ("Backspace / Delete", "Delete selected MIDI notes (in piano roll)"),
                ("Right-click MIDI note", "Edit velocity / length / pitch"),
                ("Scroll on piano roll", "Zoom at cursor · Shift+scroll pans pitch"),
                ("Alt / Middle-drag timeline", "Pan time + scroll across tracks"),
                ("A W S E D F T G Y H U J K", "Musical Typing when Pads are on (Z / X = octave)"),
                ("Ctrl + T", "Add Universal Track"),
                ("Ctrl + F", "Toggle Track Freeze"),
                ("Ctrl + G", "Glue / Consolidate Selected Clips"),
                ("Ctrl + C / X / V / D", "Copy / Cut / Paste / Duplicate Clip(s)"),
                ("Ctrl + B", "Bounce (+ Golden Bounce after export)"),
                ("Ctrl + S / Ctrl + O", "Save / Open .ferric session"),
                ("Ctrl + Z / Ctrl + Y", "Undo / Redo"),
            ];
            for (k, desc) in rows {
                ui.label(RichText::new(k).monospace().strong().color(ACCENT_TEAL));
                ui.label(desc);
                ui.end_row();
            }
        });
}
