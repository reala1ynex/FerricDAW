use egui::{
    epaint::Shadow, Align2, Color32, Context, CornerRadius, FontId, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, Ui, Vec2, Visuals,
};
use std::f32::consts::PI;

pub const BG_DEEP: Color32 = Color32::from_rgb(7, 8, 10);
pub const BG_PANEL: Color32 = Color32::from_rgb(18, 22, 27);
pub const BG_ELEVATED: Color32 = Color32::from_rgb(26, 33, 40);
pub const BG_TRACK_EVEN: Color32 = Color32::from_rgb(15, 19, 23);
pub const BG_TRACK_ODD: Color32 = Color32::from_rgb(20, 25, 30);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(49, 61, 71);
pub const ACCENT_FERRIC: Color32 = Color32::from_rgb(255, 90, 31);
pub const ACCENT_TEAL: Color32 = Color32::from_rgb(66, 196, 176);
pub const ACCENT_AMBER: Color32 = Color32::from_rgb(240, 182, 62);
pub const ACCENT_PURPLE: Color32 = Color32::from_rgb(186, 108, 226);
pub const ACCENT_CYAN: Color32 = Color32::from_rgb(0, 229, 255);
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(59, 139, 255);
pub const ACCENT_RED: Color32 = Color32::from_rgb(232, 74, 86);
pub const STATE_PLAY: Color32 = Color32::from_rgb(46, 232, 134);
pub const STATE_ARM: Color32 = Color32::from_rgb(255, 46, 99);
pub const STATE_SOLO: Color32 = Color32::from_rgb(255, 194, 26);
pub const TEXT_MAIN: Color32 = Color32::from_rgb(242, 242, 242);
pub const TEXT_DIM: Color32 = Color32::from_rgb(138, 180, 189);
pub const BUTTON_SURFACE: Color32 = Color32::from_rgb(35, 44, 52);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(49, 62, 73);
pub const BUTTON_ACTIVE: Color32 = Color32::from_rgb(58, 72, 84);
pub const BUTTON_FOCUS: Color32 = Color32::from_rgb(136, 166, 177);
pub const BUTTON_HEIGHT: f32 = 24.0;
pub const BUTTON_RADIUS: u8 = 5;

pub fn apply_studio_theme(ctx: &Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_ELEVATED;
    visuals.extreme_bg_color = BG_DEEP;
    visuals.faint_bg_color = BG_TRACK_ODD;
    visuals.selection.bg_fill = ACCENT_CYAN.gamma_multiply(0.26);
    visuals.selection.stroke = Stroke::new(1.5_f32, ACCENT_CYAN);
    visuals.widgets.noninteractive.bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_MAIN);
    visuals.widgets.inactive.bg_fill = BUTTON_SURFACE;
    visuals.widgets.inactive.weak_bg_fill = BUTTON_SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_MAIN);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(BUTTON_RADIUS);
    visuals.widgets.hovered.bg_fill = BUTTON_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BUTTON_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BUTTON_FOCUS);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_MAIN);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(BUTTON_RADIUS);
    visuals.widgets.active.bg_fill = BUTTON_ACTIVE;
    visuals.widgets.active.weak_bg_fill = BUTTON_ACTIVE;
    visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, ACCENT_CYAN);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT_MAIN);
    visuals.widgets.active.corner_radius = CornerRadius::same(BUTTON_RADIUS);
    visuals.widgets.open = visuals.widgets.hovered;
    visuals.button_frame = true;
    visuals.menu_corner_radius = CornerRadius::same(BUTTON_RADIUS);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.window_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(150),
    };
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(6.0, 5.0);
    style.spacing.button_padding = Vec2::new(8.0, 3.0);
    style.spacing.interact_size = Vec2::new(24.0, BUTTON_HEIGHT);
    style.spacing.menu_margin = egui::Margin::same(4);
    ctx.set_style(style);
}

