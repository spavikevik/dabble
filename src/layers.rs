//! Layers: the layer stack, layer operations, compositing, and the layers panel.
//!
//! The active layer's pixels live in `DabbleApp::doc` so every tool keeps working
//! on a plain `Pixmap`. While a layer is active its slot in `layers` holds a
//! 1×1 placeholder. `with_layers` swaps the real pixels back in when an
//! operation needs the whole stack.

use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use egui_phosphor::regular as ic;
use tiny_skia::{Pixmap, PixmapPaint, Transform};

use crate::app::DabbleApp;
use crate::history::{self, Change, History};
use crate::{raster, theme};

#[derive(Clone)]
pub struct Layer {
    pub pm: Pixmap,
    pub name: String,
    pub visible: bool,
    /// 0..=1
    pub opacity: f32,
}

impl Layer {
    pub fn new(name: String, pm: Pixmap) -> Self {
        Self { pm, name, visible: true, opacity: 1.0 }
    }
}

pub fn placeholder() -> Pixmap {
    Pixmap::new(1, 1).unwrap()
}

impl DabbleApp {
    /// Run `f` with every layer's real pixels in place. `f` may change the
    /// stack and the active index; textures are refreshed afterwards.
    pub fn with_layers<R>(&mut self, f: impl FnOnce(&mut Vec<Layer>, &mut usize, &mut History) -> R) -> R {
        std::mem::swap(&mut self.doc, &mut self.layers[self.active].pm);
        let mut active = self.active;
        let r = f(&mut self.layers, &mut active, &mut self.history);
        self.active = active.min(self.layers.len() - 1);
        std::mem::swap(&mut self.doc, &mut self.layers[self.active].pm);
        self.layers_dirty = true;
        self.mark_changed();
        r
    }

    /// Replace every layer with `f(index, pixels, bg)` as one undo step, where
    /// `bg` is what new space should hold (the canvas color for the bottom
    /// layer, transparent above it). Returns false if `f` failed for any layer.
    pub fn map_layers(&mut self, f: impl Fn(usize, &Pixmap, Color32) -> Option<Pixmap>) -> bool {
        self.commit_pending();
        let bg = self.canvas_bg();
        self.with_layers(|ls, act, h| {
            let mut after = Vec::with_capacity(ls.len());
            for (i, l) in ls.iter().enumerate() {
                after.push(f(i, &l.pm, if i == 0 { bg } else { Color32::TRANSPARENT })?);
            }
            let before: Vec<Pixmap> =
                ls.iter_mut().zip(&after).map(|(l, p)| std::mem::replace(&mut l.pm, p.clone())).collect();
            h.push(vec![Change::Replace { before, after }], *act, *act);
            Some(())
        })
        .is_some()
    }

    /// Composite all visible layers.
    pub fn flatten(&self) -> Pixmap {
        let l = &self.layers[self.active];
        if self.layers.len() == 1 && l.visible && l.opacity >= 1.0 {
            return self.doc.clone();
        }
        let mut out = Pixmap::new(self.doc.width(), self.doc.height()).unwrap();
        for (i, l) in self.layers.iter().enumerate() {
            if !l.visible {
                continue;
            }
            let pm = if i == self.active { &self.doc } else { &l.pm };
            let paint = PixmapPaint { opacity: l.opacity, ..Default::default() };
            out.draw_pixmap(0, 0, pm.as_ref(), &paint, Transform::identity(), None);
        }
        out
    }

    pub fn select_layer(&mut self, i: usize) {
        if i == self.active || i >= self.layers.len() {
            return;
        }
        self.commit_pending();
        self.with_layers(|_, act, _| *act = i);
    }

    pub fn add_layer(&mut self) {
        self.commit_pending();
        self.layer_counter += 1;
        let name = format!("Layer {}", self.layer_counter);
        let (w, h) = (self.doc.width(), self.doc.height());
        self.with_layers(|ls, act, hist| {
            let layer = Layer::new(name, Pixmap::new(w, h).unwrap());
            let at = *act + 1;
            ls.insert(at, layer.clone());
            hist.push(vec![Change::Insert { index: at, layer }], *act, at);
            *act = at;
        });
    }

