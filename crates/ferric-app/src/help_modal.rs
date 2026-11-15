use crate::theme::{
    ACCENT_AMBER, ACCENT_FERRIC, ACCENT_TEAL, BG_DEEP, BG_ELEVATED, TEXT_DIM, TEXT_MAIN,
};
use egui::{Context, RichText, Stroke};

/// Generates a crisp 64×64 RGBA Ferric Copper-Oxide Magnetic Tape Reel + Waveform Studio Icon!
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

            // Rounded studio badge background
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

            // Deep dark slate background
            let mut col = [18_u8, 20, 27, 255];

            // Outer Ferric Copper-Oxide Reel Ring
            if (21.0..=26.5).contains(&r) {
                col = [232, 114, 62, 255];
            } else if (12.0..21.0).contains(&r) {
                // Dark magnetic tape pack with 3 reel window cutouts
                let ang = dy.atan2(dx);
                let spoke = ((ang * 3.0).cos()).abs();
                if spoke > 0.65 {
                    col = [34, 38, 49, 255];
                } else {
                    col = [56, 34, 28, 255];
                }
            } else if r <= 7.5 {
                // Teal precision hub spindle
                col = if r <= 3.2 {
                    [16, 18, 24, 255]
                } else {
                    [66, 196, 176, 255]
                };
            }

            // Crisp audio transient waveform pulse across bottom-right quadrant
            let wave_y = 32.0 - 9.0 * ((fx - 32.0) * 0.35).sin() * (-((fx - 32.0).abs() * 0.06)).exp();
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

