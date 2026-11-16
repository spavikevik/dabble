//! Application state, document lifecycle, and the "pending" objects
//! (selection, shape, text box) that stay editable until committed.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};
use tiny_skia::{FillRule, IntRect, Pixmap, Transform};

use crate::history::History;
use crate::layers::{self, Layer};
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
    /// The selected pixels as last rotated/flipped/skewed, before any
    /// handle-drag scaling. Scaling always starts from this to avoid blur build-up.
    pub source: Pixmap,
    /// The selected pixels as shown, moving with the selection.
    pub floating: Pixmap,
}

impl Lifted {
    pub fn new(hole: Pixmap, floating: Pixmap) -> Self {
        Self { hole, source: floating.clone(), floating }
    }
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

#[derive(Clone)]
pub struct ShapeState {
    pub kind: ShapeKind,
    /// Corners of the box (or a line/curve's endpoints).
    pub a: Pos2,
    pub b: Pos2,
    /// Curve: up to two control points. Polygon: every corner.
    pub pts: Vec<Pos2>,
    /// Curve: 0 drawing, 1 waiting for first bend, 2 for second, 3 done.
    /// Polygon: 0 first side, 1 adding corners, 2 closed.
    pub stage: u8,
    /// Drawn with the right button: outline/fill colors swapped.
    pub swap: bool,
}

impl ShapeState {
    pub fn new(kind: ShapeKind, a: Pos2, swap: bool) -> Self {
        Self { kind, a, b: a, pts: vec![], stage: 0, swap }
    }

    /// Draggable points for shapes edited by their points.
    pub fn vertices(&self) -> Vec<Pos2> {
        match self.kind {
            ShapeKind::Polygon if self.stage > 0 => self.pts.clone(),
            ShapeKind::Curve => [self.a, self.b].into_iter().chain(self.pts.iter().copied()).collect(),
            _ => vec![self.a, self.b],
        }
    }

    pub fn set_vertex(&mut self, i: usize, p: Pos2) {
        match (self.kind, i) {
            (ShapeKind::Polygon, _) if self.stage > 0 => self.pts[i] = p,
            (_, 0) => self.a = p,
            (_, 1) => self.b = p,
            (_, i) => self.pts[i - 2] = p,
        }
    }

    pub fn translate(&mut self, d: egui::Vec2) {
        self.a += d;
        self.b += d;
        for p in &mut self.pts {
            *p += d;
        }
    }

    pub fn bbox(&self) -> Rect {
        if self.kind.by_points() { Rect::from_points(&self.vertices()) } else { Rect::from_two_pos(self.a, self.b) }
    }

    /// Whether the fill applies (closed outline).
    pub fn closed(&self) -> bool {
        match self.kind {
            ShapeKind::Line | ShapeKind::Curve => false,
            ShapeKind::Polygon => self.stage >= 2,
            _ => true,
        }
    }

    pub fn path(&self) -> Option<tiny_skia::Path> {
        match self.kind {
            ShapeKind::Curve => shapes::curve_path(self.a, self.b, &self.pts),
            ShapeKind::Polygon if self.stage > 0 => shapes::polygon_path(&self.pts, self.closed()),
            k => shapes::path(k, self.a, self.b),
        }
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

pub struct DabbleApp {
    /// Pixels of the active layer (see `layers.rs`).
    pub doc: Pixmap,
    pub layers: Vec<Layer>,
    pub active: usize,
    pub layer_texs: Vec<Option<egui::TextureHandle>>,
    /// Inactive layer textures need re-uploading.
    pub layers_dirty: bool,
    pub show_layers: bool,
    pub layer_counter: u32,
    pub renaming: Option<(usize, String)>,
    pub theme_pref: theme::ThemePref,
    pub dark: bool,
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
    pub brush_kind: crate::brushes::BrushKind,
    /// Brush menu previews, rendered once per light/dark mode.
    pub brush_previews: Option<(bool, Vec<egui::TextureHandle>)>,
    pub shape_kind: ShapeKind,
    pub shape_outline: shapes::PaintStyle,
    pub shape_fill: shapes::PaintStyle,
    pub select_mode: SelectMode,
    pub transparent_selection: bool,
    /// Erasing, deleting and growing the canvas leave transparent pixels
    /// instead of Color 2 / white.
    pub transparent_bg: bool,
    pub dialog: Option<crate::dialogs::Dialog>,
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

    pub settings: crate::settings::Settings,
    /// The native macOS toolbar and inspector, when that interface is on.
    pub native: Option<crate::mac_ui::NativeUi>,
    #[cfg(target_os = "macos")]
    pub ns_window: Option<objc2::rc::Retained<objc2_app_kit::NSWindow>>,
    /// Frames drawn, for the debug snapshot.
    pub frames: u64,
}

impl DabbleApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        #[allow(unused_mut)]
        let mut app = Self::create(&cc.egui_ctx, initial, true);
        #[cfg(target_os = "macos")]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = cc.window_handle()
                && let RawWindowHandle::AppKit(a) = h.as_raw()
            {
                app.ns_window = crate::mac_ui::window_of(a.ns_view);
            }
            if app.settings.interface == crate::settings::Interface::Native {
                app.set_native(&cc.egui_ctx, true);
            }
        }
        app
    }

