//! The toolbar: menus row, tool groups row, and a contextual text row.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Ui, vec2};
use egui_phosphor::regular as ic;

use crate::app::{DaubApp, Pending, PendingKind, SelectMode, Tool};
use crate::menu::Action;
use crate::shapes::{self, ShapeKind};
use crate::theme;

const PALETTE: [[u8; 3]; 20] = [
    [0x00, 0x00, 0x00],
    [0x59, 0x59, 0x59],
    [0x9b, 0x1c, 0x1c],
    [0xe5, 0x39, 0x35],
    [0xfb, 0x8c, 0x00],
    [0xfd, 0xd8, 0x35],
    [0x43, 0xa0, 0x47],
    [0x00, 0xac, 0xc1],
    [0x1e, 0x88, 0xe5],
    [0x8e, 0x24, 0xaa],
    [0xff, 0xff, 0xff],
    [0xbd, 0xbd, 0xbd],
    [0x8d, 0x6e, 0x63],
    [0xf4, 0x8f, 0xb1],
    [0xff, 0xcc, 0x80],
    [0xff, 0xf5, 0x9d],
    [0xc5, 0xe1, 0xa5],
    [0x80, 0xde, 0xea],
    [0x90, 0xca, 0xf9],
    [0xce, 0x93, 0xd8],
];

const ROTATE_ACTIONS: [Action; 5] = [
    Action::RotateRight,
    Action::RotateLeft,
    Action::Rotate180,
    Action::FlipVertical,
    Action::FlipHorizontal,
];

const FONT_SIZES: [f32; 16] = [8., 9., 10., 11., 12., 14., 16., 18., 20., 22., 24., 28., 36., 48., 72., 96.];

#[derive(PartialEq)]
struct StyleSig {
    primary: Color32,
    secondary: Color32,
    shape_size: f32,
    outline: bool,
    fill: bool,
    text: crate::text::TextStyle,
}

fn icon_button(ui: &mut Ui, icon: &str, selected: bool, size: f32, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let bg = if selected {
        theme::SELECTED_BG
    } else if resp.hovered() {
        theme::HOVER_BG
    } else {
        Color32::TRANSPARENT
    };
    let p = ui.painter();
    p.rect_filled(rect, 4.0, bg);
    if selected {
        let bar = Rect::from_center_size(rect.center_bottom() - vec2(0.0, 1.5), vec2(size * 0.4, 3.0));
        p.rect_filled(bar, 1.5, theme::ACCENT);
    }
    let color = if ui.is_enabled() { theme::TEXT } else { Color32::from_gray(170) };
    p.text(rect.center(), Align2::CENTER_CENTER, icon, FontId::proportional(size * 0.58), color);
    resp.on_hover_text(tip)
}

fn group(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        let r = ui.horizontal(|ui| {
            ui.set_min_height(62.0);
            add(ui);
        });
        let w = r.response.rect.width();
        ui.add_sized([w, 14.0], egui::Label::new(RichText::new(label).size(11.0).color(theme::SUBTLE_TEXT)));
    });
}

fn divider(ui: &mut Ui) {
    ui.add_space(6.0);
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 70.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme::DIVIDER);
    ui.add_space(6.0);
}

fn swatch(ui: &mut Ui, c: Option<Color32>, d: f32, ring: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(d, d), Sense::click());
    let p = ui.painter();
    let center = rect.center();
    if ring {
        p.circle_stroke(center, d / 2.0, Stroke::new(2.0, theme::ACCENT));
    } else if resp.hovered() {
        p.circle_stroke(center, d / 2.0, Stroke::new(1.0, Color32::from_gray(120)));
    }
    let inner = d / 2.0 - if ring { 4.0 } else { 2.0 };
    match c {
        Some(c) => {
            p.circle_filled(center, inner, c);
            p.circle_stroke(center, inner, Stroke::new(1.0, Color32::from_black_alpha(50)));
        }
        None => {
            p.circle_stroke(center, inner, Stroke::new(1.0, Color32::from_gray(200)));
        }
    }
    resp.on_hover_text(tip)
}

impl DaubApp {
    fn style_sig(&self) -> StyleSig {
        StyleSig {
            primary: self.primary,
            secondary: self.secondary,
            shape_size: self.sizes.shape,
            outline: self.shape_outline,
            fill: self.shape_fill,
            text: self.text_style.clone(),
        }
    }