/// Returns `true` when the user clicks **Start Night Current Tutorial**.
pub fn render_quickstart_help_modal(ctx: &Context, open: &mut bool) -> bool {
    if !*open {
        return false;
    }
    let mut start_tutorial = false;
    egui::Window::new("❓ FerricDAW v1.1.1 — Studio Quickstart & Shortcut Guide")
        .open(open)
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_size([660.0, 510.0])
        .frame(
            egui::Frame::new()
                .fill(BG_ELEVATED)
                .stroke(Stroke::new(1.5_f32, ACCENT_FERRIC))
                .corner_radius(8)
                .inner_margin(14.0),
        )
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(
                    RichText::new("Welcome to FerricDAW v1.1.1")
                        .size(16.0)
                        .strong()
                        .color(ACCENT_FERRIC),
                );
                ui.label(
                    RichText::new(
                        "Ultra-lightweight pure-Rust Hybrid DAW combining REAPER's 'Track = Anything' architecture with Bitwig/Ableton Session Launching, Modulators, WSOLA Warp, CLAP + VST3 Hosting & -14 LUFS Mastering.",
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
                            RichText::new("🎓 Build Night Current (in-app tutorial)")
                                .strong()
                                .color(ACCENT_FERRIC),
                        );
                        ui.label(
                            RichText::new(
                                "Step-by-step coach that walks you through the whole dark-techno song — chords, Generate Arr, drums, hook, sidechain, mix, bounce.",
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
                            start_tutorial = true;
                        }
                    });
                ui.separator();

                // Section 1: Hardware Rig
                egui::Frame::new()
                    .fill(BG_DEEP)
                    .corner_radius(6)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("1. Hardware Studio Quickstart (Guitars, Mics, Akai LPK25 & Nintendo DS)")
                                .strong()
                                .color(ACCENT_TEAL),
                        );
                        ui.label("• Click '🎹 Rig / DS-10' in the left Browser -> '🚀 Spawn LPK25 + DS-10 Hybrid Tracks' to create armed USB MIDI + Stereo Line-In channels in 1 click.");
                        ui.label("• DS Line-In: Out=Speakers, In=External Microphone (Realtek). Keep Windows Mic Boost at 0%. Raise DS headphone volume, use Ferric +6 dB trim — not Mic Boost (hiss).");
                        ui.label("• Use 'Buffer: 64..1024 smp' to tune real-time latency, and click '📡 Loopback Ping' to calibrate sample-accurate overdub offset.");
                        ui.label("• Play a beat on external gear (like KORG DS-10) over 3.5mm Line-In and click '⚡ Lock BPM' (or tap 'TAP' 4x) to lock FerricDAW's tempo.");
                    });

                ui.add_space(6.0);

                // Section 2: Plugins & Editing
                egui::Frame::new()
                    .fill(BG_DEEP)
                    .corner_radius(6)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("2. CLAP / VST3 Plugins, Swipe Comping, Warp & Conductor")
                                .strong()
                                .color(ACCENT_AMBER),
                        );
                        ui.label("• Third-Party Plugins: Open '🎛 CLAP / VST3' in the left Browser -> click '+ Insert' on any plugin (e.g. Dexed, Six Sines, Vital, Surge XT), then click '🗔 Open UI' on its FX card to pop open its floating graphical editor.");
                        ui.label("• Multi-Take Swipe Comping: Loop-record or right-click any clip -> 'Show Multi-Take Swipe-Comp Lanes', then drag across takes to build seamless crossfaded comps.");
                        ui.label("• Auto-Crossfades & Glue: Overlap any two audio clips on a track for automatic Equal-Power crossfades, or select multiple clips and press Ctrl+G to Glue/Consolidate.");
                        ui.label("• Dynamic Conductor: Click '🎼 Tempo / Meter Map' above the timeline to draw linear tempo ramps and mid-song 3/4, 6/8, or 7/8 time-signature changes.");
                        ui.label("• Chord Lane + CoF: Toggle '🎹 Chord Lane' to edit harmony, drop Progression presets, Generate Pad/Bass/Arp, or enable ♮ Chord-Follow on a track header.");
                        ui.label("• VCA & Mix Recall: Mixing Console → + VCA strips + Mix A/B/C/D capture/recall for instant console compares.");
                        ui.label("• Pro I/O: Hardware Rig → WASAPI Shared / Low Buffer / ASIO (falls back honestly if missing) + multi-channel In 1..16; sandboxed CLAP/VST3 by default (🛡 Sandbox on FX card).");
                        ui.label("• Bounce MP3/OGG: Mastering Bounce now offers lossy delivery formats beside WAV/FLAC.");
                        ui.label("• Stem Split: Right-click an audio clip → Split to 4 Stems (Drums / Bass / Mids / Highs).");
                    });

                ui.add_space(8.0);
                ui.label(
                    RichText::new("3. Essential Keyboard & Mouse Shortcuts")
                        .strong()
                        .color(TEXT_MAIN),
                );
                egui::Grid::new("help_shortcuts_grid")
                    .striped(true)
                    .spacing([18.0, 5.0])
                    .show(ui, |ui| {
                        let rows = [
                            ("Space", "Play / Pause Transport"),
                            ("Ctrl + K", "Open Instant Command & FX Palette"),
                            ("F1", "Open / Close this Quickstart & Shortcut Guide"),
                            ("Shift + R", "Retroactive Capture (drops last 15s of unrecorded MIDI / Audio!)"),
                            ("A W S E D F T G Y H U J K", "Musical Typing Piano Keys (Z / X changes Octave)"),
                            ("Ctrl + T", "Add New Universal Audio / MIDI / Folder Track"),
                            ("Ctrl + F", "1-Click Reversible Track Freeze (0.0% DSP CPU 32-bit Stem)"),
                            ("Ctrl + G", "Glue / Consolidate Selected Clips on Track"),
                            ("Ctrl + C / X / V / D", "Copy / Cut / Paste / Duplicate Selected Clip(s)"),
                            ("Drag Empty Timeline", "Rubber-Band Marquee Multi-Clip Selection"),
                            ("Ctrl + Wheel / Shift + Wheel", "Zoom Timeline Horizontally / Pan Song Horizontally"),
                            ("Shift + Drag Automation Pt", "Bend Exponential / Logarithmic Bezier Curve Tension"),
                            ("Ctrl + B", "Mastering Bounce (WAV / FLAC / MP3 / OGG, -14 LUFS, True-Peak, TPDF Dither)"),
                            ("Ctrl + S / Ctrl + O", "Save / Open Lossless Session Bundle (.ferric + _Audio/ folder)"),
                            ("Ctrl + Z / Ctrl + Y", "Instant Undo / Redo (< 1 ms Arc-backed snapshots)"),
                        ];
                        for (k, desc) in rows {
                            ui.label(RichText::new(k).monospace().strong().color(ACCENT_TEAL));
                            ui.label(desc);
                            ui.end_row();
                        }
                    });
            });
        });
    if start_tutorial {
        *open = false;
    }
    start_tutorial
}
