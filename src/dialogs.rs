//! In-window dialogs: Resize and Skew, Image Properties.

use egui::{RichText, Ui};
use egui_phosphor::regular as ic;

use crate::app::DaubApp;
use crate::raster::MAX_DIM;
use crate::theme;

pub enum Dialog {
    Resize(ResizeDialog),
    Properties(PropertiesDialog),
}

pub struct ResizeDialog {
    /// Size of what is being resized (the selection or the image).
    pub orig: (u32, u32),
    pub on_selection: bool,
    pub by_percent: bool,
    pub keep_aspect: bool,
    /// Horizontal/vertical: percent or pixels depending on `by_percent`.
    pub horiz: f32,
    pub vert: f32,
    pub skew_h: f32,
    pub skew_v: f32,
}

impl ResizeDialog {
    pub fn new(orig: (u32, u32), on_selection: bool) -> Self {
        Self {
            orig,
            on_selection,
            by_percent: true,
            keep_aspect: true,
            horiz: 100.0,
            vert: 100.0,
            skew_h: 0.0,
            skew_v: 0.0,
        }
    }

    /// Target size in pixels.
    pub fn target(&self) -> (u32, u32) {
        let (w, h) = (self.orig.0 as f32, self.orig.1 as f32);
        let (tw, th) =
            if self.by_percent { (w * self.horiz / 100.0, h * self.vert / 100.0) } else { (self.horiz, self.vert) };
        (tw.round().clamp(1.0, MAX_DIM as f32) as u32, th.round().clamp(1.0, MAX_DIM as f32) as u32)
    }

    fn set_mode(&mut self, by_percent: bool) {
        if by_percent == self.by_percent {
            return;
        }
        let (tw, th) = self.target();
        self.by_percent = by_percent;
        if by_percent {
            self.horiz = tw as f32 / self.orig.0 as f32 * 100.0;
            self.vert = th as f32 / self.orig.1 as f32 * 100.0;
        } else {
            self.horiz = tw as f32;
            self.vert = th as f32;
        }
    }

    /// Keep the other axis in proportion after `changed_horiz` was edited.
    fn sync_aspect(&mut self, changed_horiz: bool) {
        if !self.keep_aspect {
            return;
        }
        let (w, h) = (self.orig.0 as f32, self.orig.1 as f32);
        match (self.by_percent, changed_horiz) {
            (true, true) => self.vert = self.horiz,
            (true, false) => self.horiz = self.vert,
            (false, true) => self.vert = (self.horiz * h / w).round().max(1.0),
            (false, false) => self.horiz = (self.vert * w / h).round().max(1.0),
        }
    }
}

pub struct PropertiesDialog {
    pub w: u32,
    pub h: u32,
    pub transparent: bool,
}

enum Outcome {
    Open,
    Cancel,
    Ok,
}

fn heading(ui: &mut Ui, icon: &str, text: &str) {
    ui.label(RichText::new(format!("{icon}  {text}")).size(16.0).strong());
    ui.add_space(6.0);
}

fn buttons(ui: &mut Ui) -> Outcome {
    let mut out = Outcome::Open;
    ui.add_space(10.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("  Cancel  ").clicked() {
            out = Outcome::Cancel;
        }
        let ok = egui::Button::new(RichText::new("  OK  ").color(egui::Color32::WHITE)).fill(theme::ACCENT);
        if ui.add(ok).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            out = Outcome::Ok;
        }
    });
    out
}

impl DaubApp {
    pub fn open_resize_dialog(&mut self) {
        // A shape or text box is baked in first; a selection stays as the target.
        if self.pending_selection().is_none() {
            self.commit_pending();
        }
        let (orig, on_sel) = match self.pending_selection() {
            Some(s) => ((s.rect.width(), s.rect.height()), true),
            None => ((self.doc.width(), self.doc.height()), false),
        };
        self.dialog = Some(Dialog::Resize(ResizeDialog::new(orig, on_sel)));
    }

    pub fn open_properties_dialog(&mut self) {
        self.commit_pending();
        self.dialog = Some(Dialog::Properties(PropertiesDialog {
            w: self.doc.width(),
            h: self.doc.height(),
            transparent: self.transparent_bg,
        }));
    }

