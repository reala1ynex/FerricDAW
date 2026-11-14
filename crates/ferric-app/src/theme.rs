use egui::{
    epaint::Shadow, Align2, Color32, Context, CornerRadius, FontId, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, Ui, Vec2, Visuals,
};
use std::f32::consts::PI;

pub const BG_DEEP: Color32 = Color32::from_rgb(15, 16, 20);
pub const BG_PANEL: Color32 = Color32::from_rgb(22, 24, 30);
pub const BG_ELEVATED: Color32 = Color32::from_rgb(30, 33, 41);
pub const BG_TRACK_EVEN: Color32 = Color32::from_rgb(20, 22, 28);
pub const BG_TRACK_ODD: Color32 = Color32::from_rgb(24, 26, 33);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(46, 50, 62);
pub const ACCENT_FERRIC: Color32 = Color32::from_rgb(232, 114, 62);
pub const ACCENT_TEAL: Color32 = Color32::from_rgb(66, 196, 176);
pub const ACCENT_AMBER: Color32 = Color32::from_rgb(240, 182, 62);
pub const ACCENT_RED: Color32 = Color32::from_rgb(232, 74, 86);
pub const TEXT_MAIN: Color32 = Color32::from_rgb(232, 235, 242);
pub const TEXT_DIM: Color32 = Color32::from_rgb(142, 148, 164);

pub fn apply_studio_theme(ctx: &Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_ELEVATED;
    visuals.extreme_bg_color = BG_DEEP;
    visuals.faint_bg_color = BG_TRACK_ODD;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(232, 114, 62, 90);
    visuals.selection.stroke = Stroke::new(1.5_f32, ACCENT_FERRIC);
    visuals.widgets.noninteractive.bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_MAIN);
    visuals.widgets.inactive.bg_fill = BG_ELEVATED;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_MAIN);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(5);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 46, 58);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_FERRIC);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(5);
    visuals.widgets.active.bg_fill = Color32::from_rgb(56, 40, 34);
    visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, ACCENT_FERRIC);
    visuals.widgets.active.corner_radius = CornerRadius::same(5);
    visuals.window_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(150),
    };
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(6.0, 5.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
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
        painter.circle_filled(center, radius, Color32::from_rgb(18, 20, 26));
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
                Color32::from_rgb(38, 42, 52)
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

/// Dual-bar Stereo Peak Meter (`-60 dB .. +6 dB`) with green/amber/crimson zones.
pub fn stereo_level_meter(ui: &mut Ui, peak_l_db: f32, peak_r_db: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2, BG_DEEP);

    let draw_bar = |x0: f32, x1: f32, db: f32| {
        let norm = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
        if norm <= 0.005 {
            return;
        }
        let h = rect.height() * norm;
        let bar_rect = Rect::from_min_max(Pos2::new(x0, rect.bottom() - h), Pos2::new(x1, rect.bottom()));
        let col = if db > -0.5 {
            ACCENT_RED
        } else if db > -6.0 {
            ACCENT_AMBER
        } else {
            ACCENT_TEAL
        };
        painter.rect_filled(bar_rect, 1, col);
    };

    let mid = rect.center().x;
    draw_bar(rect.left() + 1.0, mid - 0.5, peak_l_db);
    draw_bar(mid + 0.5, rect.right() - 1.0, peak_r_db);
}

pub fn pill_toggle(ui: &mut Ui, text: &str, active: bool, active_color: Color32) -> Response {
    let fill = if active {
        active_color
    } else {
        Color32::from_rgb(34, 37, 46)
    };
    let fg = if active {
        Color32::from_rgb(14, 15, 18)
    } else {
        TEXT_MAIN
    };
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(11.0).strong().color(fg))
            .fill(fill)
            .min_size(Vec2::new(22.0, 20.0)),
    )
}

#[allow(dead_code)]
pub fn draw_badge(painter: &egui::Painter, pos: Pos2, text: &str, bg: Color32, fg: Color32) {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(9.5), fg);
    let r = Rect::from_min_size(pos, galley.size() + Vec2::new(8.0, 3.0));
    painter.rect_filled(r, 3, bg);
    painter.galley(pos + Vec2::new(4.0, 1.5), galley, fg);
    let _ = Align2::LEFT_TOP;
}
