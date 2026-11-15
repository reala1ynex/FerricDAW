//! Floating in-app coach that walks **Night Current** end-to-end.

use crate::theme::{
    StudioUiExt,
    ACCENT_AMBER, ACCENT_FERRIC, ACCENT_TEAL, BG_DEEP, BG_ELEVATED, TEXT_DIM, TEXT_MAIN,
};
use egui::{Context, RichText, Stroke};
use ferric_model::{night_current_step_complete, NightCurrentStep, Project};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TutorialAction {
    None,
    /// Rebuild project through the current step (or start Session from Welcome).
    ApplyStep,
    Next,
    Back,
    Close,
    OpenBounce,
    /// Audition shipped demo/NightCurrent-Demo.wav via engine preview.
    PlayReference,
    Restart,
}

#[derive(Debug, Clone)]
pub struct TutorialCoach {
    pub open: bool,
    pub step: NightCurrentStep,
}

impl Default for TutorialCoach {
    fn default() -> Self {
        Self {
            open: false,
            step: NightCurrentStep::Welcome,
        }
    }
}

impl TutorialCoach {
    pub fn start(&mut self) {
        self.open = true;
        self.step = NightCurrentStep::Welcome;
    }

    pub fn show(
        &mut self,
        ctx: &Context,
        project: &Project,
        other_overlay_open: bool,
    ) -> TutorialAction {
        if !self.open {
            return TutorialAction::None;
        }

        // Esc dismisses the coach when nothing else owns Esc — never wipes the session.
        if !other_overlay_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.open = false;
            return TutorialAction::Close;
        }

        let mut action = TutorialAction::None;
        let step = self.step;
        let done = night_current_step_complete(project, step);
        let idx = step.index();
        let total = NightCurrentStep::ALL.len();