    fn menu_item(&mut self, ui: &mut Ui, a: Action, enabled: bool) {
        let b = egui::Button::new(a.label()).shortcut_text(a.shortcut_text());
        if ui.add_enabled(enabled, b).clicked() {
            self.ribbon_actions.push(a);
            ui.close();
        }
    }

    pub fn ribbon_ui(&mut self, ui: &mut Ui) {
        let before = self.style_sig();
        let has_sel = self.pending_selection().is_some();
        let can_undo = self.history.can_undo() || self.pending.is_some();
        let can_redo = self.history.can_redo() && self.pending.is_none();

        // Row 1: menus + quick actions.
        ui.horizontal(|ui| {
            ui.menu_button("File", |ui| {
                self.menu_item(ui, Action::New, true);
                self.menu_item(ui, Action::Open, true);
                ui.separator();
                self.menu_item(ui, Action::Save, true);
                self.menu_item(ui, Action::SaveAs, true);
                ui.separator();
                self.menu_item(ui, Action::Quit, true);
            });
            ui.menu_button("Edit", |ui| {
                self.menu_item(ui, Action::Undo, can_undo);
                self.menu_item(ui, Action::Redo, can_redo);
                ui.separator();
                self.menu_item(ui, Action::Cut, has_sel);
                self.menu_item(ui, Action::Copy, has_sel);
                self.menu_item(ui, Action::Paste, true);
                self.menu_item(ui, Action::Delete, has_sel);
                ui.separator();
                self.menu_item(ui, Action::SelectAll, true);
            });
            ui.menu_button("Image", |ui| {
                self.menu_item(ui, Action::Crop, has_sel);
                self.menu_item(ui, Action::ResizeSkew, true);
                ui.separator();
                for a in ROTATE_ACTIONS {
                    self.menu_item(ui, a, true);
                }
                ui.separator();
                self.menu_item(ui, Action::ImageProperties, true);
                ui.checkbox(&mut self.transparent_bg, "Transparent background")
                    .on_hover_text("Eraser, Delete and new canvas space leave transparent pixels");
            });
            ui.menu_button("View", |ui| {
                self.menu_item(ui, Action::ZoomIn, true);
                self.menu_item(ui, Action::ZoomOut, true);
                self.menu_item(ui, Action::ZoomActual, true);
                self.menu_item(ui, Action::ZoomFit, true);
            });
            ui.add_space(8.0);
            if icon_button(ui, ic::FLOPPY_DISK, false, 26.0, "Save (⌘S)").clicked() {
                self.ribbon_actions.push(Action::Save);
            }
            ui.add_enabled_ui(can_undo, |ui| {
                if icon_button(ui, ic::ARROW_U_UP_LEFT, false, 26.0, "Undo (⌘Z)").clicked() {
                    self.ribbon_actions.push(Action::Undo);
                }
            });
            ui.add_enabled_ui(can_redo, |ui| {
                if icon_button(ui, ic::ARROW_U_UP_RIGHT, false, 26.0, "Redo (⇧⌘Z)").clicked() {
                    self.ribbon_actions.push(Action::Redo);
                }
            });
        });
        ui.add_space(2.0);

        // Row 2: tool groups.
        egui::ScrollArea::horizontal().id_salt("ribbon-scroll").show(ui, |ui| {
            ui.horizontal(|ui| {
                self.selection_group(ui, has_sel);
                divider(ui);
                self.image_group(ui, has_sel);
                divider(ui);
                self.tools_group(ui);
                divider(ui);
                group(ui, "Brushes", |ui| {
                    if icon_button(ui, ic::PAINT_BRUSH, self.tool == Tool::Brush, 44.0, "Brush").clicked() {
                        self.set_tool(Tool::Brush);
                    }
                });
                divider(ui);
                self.shapes_group(ui);
                divider(ui);
                self.size_group(ui);
                divider(ui);
                self.colors_group(ui);
            });
        });

        if self.tool == Tool::Text {
            ui.add_space(2.0);
            self.text_row(ui);
        }
        ui.add_space(2.0);

        if self.style_sig() != before
            && matches!(
                self.pending,
                Some(Pending { kind: PendingKind::Shape(_) | PendingKind::Text(_), .. })
            )
        {
            self.rerender_pending();
        }
    }