pub fn track_color32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Custom anti-aliased vector studio rotary knob with optional live modulation ring!
pub fn studio_knob(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    min: f32,
    max: f32,
    default_val: f32,
    unit: &str,
    diameter: f32,
    accent: Color32,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.set_width(diameter + 18.0);
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(diameter + 14.0, diameter + 4.0), Sense::click_and_drag());

        if resp.double_clicked() {
            *value = default_val;
            changed = true;
        } else if resp.dragged() {
            let delta = -resp.drag_delta().y * 0.0065;
            let fine = if ui.input(|i| i.modifiers.shift) { 0.2 } else { 1.0 };
            let span = (max - min).max(1e-5);
            *value = (*value + delta * fine * span).clamp(min, max);
            changed = true;
        }

        let center = rect.center();
        let radius = diameter * 0.44;
        let norm = if (max - min).abs() < 1e-6 {
            0.0
        } else {
            ((*value - min) / (max - min)).clamp(0.0, 1.0)
        };

        let painter = ui.painter();
        painter.circle_filled(center, radius, BG_DEEP);
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(
                if resp.hovered() || resp.dragged() { 1.6_f32 } else { 1.0_f32 },
                if resp.dragged() { accent } else { BORDER_SUBTLE },
            ),
        );

        // Sweep arc from 135° to 405° (270° span)
        let start_ang = 0.75 * PI;
        let sweep = 1.5 * PI;
        let steps = 24usize;
        let active_steps = ((norm * steps as f32).ceil() as usize).min(steps);
        for s in 0..steps {
            let a0 = start_ang + sweep * (s as f32 / steps as f32);
            let a1 = start_ang + sweep * ((s + 1) as f32 / steps as f32);
            let r_arc = radius + 3.2;
            let p0 = Pos2::new(center.x + r_arc * a0.cos(), center.y + r_arc * a0.sin());
            let p1 = Pos2::new(center.x + r_arc * a1.cos(), center.y + r_arc * a1.sin());
            let col = if s < active_steps {
                accent
            } else {
                BORDER_SUBTLE
            };
            painter.line_segment([p0, p1], Stroke::new(2.3_f32, col));
        }

        // Indicator pointer
        let val_ang = start_ang + sweep * norm;
        let p_in = Pos2::new(
            center.x + (radius * 0.25) * val_ang.cos(),
            center.y + (radius * 0.25) * val_ang.sin(),
        );
        let p_out = Pos2::new(
            center.x + (radius * 0.85) * val_ang.cos(),
            center.y + (radius * 0.85) * val_ang.sin(),
        );
        painter.line_segment([p_in, p_out], Stroke::new(2.0_f32, TEXT_MAIN));

        let val_txt = if unit == "%" || (max - min) >= 50.0 {
            format!("{:.0}{}", *value, unit)
        } else {
            format!("{:.1}{}", *value, unit)
        };
        resp.on_hover_text(format!("{}: {} (Double-click to reset)", label, val_txt));

        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(label).size(10.0).color(TEXT_DIM));
            ui.label(egui::RichText::new(val_txt).size(10.0).color(TEXT_MAIN).monospace());
        });
    });
    changed
}

/// Vertical studio fader (`-60 dB .. +12 dB`) with double-click `0.0 dB` unity reset.
pub fn vertical_fader(ui: &mut Ui, gain_db: &mut f32, height: f32, accent: Color32) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(24.0, height), Sense::click_and_drag());
    let min_db = -60.0_f32;
    let max_db = 12.0_f32;

    if resp.double_clicked() {
        *gain_db = 0.0;
        resp.mark_changed();
    } else if resp.dragged() {
        let dy = -resp.drag_delta().y;
        let speed = if ui.input(|i| i.modifiers.shift) { 0.12 } else { 0.45 };
        *gain_db = (*gain_db + dy * speed).clamp(min_db, max_db);
        resp.mark_changed();
    }

    let painter = ui.painter();
    let track_x = rect.center().x;
    let top = rect.top() + 8.0;
    let bot = rect.bottom() - 8.0;
    let span = (bot - top).max(10.0);

    // Groove
    painter.line_segment(
        [Pos2::new(track_x, top), Pos2::new(track_x, bot)],
        Stroke::new(3.0_f32, BG_DEEP),
    );

    // 0 dB Unity tick mark
    let unity_norm = (0.0 - min_db) / (max_db - min_db);
    let unity_y = bot - unity_norm * span;
    painter.line_segment(
        [Pos2::new(rect.left() + 2.0, unity_y), Pos2::new(rect.right() - 2.0, unity_y)],
        Stroke::new(1.0_f32, Color32::from_rgb(90, 96, 114)),
    );

    let norm = ((*gain_db - min_db) / (max_db - min_db)).clamp(0.0, 1.0);
    let cap_y = bot - norm * span;

    painter.line_segment(
        [Pos2::new(track_x, cap_y), Pos2::new(track_x, bot)],
        Stroke::new(3.0_f32, accent),
    );

    let cap_rect = Rect::from_center_size(Pos2::new(track_x, cap_y), Vec2::new(20.0, 11.0));
    painter.rect_filled(cap_rect, 3, Color32::from_rgb(56, 61, 74));
    painter.rect_stroke(cap_rect, 3, Stroke::new(1.0_f32, TEXT_MAIN), StrokeKind::Outside);
    painter.line_segment(
        [
            Pos2::new(cap_rect.left() + 3.0, cap_y),
            Pos2::new(cap_rect.right() - 3.0, cap_y),
        ],
        Stroke::new(1.5_f32, accent),
    );

    resp
}

