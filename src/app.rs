//! Application state, document lifecycle, and the "pending" objects
//! (selection, shape, text box) that stay editable until committed.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};
use tiny_skia::{FillRule, IntRect, Pixmap, Transform};

use crate::history::History;
use crate::menu::{self, Action, NativeMenu};
use crate::shapes::{self, ShapeKind};
use crate::text::{self, Fonts, TextStyle};
use crate::{io, raster, theme};

pub const DEFAULT_W: u32 = 1152;
pub const DEFAULT_H: u32 = 648;
pub const ZOOM_STEPS: [f32; 12] = [0.125, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Pencil,
    Fill,
    Text,
    Eraser,
    Picker,
    Magnifier,
    Brush,
    Shape,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectMode {
    Rect,
    Free,
}

pub struct Sizes {
    pub pencil: f32,
    pub brush: f32,
    pub eraser: f32,
    pub shape: f32,
}

pub struct View {
    pub zoom: f32,
    pub pan: Vec2,
    pub area: Rect,
    pub fit_next: bool,
}

impl View {
    pub fn to_screen(&self, p: Pos2) -> Pos2 {
        self.area.min + self.pan + p.to_vec2() * self.zoom
    }
    pub fn to_canvas(&self, s: Pos2) -> Pos2 {
        ((s - self.area.min - self.pan) / self.zoom).to_pos2()
    }
    pub fn rect_to_screen(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }
}

pub struct Lifted {
    /// Document with the selected pixels removed.
    pub hole: Pixmap,
    /// The selected pixels, moving with the selection.
    pub floating: Pixmap,
}

pub struct Selection {
    /// Where the selection was taken from (and its size).
    pub rect: IntRect,
    /// Free-form outline, relative to `rect`'s origin.
    pub outline: Option<Vec<Pos2>>,
    /// Free-form coverage mask, `rect.width() * rect.height()` bytes.
    pub mask: Option<Vec<u8>>,
    pub lifted: Option<Lifted>,
    /// Current top-left (moves when dragged).
    pub pos: (i32, i32),
}

impl Selection {
    pub fn current_rect(&self) -> Rect {
        Rect::from_min_size(
            pos2(self.pos.0 as f32, self.pos.1 as f32),
            vec2(self.rect.width() as f32, self.rect.height() as f32),
        )
    }
}

pub struct ShapeState {
    pub kind: ShapeKind,
    pub a: Pos2,
    pub b: Pos2,
    /// Drawn with the right button: outline/fill colors swapped.
    pub swap: bool,
}

impl ShapeState {
    pub fn bbox(&self) -> Rect {
        Rect::from_two_pos(self.a, self.b)
    }
}

#[derive(Clone)]
pub struct TextState {
    pub rect: Rect,
    pub text: Vec<char>,
    pub caret: usize,
}

pub enum PendingKind {
    Selection(Selection),
    Shape(ShapeState),
    Text(TextState),
}

pub struct Pending {
    /// Document as it was before the pending object existed.
    pub base: Pixmap,
    pub kind: PendingKind,
}

pub struct DaubApp {
    pub doc: Pixmap,
    pub path: Option<PathBuf>,
    pub history: History,
    pub saved_rev: u64,

    pub tex: Option<egui::TextureHandle>,
    pub tex_dirty: bool,
    pub checker: egui::TextureHandle,
    pub view: View,

    pub tool: Tool,
    pub prev_tool: Tool,
    pub primary: Color32,
    pub secondary: Color32,
    /// 0 = Color 1 (primary), 1 = Color 2 (secondary): which one palette clicks set.
    pub active_slot: u8,
    pub custom_colors: Vec<Color32>,
    pub sizes: Sizes,
    pub shape_kind: ShapeKind,
    pub shape_outline: bool,
    pub shape_fill: bool,
    pub select_mode: SelectMode,
    pub transparent_selection: bool,
    pub text_style: TextStyle,
    pub fonts: Option<Fonts>,

    pub op: crate::canvas::Op,
    /// Mouse button driving the current gesture, if any.
    pub gesture: Option<egui::PointerButton>,
    pub pending: Option<Pending>,
    pub cursor_px: Option<(i32, i32)>,

    pub clipboard: Option<arboard::Clipboard>,
    pub native_menu: Option<NativeMenu>,
    pub title: String,
    pub allow_close: bool,
    /// Actions queued by in-window menus/buttons, run at the start of next frame.
    pub ribbon_actions: Vec<Action>,
}

impl DaubApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        Self::create(&cc.egui_ctx, initial, true)
    }

    pub fn create(ctx: &egui::Context, initial: Option<PathBuf>, native_menu: bool) -> Self {
        theme::apply(ctx);
        let checker = ctx.load_texture(
            "checker",
            egui::ColorImage::new(
                [2, 2],
                vec![
                    Color32::WHITE,
                    Color32::from_gray(214),
                    Color32::from_gray(214),
                    Color32::WHITE,
                ],
            ),
            egui::TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Nearest,
                wrap_mode: egui::TextureWrapMode::Repeat,
                mipmap_mode: None,
            },
        );
        let mut app = Self {
            doc: raster::new_canvas(DEFAULT_W, DEFAULT_H, Color32::WHITE),
            path: None,
            history: History::default(),
            saved_rev: 0,
            tex: None,
            tex_dirty: true,
            checker,
            view: View { zoom: 1.0, pan: Vec2::ZERO, area: Rect::NOTHING, fit_next: true },
            tool: Tool::Pencil,
            prev_tool: Tool::Pencil,
            primary: Color32::BLACK,
            secondary: Color32::WHITE,
            active_slot: 0,
            custom_colors: vec![],
            sizes: Sizes { pencil: 1.0, brush: 5.0, eraser: 8.0, shape: 2.0 },
            shape_kind: ShapeKind::Rect,
            shape_outline: true,
            shape_fill: false,
            select_mode: SelectMode::Rect,
            transparent_selection: false,
            text_style: TextStyle {
                family: String::new(),
                size_pt: 18.0,
                bold: false,
                italic: false,
                underline: false,
                strike: false,
                opaque_bg: false,
            },
            fonts: None,
            op: crate::canvas::Op::Idle,
            gesture: None,
            pending: None,
            cursor_px: None,
            clipboard: arboard::Clipboard::new().ok(),
            native_menu: if native_menu { NativeMenu::install(ctx) } else { None },
            title: String::new(),
            allow_close: false,
            ribbon_actions: vec![],
        };
        if let Some(p) = initial {
            app.open_path(p);
        }
        app
    }

    // ---------------------------------------------------------------- state

    pub fn is_dirty(&self) -> bool {
        self.history.revision() != self.saved_rev || self.pending_changes_doc()
    }

    fn pending_changes_doc(&self) -> bool {
        match &self.pending {
            None => false,
            Some(p) => match &p.kind {
                PendingKind::Selection(s) => s.lifted.is_some(),
                _ => true,
            },
        }
    }

    pub fn file_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into())
    }

    pub fn mark_changed(&mut self) {
        self.tex_dirty = true;
    }

    pub fn set_tool(&mut self, t: Tool) {
        if t == self.tool {
            return;
        }
        self.commit_pending();
        if t == Tool::Picker {
            self.prev_tool = self.tool;
        }
        if t == Tool::Text {
            self.ensure_fonts();
        }
        self.tool = t;
    }

    pub fn ensure_fonts(&mut self) {
        if self.fonts.is_none() {
            let f = Fonts::load();
            if self.text_style.family.is_empty() {
                self.text_style.family = f.default_family();
            }
            self.fonts = Some(f);
        }
    }

    pub fn size_for_tool(&mut self) -> Option<&mut f32> {
        match self.tool {
            Tool::Pencil => Some(&mut self.sizes.pencil),
            Tool::Brush => Some(&mut self.sizes.brush),
            Tool::Eraser => Some(&mut self.sizes.eraser),
            Tool::Shape => Some(&mut self.sizes.shape),
            _ => None,
        }
    }

    pub fn set_active_color(&mut self, c: Color32) {
        if self.active_slot == 0 {
            self.primary = c;
        } else {
            self.secondary = c;
        }
    }

    // ------------------------------------------------------------- pending

    /// Bake whatever is pending into the document and record one undo step.
    pub fn commit_pending(&mut self) {
        if let Some(p) = self.pending.take() {
            self.history.record(&p.base, &self.doc);
            self.mark_changed();
        }
    }

    /// Drop the pending object and restore the document under it.
    pub fn cancel_pending(&mut self) {
        if let Some(p) = self.pending.take() {
            raster::restore(&mut self.doc, &p.base);
            self.mark_changed();
        }
    }

    /// Redraw the document from the pending object's base.
    pub fn rerender_pending(&mut self) {
        let Some(p) = self.pending.take() else { return };
        match &p.kind {
            PendingKind::Selection(s) => {
                if let Some(l) = &s.lifted {
                    raster::restore(&mut self.doc, &l.hole);
                    raster::draw_over(&mut self.doc, &l.floating, s.pos.0, s.pos.1);
                }
            }
            PendingKind::Shape(s) => {
                raster::restore(&mut self.doc, &p.base);
                self.draw_shape(s);
            }
            PendingKind::Text(t) => {
                raster::restore(&mut self.doc, &p.base);
                if let Some((font, lay)) = self.text_layout_for(t) {
                    let bg = self.text_style.opaque_bg.then_some(self.secondary);
                    text::render(&mut self.doc, &font, &self.text_style, &lay, t.rect, self.primary, bg);
                }
            }
        }
        // `base` stays untouched: commit and undo compare against it.
        self.pending = Some(p);
        self.mark_changed();
    }

    pub fn draw_shape(&mut self, s: &ShapeState) {
        let Some(path) = shapes::path(s.kind, s.a, s.b) else { return };
        let (outline, fill) =
            if s.swap { (self.secondary, self.primary) } else { (self.primary, self.secondary) };
        if self.shape_fill && !s.kind.is_line() {
            self.doc.fill_path(
                &path,
                &raster::paint(fill, true),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        if self.shape_outline || s.kind.is_line() {
            let stroke = tiny_skia::Stroke {
                width: self.sizes.shape,
                line_cap: if s.kind.is_line() { tiny_skia::LineCap::Round } else { tiny_skia::LineCap::Butt },
                line_join: if s.kind.smooth_joins() {
                    tiny_skia::LineJoin::Round
                } else {
                    tiny_skia::LineJoin::Miter
                },
                miter_limit: 4.0,
                ..Default::default()
            };
            self.doc.stroke_path(&path, &raster::paint(outline, true), &stroke, Transform::identity(), None);
        }
    }

    pub fn text_layout_for(&mut self, t: &TextState) -> Option<(Arc<ab_glyph::FontVec>, text::Layout)> {
        self.ensure_fonts();
        let st = &self.text_style;
        let font = self.fonts.as_mut()?.font(&st.family, st.bold, st.italic)?;
        let lay = text::layout(&font, st.px(), &t.text, t.rect.width().max(1.0));
        Some((font, lay))
    }

    pub fn pending_text(&self) -> Option<TextState> {
        match &self.pending {
            Some(Pending { kind: PendingKind::Text(t), .. }) => Some(t.clone()),
            _ => None,
        }
    }

    pub fn pending_selection(&self) -> Option<&Selection> {
        match &self.pending {
            Some(Pending { kind: PendingKind::Selection(s), .. }) => Some(s),
            _ => None,
        }
    }

    /// Cut the selected pixels out of the document so they can move.
    pub fn lift_selection(&mut self) {
        let secondary = self.secondary;
        let transparent = self.transparent_selection;
        let Some(Pending { kind: PendingKind::Selection(s), .. }) = &mut self.pending else { return };
        if s.lifted.is_some() {
            return;
        }
        let Some(mut floating) = raster::extract(&self.doc, s.rect, s.mask.as_deref()) else { return };
        if transparent {
            raster::key_out(&mut floating, secondary);
        }
        let mut hole = self.doc.clone();
        raster::fill_region(&mut hole, s.rect, s.mask.as_deref(), secondary);
        s.lifted = Some(Lifted { hole, floating });
        self.rerender_pending();
    }

    pub fn start_selection(&mut self, rect: IntRect, outline: Option<Vec<Pos2>>, mask: Option<Vec<u8>>) {
        self.commit_pending();
        self.pending = Some(Pending {
            base: self.doc.clone(),
            kind: PendingKind::Selection(Selection { rect, outline, mask, lifted: None, pos: (rect.x(), rect.y()) }),
        });
    }

    // -------------------------------------------------------------- actions

    pub fn perform(&mut self, a: Action, ctx: &egui::Context) {
        match a {
            Action::New => {
                if self.confirm_discard() {
                    self.replace_doc(raster::new_canvas(DEFAULT_W, DEFAULT_H, Color32::WHITE), None);
                }
            }
            Action::Open => self.open_dialog(),
            Action::Save => {
                self.save();
            }
            Action::SaveAs => {
                self.save_as();
            }
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Cut => {
                if self.copy() {
                    self.delete_selection();
                }
            }
            Action::Copy => {
                self.copy();
            }
            Action::Paste => self.paste(),
            Action::Delete => self.delete_selection(),
            Action::SelectAll => {
                self.set_tool(Tool::Select);
                let r = IntRect::from_xywh(0, 0, self.doc.width(), self.doc.height()).unwrap();
                self.start_selection(r, None, None);
            }
            Action::ZoomIn => self.zoom_step(1),
            Action::ZoomOut => self.zoom_step(-1),
            Action::ZoomActual => {
                let c = self.view.area.center();
                self.zoom_at(1.0, c);
            }
            Action::ZoomFit => self.view.fit_next = true,
        }
    }

    pub fn undo(&mut self) {
        if !matches!(self.op, crate::canvas::Op::Idle) {
            return;
        }
        if self.pending.is_some() {
            // Undo first discards whatever is still being edited.
            self.cancel_pending();
            return;
        }
        if self.history.undo(&mut self.doc) {
            self.mark_changed();
        }
    }

    pub fn redo(&mut self) {
        if !matches!(self.op, crate::canvas::Op::Idle) || self.pending.is_some() {
            return;
        }
        if self.history.redo(&mut self.doc) {
            self.mark_changed();
        }
    }

    /// Copy the selection to the system clipboard. Returns true if something was copied.
    pub fn copy(&mut self) -> bool {
        let Some(s) = self.pending_selection() else { return false };
        let img = match &s.lifted {
            Some(l) => Some(l.floating.clone()),
            None => raster::extract(&self.doc, s.rect, s.mask.as_deref()),
        };
        let (Some(img), Some(cb)) = (img, self.clipboard.as_mut()) else { return false };
        let data = arboard::ImageData {
            width: img.width() as usize,
            height: img.height() as usize,
            bytes: raster::to_rgba_unmul(&img).into(),
        };
        cb.set_image(data).is_ok()
    }

    pub fn delete_selection(&mut self) {
        let secondary = self.secondary;
        let Some(Pending { kind: PendingKind::Selection(s), .. }) = &self.pending else { return };
        match &s.lifted {
            Some(l) => raster::restore(&mut self.doc, &l.hole),
            None => {
                let (rect, mask) = (s.rect, s.mask.clone());
                raster::fill_region(&mut self.doc, rect, mask.as_deref(), secondary);
            }
        }
        self.commit_pending();
    }

    pub fn paste(&mut self) {
        // Typing into a text box: paste text.
        if let Some(Pending { kind: PendingKind::Text(_), .. }) = &self.pending {
            if let Some(s) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) {
                self.text_insert(&s);
            }
            return;
        }
        let Some(img) = self.clipboard.as_mut().and_then(|c| c.get_image().ok()) else { return };
        let Some(pm) = raster::from_rgba_unmul(img.width as u32, img.height as u32, &img.bytes) else {
            return;
        };
        self.commit_pending();
        if pm.width() > self.doc.width() || pm.height() > self.doc.height() {
            let grown = raster::grow(&self.doc, pm.width(), pm.height(), Color32::WHITE);
            self.history.record(&self.doc, &grown);
            self.doc = grown;
        }
        // Paste at the top-left of the visible part of the canvas.
        let tl = self.view.to_canvas(self.view.area.min);
        let x = (tl.x.max(0.0) as i32).min(self.doc.width() as i32 - pm.width() as i32).max(0);
        let y = (tl.y.max(0.0) as i32).min(self.doc.height() as i32 - pm.height() as i32).max(0);
        self.tool = Tool::Select;
        let rect = IntRect::from_xywh(x, y, pm.width(), pm.height()).unwrap();
        self.pending = Some(Pending {
            base: self.doc.clone(),
            kind: PendingKind::Selection(Selection {
                rect,
                outline: None,
                mask: None,
                lifted: Some(Lifted { hole: self.doc.clone(), floating: pm }),
                pos: (x, y),
            }),
        });
        self.rerender_pending();
    }

    pub fn text_insert(&mut self, s: &str) {
        if let Some(Pending { kind: PendingKind::Text(t), .. }) = &mut self.pending {
            for c in s.replace("\r\n", "\n").chars() {
                t.text.insert(t.caret, c);
                t.caret += 1;
            }
            self.rerender_pending();
        }
    }

    // ----------------------------------------------------------------- view

    pub fn zoom_at(&mut self, zoom: f32, anchor: Pos2) {
        let zoom = zoom.clamp(ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1]);
        let c = self.view.to_canvas(anchor);
        self.view.zoom = zoom;
        self.view.pan = anchor - self.view.area.min - c.to_vec2() * zoom;
    }

    pub fn zoom_step(&mut self, dir: i32) {
        let z = self.view.zoom;
        let next = if dir > 0 {
            ZOOM_STEPS.iter().copied().find(|s| *s > z + 1e-3)
        } else {
            ZOOM_STEPS.iter().rev().copied().find(|s| *s < z - 1e-3)
        };
        if let Some(n) = next {
            let c = self.view.area.center();
            self.zoom_at(n, c);
        }
    }

    pub fn fit(&mut self) {
        let a = self.view.area;
        if !a.is_positive() {
            return;
        }
        let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
        let z = ((a.width() - 48.0) / w).min((a.height() - 48.0) / h).min(1.0);
        self.view.zoom = z.max(ZOOM_STEPS[0] / 2.0);
        self.view.pan = (a.size() - vec2(w, h) * self.view.zoom) / 2.0;
    }

    // ---------------------------------------------------------------- files

    pub fn replace_doc(&mut self, pm: Pixmap, path: Option<PathBuf>) {
        self.pending = None;
        self.op = crate::canvas::Op::Idle;
        self.gesture = None;
        self.doc = pm;
        self.path = path;
        self.history.clear();
        self.saved_rev = self.history.revision();
        self.view.fit_next = true;
        self.mark_changed();
    }

    pub fn open_dialog(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        if let Some(p) = rfd::FileDialog::new().add_filter("Images", io::OPEN_EXTS).pick_file() {
            self.open_path(p);
        }
    }

    pub fn open_path(&mut self, p: PathBuf) {
        match io::load(&p) {
            Ok(pm) => self.replace_doc(pm, Some(p)),
            Err(e) => error_dialog(&format!("Couldn't open {}", p.display()), &e),
        }
    }

    pub fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(p) => self.save_to(p),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) -> bool {
        let mut d = rfd::FileDialog::new().set_file_name(self.file_name_for_save());
        for (name, exts) in io::SAVE_FORMATS {
            d = d.add_filter(*name, exts);
        }
        match d.save_file() {
            Some(mut p) => {
                if p.extension().is_none() {
                    p.set_extension("png");
                }
                self.save_to(p)
            }
            None => false,
        }
    }

    fn file_name_for_save(&self) -> String {
        match &self.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => "Untitled.png".into(),
        }
    }

    fn save_to(&mut self, p: PathBuf) -> bool {
        self.commit_pending();
        match io::save(&self.doc, &p) {
            Ok(()) => {
                self.path = Some(p);
                self.saved_rev = self.history.revision();
                true
            }
            Err(e) => {
                error_dialog(&format!("Couldn't save {}", p.display()), &e);
                false
            }
        }
    }

    /// Ask to save unsaved changes. Returns false if the user cancelled.
    pub fn confirm_discard(&mut self) -> bool {
        if !self.is_dirty() {
            return true;
        }
        let res = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Daub")
            .set_description(format!("Do you want to save changes to {}?", self.file_name()))
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                "Save".into(),
                "Don't Save".into(),
                "Cancel".into(),
            ))
            .show();
        match res {
            rfd::MessageDialogResult::Custom(s) if s == "Save" => self.save(),
            rfd::MessageDialogResult::Yes => self.save(),
            rfd::MessageDialogResult::Custom(s) if s == "Don't Save" => true,
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }
}

