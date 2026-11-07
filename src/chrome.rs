//! The borderless window's chrome: a HUD-style frame with corner brackets, a custom title bar
//! that moves the window, and its Minimize / Close caption buttons.

use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::theme;

/// Height of the custom title bar.
pub const TITLE_H: f32 = 40.0;
/// Space the decorative frame takes on each side; the content sits inside it.
pub const FRAME_INSET: f32 = 8.0;

/// Paints the decorative HUD frame around the borderless window.
pub fn window_frame(ctx: &egui::Context) {
    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
    let r = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("window-frame"),
    ));
    let accent = if focused {
        theme::ACCENT
    } else {
        theme::ACCENT_DIM
    };

    painter.rect_stroke(
        r,
        CornerRadius::ZERO,
        Stroke::new(1.0, theme::LINE_STRONG),
        StrokeKind::Inside,
    );
    let inner = r.shrink(4.0);
    painter.rect_stroke(
        inner,
        CornerRadius::ZERO,
        Stroke::new(1.0, theme::LINE),
        StrokeKind::Inside,
    );

    let len = 22.0;
    let bracket = Stroke::new(2.0, accent);
    let b = r.shrink(2.0);
    for (corner, dx, dy) in [
        (b.left_top(), 1.0, 1.0),
        (b.right_top(), -1.0, 1.0),
        (b.left_bottom(), 1.0, -1.0),
        (b.right_bottom(), -1.0, -1.0),
    ] {
        painter.line_segment([corner, corner + vec2(len * dx, 0.0)], bracket);
        painter.line_segment([corner, corner + vec2(0.0, len * dy)], bracket);
        let rivet = corner + vec2(7.0 * dx, 7.0 * dy);
        painter.rect_filled(
            Rect::from_center_size(rivet, vec2(3.0, 3.0)),
            CornerRadius::ZERO,
            accent.gamma_multiply(0.7),
        );
    }

    let tick = Stroke::new(1.0, theme::LINE_STRONG);
    let ticks = |a: Pos2, b: Pos2, n: usize, out: egui::Vec2| {
        for k in 1..n {
            let t = k as f32 / n as f32;
            let p = a.lerp(b, t);
            let long = k % 4 == 0;
            painter.line_segment([p, p + out * if long { 5.0 } else { 3.0 }], tick);
        }
    };
    let bottom_y = r.bottom() - 1.0;
    ticks(
        pos2(r.left() + 40.0, bottom_y),
        pos2(r.right() - 40.0, bottom_y),
        32,
        vec2(0.0, -1.0),
    );
    ticks(
        pos2(r.left() + 1.0, r.top() + 50.0),
        pos2(r.left() + 1.0, r.bottom() - 40.0),
        16,
        vec2(1.0, 0.0),
    );
    ticks(
        pos2(r.right() - 1.0, r.top() + 50.0),
        pos2(r.right() - 1.0, r.bottom() - 40.0),
        16,
        vec2(-1.0, 0.0),
    );

    let top = Rect::from_min_max(
        pos2(r.left() + len + 8.0, r.top() + 1.0),
        pos2(r.right() - len - 8.0, r.top() + 3.0),
    );
    painter.rect_filled(top, CornerRadius::ZERO, accent.gamma_multiply(0.35));
}

/// Dragging the title bar's empty space moves the window.
pub fn title_bar_drag(ui: &mut Ui) {
    let rect = ui.max_rect();
    let drag_rect = Rect::from_min_max(rect.min, pos2(rect.max.x - 90.0, rect.max.y));
    let resp = ui.interact(drag_rect, ui.id().with("title-bar"), Sense::click_and_drag());
    if (resp.hovered() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)))
        || resp.drag_started_by(egui::PointerButton::Primary)
    {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowButton {
    Minimize,
    Close,
}

pub fn window_button(ui: &mut Ui, button: WindowButton, height: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(44.0, height), Sense::click());
    let hovered = resp.hovered();
    let (fill, ink) = match (button, hovered) {
        (WindowButton::Close, true) => (Color32::from_rgb(0xc4, 0x2b, 0x1c), Color32::WHITE),
        (_, true) => (theme::LINE, theme::INK),
        (_, false) => (Color32::TRANSPARENT, theme::MUTED),
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, fill);
    let c = rect.center();
    let stroke = Stroke::new(1.4, ink);
    let s = 5.5;
    let label = match button {
        WindowButton::Minimize => {
            p.hline(c.x - s..=c.x + s, c.y, stroke);
            "Minimize"
        }
        WindowButton::Close => {
            p.line_segment([c + vec2(-s, -s), c + vec2(s, s)], stroke);
            p.line_segment([c + vec2(-s, s), c + vec2(s, -s)], stroke);
            "Close"
        }
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp.on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