    fn selection_group(&mut self, ui: &mut Ui, has_sel: bool) {
        group(ui, "Selection", |ui| {
            let icon = if self.select_mode == SelectMode::Rect { ic::SELECTION } else { ic::LASSO };
            if icon_button(ui, icon, self.tool == Tool::Select, 44.0, "Select").clicked() {
                self.set_tool(Tool::Select);
            }
            ui.menu_button(ic::CARET_DOWN, |ui| {
                ui.set_min_width(190.0);
                if ui.selectable_label(self.select_mode == SelectMode::Rect, format!("{}  Rectangle", ic::SELECTION)).clicked() {
                    self.select_mode = SelectMode::Rect;
                    self.set_tool(Tool::Select);
                    ui.close();
                }
                if ui.selectable_label(self.select_mode == SelectMode::Free, format!("{}  Free-form", ic::LASSO)).clicked() {
                    self.select_mode = SelectMode::Free;
                    self.set_tool(Tool::Select);
                    ui.close();
                }
                ui.separator();
                self.menu_item(ui, Action::SelectAll, true);
                self.menu_item(ui, Action::Delete, has_sel);
                ui.separator();
                ui.checkbox(&mut self.transparent_selection, "Transparent selection")
                    .on_hover_text("When moving a selection, Color 2 pixels become see-through");
            });
        });
    }