pub fn error_dialog(title: &str, msg: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title)
        .set_description(msg)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

impl eframe::App for DaubApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame_ui(ui);
    }
}

impl DaubApp {
    pub fn frame_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // Actions from the native menu (or fallback shortcuts).
        let mut actions = match &self.native_menu {
            Some(m) => m.drain(),
            None => menu::egui_shortcuts(&ctx),
        };
        actions.append(&mut self.ribbon_actions);
        for a in actions {
            self.perform(a, &ctx);
        }

        // Files dropped on the window.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if let Some(p) = dropped.into_iter().next()
            && self.confirm_discard() {
                self.open_path(p);
            }

        // Window close with unsaved changes.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close
            && self.is_dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.confirm_discard() {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }

        let title = format!("{}{} - Daub", if self.is_dirty() { "*" } else { "" }, self.file_name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("ribbon")
            .frame(egui::Frame::NONE.fill(theme::RIBBON_BG).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(ui, |ui| self.ribbon_ui(ui));
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(egui::Frame::NONE.fill(theme::STATUS_BG).inner_margin(egui::Margin::symmetric(10, 0)))
            .show(ui, |ui| self.status_ui(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::CANVAS_AREA_BG))
            .show(ui, |ui| self.canvas_ui(ui));
    }
}