    pub fn duplicate_layer(&mut self) {
        self.commit_pending();
        self.with_layers(|ls, act, hist| {
            let mut layer = ls[*act].clone();
            layer.name = format!("{} copy", layer.name);
            let at = *act + 1;
            ls.insert(at, layer.clone());
            hist.push(vec![Change::Insert { index: at, layer }], *act, at);
            *act = at;
        });
    }

    pub fn delete_layer(&mut self) {
        if self.layers.len() < 2 {
            return;
        }
        self.commit_pending();
        self.with_layers(|ls, act, hist| {
            let i = *act;
            let layer = ls.remove(i);
            let next = i.saturating_sub(1);
            hist.push(vec![Change::Remove { index: i, layer }], i, next);
            *act = next;
        });
    }

    /// Move the active layer up (`dir` = 1) or down (-1) the stack.
    pub fn move_layer(&mut self, dir: i32) {
        let to = self.active as i32 + dir;
        if to < 0 || to >= self.layers.len() as i32 {
            return;
        }
        self.commit_pending();
        let to = to as usize;
        self.with_layers(|ls, act, hist| {
            let from = *act;
            let l = ls.remove(from);
            ls.insert(to, l);
            hist.push(vec![Change::Move { from, to }], from, to);
            *act = to;
        });
    }

    /// Merge the active layer into the one below it.
    pub fn merge_down(&mut self) {
        if self.active == 0 {
            return;
        }
        self.commit_pending();
        self.with_layers(|ls, act, hist| {
            let upper = *act;
            let lower = upper - 1;
            let before = ls[lower].pm.clone();
            if ls[upper].visible {
                let paint = PixmapPaint { opacity: ls[upper].opacity, ..Default::default() };
                let top = ls[upper].pm.clone();
                ls[lower].pm.draw_pixmap(0, 0, top.as_ref(), &paint, Transform::identity(), None);
            }
            let mut changes: Vec<Change> = history::pixels(lower, &before, &ls[lower].pm).into_iter().collect();
            let layer = ls.remove(upper);
            changes.push(Change::Remove { index: upper, layer });
            hist.push(changes, upper, lower);
            *act = lower;
        });
    }

    /// Upload textures for the inactive layers after the stack changed.
    pub fn sync_layer_textures(&mut self, ctx: &egui::Context) {
        if !self.layers_dirty {
            return;
        }
        let opts = crate::canvas::canvas_texture_options();
        self.layer_texs.resize_with(self.layers.len(), || None);
        for (i, l) in self.layers.iter().enumerate() {
            if i == self.active {
                continue;
            }
            let img = raster::to_color_image(&l.pm);
            match &mut self.layer_texs[i] {
                Some(t) => t.set(img, opts),
                slot => *slot = Some(ctx.load_texture(format!("layer-{i}"), img, opts)),
            }
        }
        self.layers_dirty = false;
    }

    /// Texture showing layer `i`, if uploaded.
    pub fn layer_texture(&self, i: usize) -> Option<egui::TextureId> {
        if i == self.active { self.tex.as_ref().map(|t| t.id()) } else { self.layer_texs.get(i)?.as_ref().map(|t| t.id()) }
    }