    pub fn dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialog else { return };
        let mut outcome = Outcome::Open;
        let modal = egui::Modal::new(egui::Id::new("daub-dialog")).show(ctx, |ui| {
            ui.set_width(300.0);
            // Give number fields a visible box so they read as editable.
            let w = &mut ui.visuals_mut().widgets.inactive;
            w.bg_stroke = egui::Stroke::new(1.0, theme::DIVIDER);
            w.bg_fill = egui::Color32::WHITE;
            match dialog {
                Dialog::Resize(d) => outcome = resize_ui(ui, d),
                Dialog::Properties(d) => outcome = properties_ui(ui, d),
            }
        });
        if modal.should_close() && matches!(outcome, Outcome::Open) {
            outcome = Outcome::Cancel;
        }
        match outcome {
            Outcome::Open => {}
            Outcome::Cancel => self.dialog = None,
            Outcome::Ok => match self.dialog.take() {
                Some(Dialog::Resize(d)) => {
                    let (w, h) = d.target();
                    if (w, h) != d.orig || d.skew_h != 0.0 || d.skew_v != 0.0 {
                        self.resize_and_skew(w, h, d.skew_h, d.skew_v);
                    }
                }
                Some(Dialog::Properties(d)) => {
                    self.transparent_bg = d.transparent;
                    self.set_canvas_size(d.w, d.h);
                }
                None => {}
            },
        }
    }
}

fn resize_ui(ui: &mut Ui, d: &mut ResizeDialog) -> Outcome {
    heading(ui, ic::ARROWS_OUT, "Resize and Skew");
    ui.label(
        RichText::new(if d.on_selection { "Applies to the selection" } else { "Applies to the whole image" })
            .size(12.0)
            .color(theme::SUBTLE_TEXT),
    );
    ui.add_space(8.0);
    ui.label(RichText::new("Resize").strong());
    ui.horizontal(|ui| {
        ui.label("By:");
        let mut pct = d.by_percent;
        ui.radio_value(&mut pct, true, "Percentage");
        ui.radio_value(&mut pct, false, "Pixels");
        d.set_mode(pct);
    });
    let (suffix, max) = if d.by_percent { ("%", 1000.0) } else { (" px", MAX_DIM as f32) };
    egui::Grid::new("resize-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label(format!("{}  Horizontal", ic::ARROWS_LEFT_RIGHT));
        if ui.add(egui::DragValue::new(&mut d.horiz).range(1.0..=max).suffix(suffix).max_decimals(0)).changed() {
            d.sync_aspect(true);
        }
        ui.end_row();
        ui.label(format!("{}  Vertical", ic::ARROWS_DOWN_UP));
        if ui.add(egui::DragValue::new(&mut d.vert).range(1.0..=max).suffix(suffix).max_decimals(0)).changed() {
            d.sync_aspect(false);
        }
        ui.end_row();
    });
    if ui.checkbox(&mut d.keep_aspect, "Maintain aspect ratio").changed() {
        d.sync_aspect(true);
    }
    let (tw, th) = d.target();
    ui.label(
        RichText::new(format!("{} × {}px  {}  {tw} × {th}px", d.orig.0, d.orig.1, ic::ARROW_RIGHT))
            .size(12.0)
            .color(theme::SUBTLE_TEXT),
    );
    ui.add_space(8.0);
    ui.label(RichText::new("Skew (degrees)").strong());
    egui::Grid::new("skew-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label(format!("{}  Horizontal", ic::PARALLELOGRAM));
        ui.add(egui::DragValue::new(&mut d.skew_h).range(-89.0..=89.0).suffix("°").max_decimals(0));
        ui.end_row();
        ui.label(format!("{}  Vertical", ic::PARALLELOGRAM));
        ui.add(egui::DragValue::new(&mut d.skew_v).range(-89.0..=89.0).suffix("°").max_decimals(0));
        ui.end_row();
    });
    buttons(ui)
}

fn properties_ui(ui: &mut Ui, d: &mut PropertiesDialog) -> Outcome {
    heading(ui, ic::IMAGE, "Image Properties");
    egui::Grid::new("props-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("Width");
        ui.add(egui::DragValue::new(&mut d.w).range(1..=MAX_DIM).suffix(" px"));
        ui.end_row();
        ui.label("Height");
        ui.add(egui::DragValue::new(&mut d.h).range(1..=MAX_DIM).suffix(" px"));
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.checkbox(&mut d.transparent, "Transparent background")
        .on_hover_text("Eraser, Delete and new canvas space leave transparent pixels");
    ui.label(
        RichText::new("Changing the size crops or extends the canvas from the bottom-right.")
            .size(12.0)
            .color(theme::SUBTLE_TEXT),
    );
    buttons(ui)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_dialog_math() {
        let mut d = ResizeDialog::new((200, 100), false);
        d.horiz = 50.0;
        d.sync_aspect(true);
        assert_eq!(d.target(), (100, 50));
        d.set_mode(false);
        assert_eq!((d.horiz, d.vert), (100.0, 50.0));
        d.horiz = 300.0;
        d.sync_aspect(true);
        assert_eq!(d.target(), (300, 150));
        d.keep_aspect = false;
        d.vert = 10.0;
        assert_eq!(d.target(), (300, 10));
        d.set_mode(true);
        assert_eq!((d.horiz, d.vert), (150.0, 10.0));
    }
}
