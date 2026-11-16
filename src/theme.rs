//! Light, rounded, Fluent-inspired look.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(0, 95, 184);
pub const RIBBON_BG: Color32 = Color32::from_rgb(249, 249, 249);
pub const CANVAS_AREA_BG: Color32 = Color32::from_rgb(230, 235, 241);
pub const STATUS_BG: Color32 = Color32::from_rgb(243, 243, 243);
pub const SELECTED_BG: Color32 = Color32::from_rgb(222, 233, 247);
pub const HOVER_BG: Color32 = Color32::from_rgb(234, 234, 234);
pub const TEXT: Color32 = Color32::from_rgb(28, 28, 28);
pub const SUBTLE_TEXT: Color32 = Color32::from_rgb(96, 96, 96);
pub const DIVIDER: Color32 = Color32::from_rgb(224, 224, 224);

pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    let mut v = Visuals::light();
    v.panel_fill = RIBBON_BG;
    v.window_fill = Color32::WHITE;
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, DIVIDER);
    v.selection.bg_fill = SELECTED_BG;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.override_text_color = Some(TEXT);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(4);
    }
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = Color32::from_rgb(240, 240, 240);
    v.widgets.hovered.weak_bg_fill = HOVER_BG;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.weak_bg_fill = Color32::from_rgb(222, 222, 222);
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.open.weak_bg_fill = HOVER_BG;
    v.slider_trailing_fill = true;
    ctx.set_visuals_of(egui::Theme::Light, v);
    ctx.set_theme(egui::ThemePreference::Light);

    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(4.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
    });
}