        egui::Window::new("🎓 Night Current Tutorial")
            .open(&mut self.open)
            .collapsible(true)
            .resizable(true)
            // Bottom-right so Chord Lane / Gen Arr stay visible.
            .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -48.0])
            .default_size([360.0, 460.0])
            .min_width(300.0)
            .default_open(true)
            .frame(
                egui::Frame::new()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.5_f32, ACCENT_FERRIC))
                    .corner_radius(8)
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Step {} / {}", idx + 1, total))
                            .size(11.0)
                            .color(TEXT_DIM),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .chrome_button(RichText::new("✕ Close").size(12.0).color(TEXT_DIM))
                            .on_hover_text("Close tutorial coach (Esc) — keeps your session")
                            .clicked()
                        {
                            action = TutorialAction::Close;
                        }
                        ui.label(
                            RichText::new(step.letter())
                                .size(14.0)
                                .strong()
                                .color(ACCENT_TEAL),
                        );
                    });
                });

                // Progress ticks
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (i, &s) in NightCurrentStep::ALL.iter().enumerate() {
                        let filled = i <= idx;
                        let color = if filled { ACCENT_FERRIC } else { TEXT_DIM };
                        let w = if s == step { 18.0 } else { 10.0 };
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(w, 4.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, color);
                    }
                });

                ui.add_space(8.0);
                ui.label(
                    RichText::new(step.title())
                        .size(15.0)
                        .strong()
                        .color(ACCENT_FERRIC),
                );
                ui.add_space(4.0);

                egui::Frame::new()
                    .fill(BG_DEEP)
                    .corner_radius(6)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("What you’re aiming for")
                                .size(11.0)
                                .strong()
                                .color(ACCENT_TEAL),
                        );
                        ui.label(RichText::new(step.goal()).size(12.0).color(TEXT_MAIN));
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Do this")
                                .size(11.0)
                                .strong()
                                .color(ACCENT_AMBER),
                        );
                        ui.label(RichText::new(step.how_to()).size(12.0).color(TEXT_MAIN));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(step.ui_hint())
                                .size(11.0)
                                .italics()
                                .color(TEXT_DIM),
                        );
                    });

                if matches!(step, NightCurrentStep::Counter) {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Section layout")
                            .size(11.0)
                            .strong()
                            .color(TEXT_DIM),
                    );
                    ui.label(
                        RichText::new(
                            "1–2 Intro: sparse drums\n\
                             3–4 Groove: full drums\n\
                             5–8 Hook A: main lead on\n\
                             9–12 Stack: arp + counter on\n\
                             13–16 Hook B: lead back, louder",
                        )
                        .size(11.0)
                        .color(TEXT_MAIN),
                    );
                }

                ui.add_space(8.0);
                let status = if done {
                    ("✓ Looks good — hit Next when you’re ready", ACCENT_TEAL)
                } else if step.is_build_step() || step == NightCurrentStep::Welcome {
                    ("○ Not finished yet — follow Do this, or use the teal button", ACCENT_AMBER)
                } else {
                    ("→ Bounce, then compare to the demo and hit Next", TEXT_DIM)
                };
                ui.label(RichText::new(status.0).size(11.5).color(status.1));

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            step != NightCurrentStep::Welcome,
                            egui::Button::new("← Back")
                                .fill(crate::theme::BUTTON_SURFACE)
                                .stroke(egui::Stroke::new(1.0_f32, crate::theme::BORDER_SUBTLE))
                                .corner_radius(crate::theme::BUTTON_RADIUS),
                        )
                        .clicked()
                    {
                        action = TutorialAction::Back;
                    }

                    if step == NightCurrentStep::Welcome {
                        if ui
                        .chrome_button(
                                RichText::new("Begin Session")
                                    .strong()
                                    .color(ACCENT_FERRIC),
                            )
                            .on_hover_text(
                                "Replaces the current project with a clean Night Current at 128 BPM",
                            )
                            .clicked()
                        {
                            action = TutorialAction::ApplyStep;
                        }
                    } else if step.is_build_step() {
                        if ui
                        .chrome_button(
                                RichText::new("Do this step for me")
                                    .strong()
                                    .color(ACCENT_TEAL),
                            )
                            .on_hover_text(
                                "Rebuilds the tutorial song through this step. Replaces the project — save first if you care about what you have open.",
                            )
                            .clicked()
                        {
                            action = TutorialAction::ApplyStep;
                        }
                    } else if step == NightCurrentStep::Bounce {
                        if ui
                        .chrome_button(RichText::new("Open Bounce…").strong().color(ACCENT_AMBER))
                            .clicked()
                        {
                            action = TutorialAction::OpenBounce;
                        }
                        if ui
                        .chrome_button(RichText::new("Play demo WAV").color(ACCENT_TEAL))
                            .on_hover_text("Play demo/NightCurrent-Demo.wav")
                            .clicked()
                        {
                            action = TutorialAction::PlayReference;
                        }
                    } else if step == NightCurrentStep::Done {
                        if ui
                        .chrome_button(RichText::new("Play demo WAV").strong().color(ACCENT_TEAL))
                            .clicked()
                        {
                            action = TutorialAction::PlayReference;
                        }
                        if ui.chrome_button("Restart tutorial").clicked() {
                            action = TutorialAction::Restart;
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let next_label = if step == NightCurrentStep::Done {
                            "Close"
                        } else {
                            "Next →"
                        };
                        if ui
                        .chrome_button(RichText::new(next_label).strong().color(ACCENT_FERRIC))
                            .clicked()
                        {
                            action = if step == NightCurrentStep::Done {
                                TutorialAction::Close
                            } else {
                                TutorialAction::Next
                            };
                        }
                    });
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .chrome_button(RichText::new("Close tutorial").color(TEXT_DIM))
                        .on_hover_text("Close the coach (Esc). Your project stays open.")
                        .clicked()
                    {
                        action = TutorialAction::Close;
                    }
                    ui.label(
                        RichText::new("Esc closes · Space plays · Ctrl+S saves")
                            .size(10.5)
                            .color(TEXT_DIM),
                    );
                });
            });

        if action == TutorialAction::Close {
            self.open = false;
        }
        if !self.open && action == TutorialAction::None {
            action = TutorialAction::Close;
        }

        action
    }
}