    pub fn layers_panel(&mut self, ui: &mut Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}  Layers", ic::STACK)).strong().size(14.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(ic::X).on_hover_text("Hide layers").clicked() {
                    self.show_layers = false;
                }
            });
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let n = self.layers.len();
            let a = self.active;
            let b = |ui: &mut Ui, enabled: bool, icon: &str, tip: &str| {
                ui.add_enabled(enabled, egui::Button::new(icon).min_size(vec2(28.0, 24.0))).on_hover_text(tip).clicked()
            };
            if b(ui, true, ic::PLUS, "New layer") {
                self.add_layer();
            }
            if b(ui, true, ic::COPY, "Duplicate layer") {
                self.duplicate_layer();
            }
            if b(ui, n > 1, ic::TRASH, "Delete layer") {
                self.delete_layer();
            }
            if b(ui, a + 1 < n, ic::ARROW_UP, "Move layer up") {
                self.move_layer(1);
            }
            if b(ui, a > 0, ic::ARROW_DOWN, "Move layer down") {
                self.move_layer(-1);
            }
            if b(ui, a > 0, ic::ARROW_LINE_DOWN, "Merge down") {
                self.merge_down();
            }
        });
        ui.add_space(6.0);

        let aspect = self.doc.height() as f32 / self.doc.width() as f32;
        let thumb = if aspect > 1.0 { vec2(40.0 / aspect, 40.0) } else { vec2(56.0, (56.0 * aspect).max(8.0)) };
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height() - 60.0).show(ui, |ui| {
            for i in (0..self.layers.len()).rev() {
                let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
                let selected = i == self.active;
                let hover = ui.ctx().animate_bool(resp.id.with("hover"), resp.hovered());
                let bg = if selected { theme::selected_bg() } else { theme::hover_bg().gamma_multiply(hover) };
                ui.painter().rect_filled(row, 6.0, bg);
                if selected {
                    let bar = Rect::from_min_size(row.left_center() - vec2(0.0, 9.0), vec2(3.0, 18.0));
                    ui.painter().rect_filled(bar, 1.5, theme::accent());
                }
                // Thumbnail on a checkerboard.
                let tr = Rect::from_center_size(row.left_center() + vec2(12.0 + 28.0, 0.0), thumb);
                ui.painter().image(
                    self.checker.id(),
                    tr,
                    Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(tr.width() / 6.0, tr.height() / 6.0)),
                    Color32::WHITE,
                );
                if let Some(t) = self.layer_texture(i) {
                    ui.painter().image(t, tr, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                ui.painter().rect_stroke(tr, 0.0, Stroke::new(1.0, theme::divider()), StrokeKind::Outside);

                // Visibility toggle.
                let eye = Rect::from_center_size(row.right_center() - vec2(18.0, 0.0), vec2(24.0, 24.0));
                let eye_resp = ui.interact(eye, resp.id.with("eye"), Sense::click());
                let visible = self.layers[i].visible;
                let eye_color = if eye_resp.hovered() { theme::text() } else { theme::subtle_text() };
                ui.painter().text(
                    eye.center(),
                    egui::Align2::CENTER_CENTER,
                    if visible { ic::EYE } else { ic::EYE_SLASH },
                    egui::FontId::proportional(16.0),
                    eye_color,
                );
                if eye_resp.on_hover_text(if visible { "Hide layer" } else { "Show layer" }).clicked() {
                    self.layers[i].visible = !visible;
                    self.mark_changed();
                }

                // Name, or a rename box.
                let name_rect = Rect::from_min_max(
                    egui::pos2(tr.right() + 12.0, row.top() + 8.0),
                    egui::pos2(eye.left() - 4.0, row.bottom() - 8.0),
                );
                let renaming = matches!(&self.renaming, Some((j, _)) if *j == i);
                if renaming {
                    let mut done = false;
                    if let Some((_, buf)) = &mut self.renaming {
                        let te = ui.put(name_rect, egui::TextEdit::singleline(buf));
                        te.request_focus();
                        done = te.lost_focus();
                    }
                    if done
                        && let Some((j, buf)) = self.renaming.take()
                        && !buf.trim().is_empty()
                    {
                        self.layers[j].name = buf.trim().to_string();
                    }
                } else {
                    let l = &self.layers[i];
                    let color = if l.visible { theme::text() } else { theme::subtle_text() };
                    ui.painter().text(
                        name_rect.left_center() - vec2(0.0, 7.0),
                        egui::Align2::LEFT_CENTER,
                        &l.name,
                        egui::FontId::proportional(13.0),
                        color,
                    );
                    ui.painter().text(
                        name_rect.left_center() + vec2(0.0, 9.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{:.0}%", l.opacity * 100.0),
                        egui::FontId::proportional(11.0),
                        theme::subtle_text(),
                    );
                }
                if resp.double_clicked() {
                    self.renaming = Some((i, self.layers[i].name.clone()));
                } else if resp.clicked() {
                    self.select_layer(i);
                }
                resp.on_hover_text("Click to select · double-click to rename");
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Opacity");
            let a = self.active;
            let mut pct = self.layers[a].opacity * 100.0;
            if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%").integer()).changed() {
                self.layers[a].opacity = pct / 100.0;
                self.mark_changed();
            }
        });
    }
}