    /// Turn the native macOS interface on or off.
    #[allow(unused_variables)]
    pub fn set_native(&mut self, ctx: &egui::Context, on: bool) {
        #[cfg(target_os = "macos")]
        {
            if on && self.native.is_none() {
                if let Some(w) = self.ns_window.clone() {
                    self.native = crate::mac_ui::NativeUi::install(ctx, w);
                    self.apply_native_appearance();
                }
            } else if !on && let Some(ui) = self.native.take() {
                ui.uninstall();
            }
            self.view.fit_next = true;
        }
    }

    #[cfg(target_os = "macos")]
    fn apply_native_appearance(&self) {
        if let Some(ui) = &self.native {
            ui.set_appearance(match self.theme_pref {
                theme::ThemePref::System => None,
                theme::ThemePref::Light => Some(false),
                theme::ThemePref::Dark => Some(true),
            });
        }
    }

    pub fn apply_settings(&mut self, ctx: &egui::Context, s: crate::settings::Settings) {
        self.theme_pref = s.theme;
        self.set_native(ctx, s.interface == crate::settings::Interface::Native);
        #[cfg(target_os = "macos")]
        self.apply_native_appearance();
        s.save();
        self.settings = s;
    }

    /// Handle input from the native controls.
    pub fn apply_native_event(&mut self, ev: crate::mac_ui::NativeEvent, ctx: &egui::Context) {
        use crate::mac_ui::{LayerCmd, NativeEvent as E};
        match ev {
            E::Action(a) => self.perform(a, ctx),
            E::Tool(t) => self.set_tool(t),
            E::SelectMode(m) => {
                self.select_mode = m;
                self.set_tool(Tool::Select);
            }
            E::TransparentSelection(on) => self.transparent_selection = on,
            E::Brush(k) => {
                self.brush_kind = k;
                self.set_tool(Tool::Brush);
            }
            E::Shape(k) => {
                self.commit_pending();
                self.shape_kind = k;
                self.set_tool(Tool::Shape);
            }
            E::Outline(st) => self.shape_outline = st,
            E::Fill(st) => self.shape_fill = st,
            E::Size(v) => {
                if let Some(s) = self.size_for_tool() {
                    *s = v.clamp(1.0, 100.0);
                }
            }
            E::Color1(c) => self.primary = c,
            E::Color2(c) => self.secondary = c,
            E::Palette(c, second) => {
                if second {
                    self.secondary = c;
                } else {
                    self.primary = c;
                }
            }
            E::Layer(cmd) => match cmd {
                LayerCmd::Select(i) => self.select_layer(i),
                LayerCmd::ToggleVisible(i) => {
                    if let Some(l) = self.layers.get_mut(i) {
                        l.visible = !l.visible;
                        self.mark_changed();
                    }
                }
                LayerCmd::Opacity(o) => {
                    self.layers[self.active].opacity = o.clamp(0.0, 1.0);
                    self.mark_changed();
                }
                LayerCmd::Add => self.add_layer(),
                LayerCmd::Duplicate => self.duplicate_layer(),
                LayerCmd::Delete => self.delete_layer(),
                LayerCmd::Up => self.move_layer(1),
                LayerCmd::Down => self.move_layer(-1),
                LayerCmd::Merge => self.merge_down(),
                LayerCmd::Rename => {
                    let name = self.layers[self.active].name.clone();
                    if let Some(n) = crate::mac_ui::prompt_text("Rename Layer", &name)
                        && !n.trim().is_empty()
                    {
                        self.layers[self.active].name = n.trim().to_string();
                    }
                }
            },
            E::Font(i) => {
                if let Some(f) = self.fonts.as_ref().and_then(|f| f.families.get(i)).cloned() {
                    self.text_style.family = f;
                }
            }
            E::FontSize(v) => self.text_style.size_pt = v,
            E::TextStyle([b, i, u, st]) => {
                let t = &mut self.text_style;
                (t.bold, t.italic, t.underline, t.strike) = (b, i, u, st);
            }
            E::TextBackground(on) => self.text_style.opaque_bg = on,
            E::ToggleInspector => self.toggle_inspector(),
        }
    }