    fn image_group(&mut self, ui: &mut Ui, has_sel: bool) {
        group(ui, "Image", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(has_sel, |ui| {
                        if icon_button(ui, ic::CROP, false, 30.0, "Crop to selection (⌘K)").clicked() {
                            self.ribbon_actions.push(Action::Crop);
                        }
                    });
                    if icon_button(ui, ic::ARROWS_OUT, false, 30.0, "Resize and skew (⌥⌘I)").clicked() {
                        self.ribbon_actions.push(Action::ResizeSkew);
                    }
                });
                ui.horizontal(|ui| {
                    let r = icon_button(ui, ic::ARROW_CLOCKWISE, false, 30.0, "Rotate and flip");
                    egui::Popup::menu(&r).show(|ui| {
                        ui.set_min_width(200.0);
                        for a in ROTATE_ACTIONS {
                            self.menu_item(ui, a, true);
                        }
                    });
                    if icon_button(ui, ic::IMAGE, false, 30.0, "Image properties (⌘E)").clicked() {
                        self.ribbon_actions.push(Action::ImageProperties);
                    }
                });
            });
        });
    }

    fn tools_group(&mut self, ui: &mut Ui) {
        group(ui, "Tools", |ui| {
            ui.vertical(|ui| egui::Grid::new("tools-grid").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                let row1 = [
                    (Tool::Pencil, ic::PENCIL_SIMPLE, "Pencil"),
                    (Tool::Fill, ic::PAINT_BUCKET, "Fill with color"),
                    (Tool::Text, ic::TEXT_T, "Text"),
                ];
                let row2 = [
                    (Tool::Eraser, ic::ERASER, "Eraser (right-drag replaces only Color 1)"),
                    (Tool::Picker, ic::EYEDROPPER, "Color picker"),
                    (Tool::Magnifier, ic::MAGNIFYING_GLASS, "Magnifier (right-click zooms out)"),
                ];
                for (r, row) in [row1, row2].into_iter().enumerate() {
                    for (t, icon, tip) in row {
                        if icon_button(ui, icon, self.tool == t, 30.0, tip).clicked() {
                            self.set_tool(t);
                        }
                    }
                    if r == 0 {
                        ui.end_row();
                    }
                }
            }));
        });
    }

    fn shapes_group(&mut self, ui: &mut Ui) {
        group(ui, "Shapes", |ui| {
            ui.vertical(|ui| egui::Grid::new("shapes-grid").spacing(vec2(1.0, 1.0)).show(ui, |ui| {
                for (i, k) in ShapeKind::ALL.into_iter().enumerate() {
                    let selected = self.tool == Tool::Shape && self.shape_kind == k;
                    if shape_button(ui, k, selected).clicked() {
                        self.commit_pending();
                        self.shape_kind = k;
                        self.set_tool(Tool::Shape);
                    }
                    if i % 6 == 5 && i + 1 < ShapeKind::ALL.len() {
                        ui.end_row();
                    }
                }
            }));
            ui.vertical(|ui| {
                ui.add_space(4.0);
                let label = |on: bool| if on { "Solid" } else { "None" };
                ui.menu_button(format!("{}  Outline: {}", ic::SQUARE, label(self.shape_outline)), |ui| {
                    if ui.selectable_label(!self.shape_outline, "No outline").clicked() {
                        self.shape_outline = false;
                        ui.close();
                    }
                    if ui.selectable_label(self.shape_outline, "Solid color").clicked() {
                        self.shape_outline = true;
                        ui.close();
                    }
                });
                ui.menu_button(format!("{}  Fill: {}", ic::PAINT_BUCKET, label(self.shape_fill)), |ui| {
                    if ui.selectable_label(!self.shape_fill, "No fill").clicked() {
                        self.shape_fill = false;
                        ui.close();
                    }
                    if ui.selectable_label(self.shape_fill, "Solid color (Color 2)").clicked() {
                        self.shape_fill = true;
                        ui.close();
                    }
                });
            });
        });
    }

    fn size_group(&mut self, ui: &mut Ui) {
        let tool = self.tool;
        group(ui, "Size", |ui| {
            let enabled = self.size_for_tool().is_some();
            ui.add_enabled_ui(enabled, |ui| {
                let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::click());
                let bg = if resp.hovered() { theme::HOVER_BG } else { Color32::TRANSPARENT };
                ui.painter().rect_filled(rect, 4.0, bg);
                let color = if enabled { theme::TEXT } else { Color32::from_gray(170) };
                for (i, t) in [1.0, 2.0, 3.0, 5.0].iter().enumerate() {
                    let y = rect.top() + 11.0 + i as f32 * 7.0;
                    ui.painter().line_segment(
                        [Pos2::new(rect.left() + 10.0, y), Pos2::new(rect.right() - 10.0, y)],
                        Stroke::new(*t, color),
                    );
                }
                let resp = resp.on_hover_text("Size");
                egui::Popup::menu(&resp).show(|ui| {
                    ui.set_min_width(180.0);
                    let presets: &[f32] = match tool {
                        Tool::Pencil => &[1.0, 2.0, 3.0, 4.0],
                        Tool::Eraser => &[4.0, 6.0, 8.0, 10.0, 16.0, 32.0],
                        Tool::Shape => &[1.0, 2.0, 3.0, 5.0, 8.0],
                        _ => &[1.0, 3.0, 5.0, 8.0, 12.0, 20.0],
                    };
                    let Some(size) = self.size_for_tool() else { return };
                    for &p in presets {
                        let (r, resp) = ui.allocate_exact_size(vec2(170.0, 22.0), Sense::click());
                        let sel = (*size - p).abs() < 0.01;
                        let bg = if sel {
                            theme::SELECTED_BG
                        } else if resp.hovered() {
                            theme::HOVER_BG
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(r, 4.0, bg);
                        ui.painter().text(
                            r.left_center() + vec2(8.0, 0.0),
                            Align2::LEFT_CENTER,
                            format!("{p}px"),
                            FontId::proportional(12.0),
                            theme::SUBTLE_TEXT,
                        );
                        ui.painter().line_segment(
                            [r.left_center() + vec2(48.0, 0.0), r.right_center() - vec2(8.0, 0.0)],
                            Stroke::new(p.min(16.0), theme::TEXT),
                        );
                        if resp.clicked() {
                            *size = p;
                        }
                    }
                    ui.separator();
                    ui.add(egui::Slider::new(size, 1.0..=100.0).suffix(" px").integer());
                });
            });
        });
    }

    fn colors_group(&mut self, ui: &mut Ui) {
        group(ui, "Colors", |ui| {
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    if swatch(ui, Some(self.primary), 36.0, self.active_slot == 0, "Color 1 (left click)").clicked() {
                        self.active_slot = 0;
                    }
                    if swatch(ui, Some(self.secondary), 36.0, self.active_slot == 1, "Color 2 (right click)").clicked() {
                        self.active_slot = 1;
                    }
                });
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                for row in 0..2 {
                    ui.horizontal(|ui| {
                        for c in &PALETTE[row * 10..row * 10 + 10] {
                            let col = Color32::from_rgb(c[0], c[1], c[2]);
                            let r = swatch(ui, Some(col), 20.0, false, "");
                            if r.clicked() {
                                self.set_active_color(col);
                            }
                            if r.secondary_clicked() {
                                self.secondary = col;
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    for i in 0..10 {
                        let c = self.custom_colors.get(i).copied();
                        let r = swatch(ui, c, 20.0, false, "");
                        if let Some(col) = c {
                            if r.clicked() {
                                self.set_active_color(col);
                            }
                            if r.secondary_clicked() {
                                self.secondary = col;
                            }
                        }
                    }
                });
            });
            ui.vertical(|ui| {
                ui.add_space(6.0);
                let r = icon_button(ui, ic::PALETTE, false, 36.0, "Edit colors");
                egui::Popup::menu(&r)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        let mut c = if self.active_slot == 0 { self.primary } else { self.secondary };
                        ui.label(if self.active_slot == 0 { "Color 1" } else { "Color 2" });
                        if egui::widgets::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque) {
                            self.set_active_color(c);
                        }
                        ui.horizontal(|ui| {
                            let hex = format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
                            ui.label(RichText::new(hex).monospace());
                            if ui.button("Add to custom colors").clicked() {
                                self.custom_colors.retain(|x| *x != c);
                                self.custom_colors.insert(0, c);
                                self.custom_colors.truncate(10);
                                ui.close();
                            }
                        });
                    });
            });
        });
    }

    fn text_row(&mut self, ui: &mut Ui) {
        self.ensure_fonts();
        let frame = egui::Frame::NONE
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, theme::DIVIDER))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(8, 4));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}  Text", ic::TEXT_T)).strong());
                ui.add_space(8.0);
                let families = self.fonts.as_ref().map(|f| f.families.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("font-family")
                    .width(200.0)
                    .height(400.0)
                    .selected_text(self.text_style.family.clone())
                    .show_ui(ui, |ui| {
                        for f in &families {
                            ui.selectable_value(&mut self.text_style.family, f.clone(), f);
                        }
                    });
                egui::ComboBox::from_id_salt("font-size")
                    .width(56.0)
                    .selected_text(format!("{}", self.text_style.size_pt))
                    .show_ui(ui, |ui| {
                        for s in FONT_SIZES {
                            ui.selectable_value(&mut self.text_style.size_pt, s, format!("{s}"));
                        }
                    });
                ui.add_space(8.0);
                let st = &mut self.text_style;
                for (on, icon, tip) in [
                    (&mut st.bold, ic::TEXT_B, "Bold"),
                    (&mut st.italic, ic::TEXT_ITALIC, "Italic"),
                    (&mut st.underline, ic::TEXT_UNDERLINE, "Underline"),
                    (&mut st.strike, ic::TEXT_STRIKETHROUGH, "Strikethrough"),
                ] {
                    if icon_button(ui, icon, *on, 26.0, tip).clicked() {
                        *on = !*on;
                    }
                }
                ui.add_space(8.0);
                ui.selectable_value(&mut st.opaque_bg, false, "Transparent background");
                ui.selectable_value(&mut st.opaque_bg, true, "Fill background (Color 2)");
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Click or drag on the canvas to add a text box · Esc to finish")
                        .size(11.0)
                        .color(theme::SUBTLE_TEXT),
                );
            });
        });
    }
}

fn shape_button(ui: &mut Ui, k: ShapeKind, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 20.0), Sense::click());
    let bg = if selected {
        theme::SELECTED_BG
    } else if resp.hovered() {
        theme::HOVER_BG
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 3.0, bg);
    if selected {
        ui.painter().rect_stroke(rect, 3.0, Stroke::new(1.0, theme::ACCENT), StrokeKind::Inside);
    }
    let icon_rect = Rect::from_center_size(rect.center(), vec2(14.0, 14.0));
    for line in shapes::icon_polylines(k, icon_rect) {
        ui.painter().add(Shape::line(line, Stroke::new(1.2, theme::TEXT)));
    }
    resp.on_hover_text(k.name())
}
