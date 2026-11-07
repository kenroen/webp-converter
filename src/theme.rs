//! Colours and egui style: a dark field-equipment look with an amber accent.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

pub const BG: Color32 = Color32::from_rgb(0x12, 0x14, 0x11);
pub const PANEL: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x18);
pub const FIELD: Color32 = Color32::from_rgb(0x0c, 0x0e, 0x0b);
pub const LINE: Color32 = Color32::from_rgb(0x2c, 0x31, 0x28);
pub const LINE_STRONG: Color32 = Color32::from_rgb(0x46, 0x4d, 0x3e);
pub const INK: Color32 = Color32::from_rgb(0xe4, 0xe6, 0xdc);
pub const MUTED: Color32 = Color32::from_rgb(0x8a, 0x91, 0x80);
pub const ACCENT: Color32 = Color32::from_rgb(0xff, 0xb0, 0x1f);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x8a, 0x66, 0x1f);
pub const CYAN: Color32 = Color32::from_rgb(0x3d, 0xd6, 0xd0);
pub const GOOD: Color32 = Color32::from_rgb(0x7c, 0xd9, 0x5a);
pub const BAD: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x4a);

pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(13.0, FontFamily::Monospace)),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Monospace)),
            (TextStyle::Monospace, FontId::new(15.0, FontFamily::Monospace)),
            (TextStyle::Heading, FontId::new(20.0, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        let v = &mut style.visuals;
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.extreme_bg_color = FIELD;
        v.override_text_color = Some(INK);
        v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::ZERO;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.inactive.bg_fill = PANEL;
        v.widgets.inactive.weak_bg_fill = PANEL;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE_STRONG);
        v.widgets.hovered.bg_fill = LINE;
        v.widgets.hovered.weak_bg_fill = LINE;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
        v.widgets.active.bg_fill = LINE_STRONG;
        v.widgets.active.weak_bg_fill = LINE_STRONG;
        v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    });
}