    pub fn toggle_inspector(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(ui) = &mut self.native {
            let v = !ui.inspector_visible;
            ui.set_inspector_visible(v);
        }
    }

    /// Canvas area the native toolbar and inspector leave free: (top, right).
    pub fn native_insets(&self) -> (f32, f32) {
        #[cfg(target_os = "macos")]
        if let Some(ui) = &self.native {
            return ui.insets();
        }
        (0.0, 0.0)
    }

    /// `system`: load settings and install the macOS menu bar (off in tests).
    pub fn create(ctx: &egui::Context, initial: Option<PathBuf>, system: bool) -> Self {
        theme::install(ctx);
        let settings = if system { crate::settings::Settings::load() } else { Default::default() };
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
            layers: vec![Layer::new("Background".into(), layers::placeholder())],
            active: 0,
            layer_texs: vec![],
            layers_dirty: true,
            show_layers: false,
            layer_counter: 0,
            renaming: None,
            dark: false,
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
            brush_kind: crate::brushes::BrushKind::Brush,
            brush_previews: None,
            shape_kind: ShapeKind::Rect,
            shape_outline: shapes::PaintStyle::Solid,
            shape_fill: shapes::PaintStyle::None,
            select_mode: SelectMode::Rect,
            transparent_selection: false,
            transparent_bg: false,
            dialog: None,
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
            native_menu: if system { NativeMenu::install(ctx) } else { None },
            title: String::new(),
            allow_close: false,
            ribbon_actions: vec![],
            theme_pref: settings.theme,
            settings,
            native: None,
            #[cfg(target_os = "macos")]
            ns_window: None,
            frames: 0,
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

    /// What new canvas space is filled with.
    pub fn canvas_bg(&self) -> Color32 {
        if self.transparent_bg { Color32::TRANSPARENT } else { Color32::WHITE }
    }

    /// What the eraser, Delete and a moved selection leave behind. Layers
    /// above the background always erase to transparent.
    pub fn erase_color(&self) -> Color32 {
        if self.transparent_bg || self.active > 0 { Color32::TRANSPARENT } else { self.secondary }
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
        // An unfinished polygon closes when it is committed.
        if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &mut self.pending
            && s.kind == ShapeKind::Polygon
            && s.stage == 1
        {
            s.stage = 2;
            self.rerender_pending();
        }
        if let Some(p) = self.pending.take() {
            self.history.record(self.active, &p.base, &self.doc);
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
        use shapes::PaintStyle;
        let Some(path) = s.path() else { return };
        let (outline, fill) =
            if s.swap { (self.secondary, self.primary) } else { (self.primary, self.secondary) };
        let (w, h) = (self.doc.width(), self.doc.height());
        // Textured styles render coverage into a mask, then paint it with the brush's texture.
        let textured = |doc: &mut Pixmap, color: Color32, style: PaintStyle, f: &dyn Fn(&mut tiny_skia::Mask)| {
            let (Some(kind), Some(mut m)) = (style.brush(), tiny_skia::Mask::new(w, h)) else { return };
            f(&mut m);
            crate::brushes::apply_mask(doc, &m, color, kind);
        };
        if self.shape_fill != PaintStyle::None && s.closed() {
            if self.shape_fill == PaintStyle::Solid {
                self.doc.fill_path(&path, &raster::paint(fill, true), FillRule::Winding, Transform::identity(), None);
            } else {
                textured(&mut self.doc, fill, self.shape_fill, &|m| {
                    m.fill_path(&path, FillRule::Winding, true, Transform::identity())
                });
            }
        }
        // Open shapes always get an outline, or there would be nothing to see.
        let outline_style = match self.shape_outline {
            PaintStyle::None if s.kind.is_open() || !s.closed() => PaintStyle::Solid,
            st => st,
        };
        if outline_style != PaintStyle::None {
            let open = s.kind.is_open() || !s.closed();
            let stroke = tiny_skia::Stroke {
                width: self.sizes.shape,
                line_cap: if open { tiny_skia::LineCap::Round } else { tiny_skia::LineCap::Butt },
                line_join: if s.kind.smooth_joins() {
                    tiny_skia::LineJoin::Round
                } else {
                    tiny_skia::LineJoin::Miter
                },
                miter_limit: 4.0,
                ..Default::default()
            };
            if outline_style == PaintStyle::Solid {
                self.doc.stroke_path(&path, &raster::paint(outline, true), &stroke, Transform::identity(), None);
            } else if let Some(sp) = path.stroke(&stroke, 1.0) {
                textured(&mut self.doc, outline, outline_style, &|m| {
                    m.fill_path(&sp, FillRule::Winding, true, Transform::identity())
                });
            }
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
        let erase = self.erase_color();
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
        raster::fill_region(&mut hole, s.rect, s.mask.as_deref(), erase);
        s.lifted = Some(Lifted::new(hole, floating));
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
                    self.transparent_bg = false;
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
            Action::Crop => self.crop(),
            Action::ResizeSkew => self.open_resize_dialog(),
            Action::ImageProperties => self.open_properties_dialog(),
            Action::RotateRight => self.transform(|pm, _| Some(raster::rotate90(pm, true))),
            Action::RotateLeft => self.transform(|pm, _| Some(raster::rotate90(pm, false))),
            Action::Rotate180 => self.transform(|pm, _| Some(raster::rotate180(pm))),
            Action::FlipHorizontal => self.transform(|pm, _| Some(raster::flip(pm, true))),
            Action::FlipVertical => self.transform(|pm, _| Some(raster::flip(pm, false))),
            Action::ToggleLayers => {
                if self.native.is_some() {
                    self.toggle_inspector();
                } else {
                    self.show_layers = !self.show_layers;
                }
            }
            Action::ThemeSystem | Action::ThemeLight | Action::ThemeDark => {
                let mut s = self.settings.clone();
                s.theme = match a {
                    Action::ThemeLight => theme::ThemePref::Light,
                    Action::ThemeDark => theme::ThemePref::Dark,
                    _ => theme::ThemePref::System,
                };
                if self.native_menu.is_some() {
                    self.apply_settings(ctx, s);
                } else {
                    // Tests: don't write the settings file.
                    self.theme_pref = s.theme;
                    self.settings = s;
                }
            }
            Action::Settings => self.dialog = Some(crate::dialogs::Dialog::Settings(self.settings.clone())),
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
        if self.history.can_undo() {
            self.with_layers(|ls, act, h| {
                if let Some(a) = h.undo(ls) {
                    *act = a;
                }
            });
        }
    }

    pub fn redo(&mut self) {
        if !matches!(self.op, crate::canvas::Op::Idle) || self.pending.is_some() {
            return;
        }
        if self.history.can_redo() {
            self.with_layers(|ls, act, h| {
                if let Some(a) = h.redo(ls) {
                    *act = a;
                }
            });
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
        let erase = self.erase_color();
        let Some(Pending { kind: PendingKind::Selection(s), .. }) = &self.pending else { return };
        match &s.lifted {
            Some(l) => raster::restore(&mut self.doc, &l.hole),
            None => {
                let (rect, mask) = (s.rect, s.mask.clone());
                raster::fill_region(&mut self.doc, rect, mask.as_deref(), erase);
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
            let (w, h) = (pm.width(), pm.height());
            self.map_layers(|_, l, bg| Some(raster::grow(l, w, h, bg)));
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
                lifted: Some(Lifted::new(self.doc.clone(), pm)),
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

    // ---------------------------------------------------------- image ops

    /// Apply `f` to the selection if there is one, otherwise to the whole
    /// image. `f` gets the pixels and the color for uncovered space.
    pub fn transform(&mut self, f: impl Fn(&Pixmap, Color32) -> Option<Pixmap>) {
        if self.pending_selection().is_some() {
            self.lift_selection();
            let Some(Pending { kind: PendingKind::Selection(s), .. }) = &mut self.pending else { return };
            // Keep the selection centered where it was.
            let c = s.current_rect().center();
            let Some(l) = &mut s.lifted else { return };
            let Some(new) = f(&l.floating, Color32::TRANSPARENT) else {
                error_dialog("Dabble", "The result would be too large.");
                return;
            };
            let (w, h) = (new.width(), new.height());
            s.pos = ((c.x - w as f32 / 2.0).round() as i32, (c.y - h as f32 / 2.0).round() as i32);
            s.rect = IntRect::from_xywh(s.rect.x(), s.rect.y(), w, h).unwrap();
            s.outline = None;
            s.mask = None;
            l.source = new.clone();
            l.floating = new;
            self.rerender_pending();
        } else {
            // The whole image: every layer.
            if !self.map_layers(|_, pm, bg| f(pm, bg)) {
                error_dialog("Dabble", "The result would be too large.");
            }
        }
    }

    /// Scale the lifted selection to `w`×`h` from its unscaled source.
    pub fn scale_selection(&mut self, pos: (i32, i32), w: u32, h: u32) {
        self.lift_selection();
        let Some(Pending { kind: PendingKind::Selection(s), .. }) = &mut self.pending else { return };
        let Some(l) = &mut s.lifted else { return };
        let Some(new) = raster::scale(&l.source, w, h) else { return };
        l.floating = new;
        s.pos = pos;
        s.rect = IntRect::from_xywh(s.rect.x(), s.rect.y(), w, h).unwrap();
        s.outline = None;
        s.mask = None;
        self.rerender_pending();
    }

    /// Crop the image to the selection.
    pub fn crop(&mut self) {
        let bg = self.canvas_bg();
        let Some(Pending { kind: PendingKind::Selection(s), .. }) = &self.pending else { return };
        let content = match &s.lifted {
            Some(l) => Some(l.floating.clone()),
            None => raster::extract(&self.doc, s.rect, s.mask.as_deref()),
        };
        let Some(content) = content else { return };
        let out = if s.mask.is_none() && s.lifted.is_none() {
            content
        } else {
            // Free-form or moved: put what's selected on a fresh background.
            let mut out = raster::new_canvas(content.width(), content.height(), bg);
            raster::draw_over(&mut out, &content, 0, 0);
            out
        };
        // Other layers are cut to the same rectangle (and free-form outline).
        let (x, y) = s.pos;
        let mask = if s.lifted.is_none() { s.mask.clone() } else { None };
        let p = self.pending.take().unwrap();
        self.doc = p.base;
        let active = self.active;
        self.map_layers(|i, pm, bg| {
            if i == active {
                return Some(out.clone());
            }
            let mut o = raster::new_canvas(out.width(), out.height(), bg);
            raster::copy_into(&mut o, pm, -x, -y);
            if let Some(m) = &mask {
                let r = IntRect::from_xywh(0, 0, o.width(), o.height())?;
                let outside: Vec<u8> = m.iter().map(|v| if *v == 0 { 255 } else { 0 }).collect();
                raster::fill_region(&mut o, r, Some(&outside), bg);
            }
            Some(o)
        });
    }

    pub fn set_canvas_size(&mut self, w: u32, h: u32) {
        self.commit_pending();
        if (w, h) == (self.doc.width(), self.doc.height()) {
            return;
        }
        self.map_layers(|_, pm, bg| raster::resize_canvas(pm, w, h, bg));
    }

    /// Resize (to `w`×`h`) then skew the selection, or the whole image.
    pub fn resize_and_skew(&mut self, w: u32, h: u32, skew_h: f32, skew_v: f32) {
        self.transform(move |pm, bg| {
            let scaled = raster::scale(pm, w, h)?;
            raster::skew(&scaled, skew_h, skew_v, bg)
        });
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
        self.layers = vec![Layer::new("Background".into(), layers::placeholder())];
        self.active = 0;
        self.layer_counter = 0;
        self.renaming = None;
        self.layers_dirty = true;
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
            Ok(pm) => {
                self.transparent_bg = raster::has_transparency(&pm);
                self.replace_doc(pm, Some(p));
            }
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
        match io::save(&self.flatten(), &p) {
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
            .set_title("Dabble")
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

impl eframe::App for DabbleApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame_ui(ui);
    }
}

impl DabbleApp {
    pub fn frame_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // Actions from the native menu (or fallback shortcuts).
        let mut actions = match &self.native_menu {
            Some(m) => m.drain(),
            None => menu::egui_shortcuts(&ctx),
        };
        actions.append(&mut self.ribbon_actions);
        if self.dialog.is_some() {
            // A dialog is up: only quitting gets through.
            actions.retain(|a| *a == Action::Quit);
        }
        for a in actions {
            self.perform(a, &ctx);
        }

        // Input from the native toolbar and inspector.
        let native_events = self.native.as_ref().map(|n| n.drain()).unwrap_or_default();
        if !native_events.is_empty() && self.dialog.is_none() {
            let before = self.style_sig();
            for ev in native_events {
                self.apply_native_event(ev, &ctx);
            }
            if self.style_sig() != before
                && matches!(self.pending, Some(Pending { kind: PendingKind::Shape(_) | PendingKind::Text(_), .. }))
            {
                self.rerender_pending();
            }
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

        // Light/dark: follow the setting, or macOS when set to System.
        let dark = match self.theme_pref {
            theme::ThemePref::System => ctx.system_theme() == Some(egui::Theme::Dark),
            theme::ThemePref::Light => false,
            theme::ThemePref::Dark => true,
        };
        if dark != self.dark {
            self.dark = dark;
            theme::set_dark(&ctx, dark);
        }

        let title = format!("{}{} - Dabble", if self.is_dirty() { "*" } else { "" }, self.file_name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        let native = self.native.is_some();
        if !native {
            egui::Panel::top("ribbon")
                .frame(egui::Frame::NONE.fill(theme::ribbon_bg()).inner_margin(egui::Margin::symmetric(8, 4)))
                .show(ui, |ui| self.ribbon_ui(ui));
        }
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(egui::Frame::NONE.fill(theme::status_bg()).inner_margin(egui::Margin::symmetric(10, 0)))
            .show(ui, |ui| self.status_ui(ui));
        if self.show_layers && !native {
            egui::Panel::right("layers")
                .exact_size(240.0)
                .resizable(false)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::ribbon_bg())
                        .inner_margin(egui::Margin::symmetric(10, 4))
                        .stroke(egui::Stroke::new(1.0, theme::divider())),
                )
                .show(ui, |ui| self.layers_panel(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::canvas_area_bg()))
            .show(ui, |ui| self.canvas_ui(ui));
        self.dialog_ui(&ctx);

        #[cfg(target_os = "macos")]
        {
            self.sync_native();
            // Debug aid: DABBLE_SNAPSHOT=/path.png saves the AppKit layer of the window.
            self.frames += 1;
            if self.frames < 40 && std::env::var_os("DABBLE_SNAPSHOT").is_some() {
                ctx.request_repaint();
            }
            if self.frames == 40
                && let (Some(path), Some(w)) = (std::env::var_os("DABBLE_SNAPSHOT"), &self.ns_window)
            {
                let path = std::path::PathBuf::from(path);
                crate::mac_ui::snapshot(w, &path);
                if let Some(ui) = &self.native {
                    ui.snapshot_inspector(&path.with_extension("inspector.png"));
                }
            }
        }
    }
}