/// Dual-bar Stereo Peak Meter (`-60 dB .. +6 dB`) with cyan/orange/pink zones.
pub fn stereo_level_meter(ui: &mut Ui, peak_l_db: f32, peak_r_db: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2, BG_DEEP);

    let draw_bar = |x0: f32, x1: f32, db: f32| {
        let norm = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
        let segments = (height / 4.0).floor().max(1.0) as usize;
        let step = height / segments as f32;
        for segment in 0..segments {
            let fraction = segment as f32 / segments as f32;
            let y = rect.bottom() - segment as f32 * step;
            let color = if norm <= fraction {
                BORDER_SUBTLE
            } else {
                match -60.0 + fraction * 66.0 {
                    db if db >= -3.0 => STATE_ARM,
                    db if db >= -12.0 => ACCENT_FERRIC,
                    _ => ACCENT_CYAN,
                }
            };
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(x0, y - step + 1.0), Pos2::new(x1, y)),
                0,
                color,
            );
        }
    };
    let mid = rect.center().x;
    draw_bar(rect.left() + 1.0, mid - 0.5, peak_l_db);
    draw_bar(mid + 0.5, rect.right() - 1.0, peak_r_db);
}

/// Dim section caption used above control clusters (menus, strips, docks).
#[allow(dead_code)]
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(10.5)
            .strong()
            .color(TEXT_DIM),
    );
}

pub fn pill_toggle(ui: &mut Ui, text: &str, active: bool, active_color: Color32) -> Response {
    pill_toggle_width(ui, text, active, active_color, 30.0)
}

pub fn pill_toggle_width(
    ui: &mut Ui, text: &str, active: bool, active_color: Color32, min_width: f32,
) -> Response {
    // Set widget visuals instead of overriding Button fill/stroke: egui can
    // then show hover, pressed, keyboard focus and disabled states.
    ui.scope(|ui| {
        if active {
            let widgets = &mut ui.visuals_mut().widgets;
            for (visual, fill, stroke) in [
                (&mut widgets.inactive, active_color, active_color),
                (&mut widgets.hovered, active_color.lerp_to_gamma(Color32::WHITE, 0.15), TEXT_MAIN),
                (&mut widgets.active, active_color.gamma_multiply(0.82), ACCENT_CYAN),
            ] {
                visual.bg_fill = fill;
                visual.weak_bg_fill = fill;
                visual.bg_stroke = Stroke::new(1.0_f32, stroke);
                visual.fg_stroke = Stroke::new(1.0_f32, BG_DEEP);
            }
        }
        ui.add(
            egui::Button::new(egui::RichText::new(text).size(11.0).strong())
                .corner_radius(BUTTON_RADIUS)
                .min_size(Vec2::new(min_width, BUTTON_HEIGHT)),
        )
    }).inner
}

/// Shared neutral button styling for every non-toggle action.
pub trait StudioUiExt {
    fn chrome_button(&mut self, text: impl Into<egui::WidgetText>) -> Response;
    fn chrome_small_button(&mut self, text: impl Into<egui::WidgetText>) -> Response;
    fn chrome_danger_button(&mut self, text: impl Into<egui::WidgetText>) -> Response;
    fn chrome_primary_button(&mut self, text: &str) -> Response;
}

