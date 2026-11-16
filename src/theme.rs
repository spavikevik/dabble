//! Rounded, Fluent-inspired look in light and dark variants.

use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, Visuals};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemePref {
    System,
    Light,
    Dark,
}

struct Palette {
    accent: Color32,
    ribbon_bg: Color32,
    canvas_area_bg: Color32,
    status_bg: Color32,
    selected_bg: Color32,
    hover_bg: Color32,
    pressed_bg: Color32,
    surface: Color32,
    text: Color32,
    subtle_text: Color32,
    divider: Color32,
}

const LIGHT: Palette = Palette {
    accent: Color32::from_rgb(0, 95, 184),
    ribbon_bg: Color32::from_rgb(249, 249, 249),
    canvas_area_bg: Color32::from_rgb(230, 235, 241),
    status_bg: Color32::from_rgb(243, 243, 243),
    selected_bg: Color32::from_rgb(222, 233, 247),
    hover_bg: Color32::from_rgb(234, 234, 234),
    pressed_bg: Color32::from_rgb(222, 222, 222),
    surface: Color32::WHITE,
    text: Color32::from_rgb(28, 28, 28),
    subtle_text: Color32::from_rgb(96, 96, 96),
    divider: Color32::from_rgb(224, 224, 224),
};

const DARK: Palette = Palette {
    accent: Color32::from_rgb(96, 205, 255),
    ribbon_bg: Color32::from_rgb(39, 39, 39),
    canvas_area_bg: Color32::from_rgb(25, 27, 31),
    status_bg: Color32::from_rgb(32, 32, 32),
    selected_bg: Color32::from_rgb(52, 65, 82),
    hover_bg: Color32::from_rgb(56, 56, 56),
    pressed_bg: Color32::from_rgb(48, 48, 48),
    surface: Color32::from_rgb(44, 44, 44),
    text: Color32::from_rgb(240, 240, 240),
    subtle_text: Color32::from_rgb(170, 170, 170),
    divider: Color32::from_rgb(64, 64, 64),
};

static IS_DARK: AtomicBool = AtomicBool::new(false);

fn pal() -> &'static Palette {
    if is_dark() { &DARK } else { &LIGHT }
}

pub fn is_dark() -> bool {
    IS_DARK.load(Ordering::Relaxed)
}

pub fn accent() -> Color32 {
    pal().accent
}
/// Text/icon color drawn on top of an `accent()` fill.
pub fn on_accent() -> Color32 {
    if is_dark() { Color32::BLACK } else { Color32::WHITE }
}
pub fn ribbon_bg() -> Color32 {
    pal().ribbon_bg
}
pub fn canvas_area_bg() -> Color32 {
    pal().canvas_area_bg
}
pub fn status_bg() -> Color32 {
    pal().status_bg
}
pub fn selected_bg() -> Color32 {
    pal().selected_bg
}
pub fn hover_bg() -> Color32 {
    pal().hover_bg
}
pub fn pressed_bg() -> Color32 {
    pal().pressed_bg
}
/// Cards, dialogs, popups.
pub fn surface() -> Color32 {
    pal().surface
}
pub fn text() -> Color32 {
    pal().text
}
pub fn subtle_text() -> Color32 {
    pal().subtle_text
}
pub fn divider() -> Color32 {
    pal().divider
}
pub fn disabled_text() -> Color32 {
    if is_dark() { Color32::from_gray(100) } else { Color32::from_gray(170) }
}

/// Load the icon font and set spacing. Call once.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(4.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.menu_spacing = 2.0;
        s.animation_time = 0.12;
        s.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
    });
    set_dark(ctx, false);
}

/// Switch between the light and dark palettes.
pub fn set_dark(ctx: &egui::Context, dark: bool) {
    IS_DARK.store(dark, Ordering::Relaxed);
    let p = pal();
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.ribbon_bg;
    v.window_fill = p.surface;
    v.extreme_bg_color = if dark { Color32::from_rgb(30, 30, 30) } else { Color32::WHITE };
    v.faint_bg_color = p.hover_bg;
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, p.divider);
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 110 } else { 40 }) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(if dark { 90 } else { 30 }) };
    v.selection.bg_fill = p.selected_bg;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.hyperlink_color = p.accent;
    v.override_text_color = Some(p.text);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(4);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.divider);
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = p.pressed_bg;
    v.widgets.hovered.weak_bg_fill = p.hover_bg;
    v.widgets.hovered.bg_fill = p.hover_bg;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.weak_bg_fill = p.pressed_bg;
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.open.weak_bg_fill = p.hover_bg;
    v.slider_trailing_fill = true;
    let theme = if dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_visuals_of(theme, v);
    ctx.set_theme(theme);
}