impl StudioUiExt for Ui {
    fn chrome_primary_button(&mut self, text: &str) -> Response {
        pill_toggle(self, text, true, ACCENT_FERRIC)
    }

    fn chrome_button(&mut self, text: impl Into<egui::WidgetText>) -> Response {
        self.add(
            egui::Button::new(text)
                .corner_radius(BUTTON_RADIUS)
                .min_size(Vec2::new(0.0, BUTTON_HEIGHT)),
        )
    }

    fn chrome_small_button(&mut self, text: impl Into<egui::WidgetText>) -> Response {
        self.add(
            egui::Button::new(text)
                .corner_radius(BUTTON_RADIUS)
                .min_size(Vec2::new(30.0, BUTTON_HEIGHT)),
        )
    }

    fn chrome_danger_button(&mut self, text: impl Into<egui::WidgetText>) -> Response {
        self.add(
            egui::Button::new(text)
                .fill(ACCENT_RED.gamma_multiply(0.18))
                .stroke(Stroke::new(1.0_f32, ACCENT_RED.gamma_multiply(0.85)))
                .corner_radius(BUTTON_RADIUS)
                .min_size(Vec2::new(0.0, BUTTON_HEIGHT)),
        )
    }
}

/// Shared Hardware Rig / Pads chrome: elevated inset card.
pub fn chrome_card(ui: &mut Ui, accent: Color32, add_contents: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(BG_ELEVATED)
        .stroke(Stroke::new(1.0_f32, accent))
        .corner_radius(6)
        .inner_margin(8.0)
        .show(ui, add_contents);
}

/// Colored label used on Pad A/B-style picker rows (`Pad A ->`, `In ->`, …).
pub fn chrome_row_label(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(egui::RichText::new(text).size(12.0).color(color));
}

/// Strong section title inside a chrome card (matches Hardware Rig / Pad strip headers).
pub fn chrome_section_title(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(
        egui::RichText::new(text)
            .size(11.5)
            .strong()
            .color(color),
    );
}

#[allow(dead_code)]
pub fn draw_badge(painter: &egui::Painter, pos: Pos2, text: &str, bg: Color32, fg: Color32) {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(9.5), fg);
    let r = Rect::from_min_size(pos, galley.size() + Vec2::new(8.0, 3.0));
    painter.rect_filled(r, 3, bg);
    painter.galley(pos + Vec2::new(4.0, 1.5), galley, fg);
    let _ = Align2::LEFT_TOP;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_theme_keeps_one_button_geometry_and_neutral_interaction_states() {
        let ctx = Context::default();
        apply_studio_theme(&ctx);
        let visuals = ctx.style().visuals.clone();

        assert_eq!(visuals.widgets.inactive.corner_radius, CornerRadius::same(BUTTON_RADIUS));
        assert_eq!(visuals.widgets.hovered.corner_radius, CornerRadius::same(BUTTON_RADIUS));
        assert_eq!(visuals.widgets.active.corner_radius, CornerRadius::same(BUTTON_RADIUS));
        assert_eq!(visuals.widgets.open, visuals.widgets.hovered);
        assert_eq!(visuals.widgets.hovered.bg_fill, BUTTON_HOVER);
        assert_eq!(visuals.widgets.active.bg_fill, BUTTON_ACTIVE);
        assert_eq!(BUTTON_HEIGHT, 24.0);
    }

    #[test]
    fn semantic_accents_are_distinct_and_share_the_same_text_safe_surface() {
        assert_ne!(ACCENT_FERRIC, ACCENT_TEAL);
        assert_ne!(ACCENT_TEAL, ACCENT_PURPLE);
        assert_ne!(ACCENT_PURPLE, ACCENT_AMBER);
        assert_ne!(ACCENT_BLUE, ACCENT_CYAN);
        assert_ne!(BUTTON_SURFACE, BUTTON_HOVER);
        assert_ne!(BUTTON_HOVER, BUTTON_ACTIVE);
    }
}
