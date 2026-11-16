//! The canvas: pointer handling for every tool, zoom/pan, and overlays
//! (marching ants, shape handles, text caret, brush outline).

use egui::{
    Color32, CursorIcon, Event, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
    Ui, pos2, vec2,
};
use tiny_skia::{IntRect, Pixmap};

use crate::app::{DabbleApp, Pending, PendingKind, SelectMode, ShapeState, TextState, Tool, ZOOM_STEPS};
use crate::menu::Action;
use crate::shapes::ShapeKind;
use crate::{raster, shapes, theme};

const HANDLE_PX: f32 = 7.0;

/// An in-progress pointer gesture.
pub enum Op {
    Idle,
    Pan { last: Pos2 },
    Pencil { last: (i32, i32), before: Pixmap, color: Color32 },
    Eraser { last: (i32, i32), before: Pixmap, replace_only: bool },
    Brush { stroke: crate::brushes::Stroke },
    ShapeDraw { start: Pos2 },
    ShapeHandle { handle: usize, min0: Pos2, max0: Pos2 },
    /// Dragging one point of a line, curve or polygon.
    Vertex { idx: usize },
    /// Bending a curve: the first bend moves both control points.
    CurveBend { first: bool },
    ShapeMove { grab: Pos2, orig: ShapeState },
    SelectRect { start: Pos2, end: Pos2 },
    SelectFree { points: Vec<Pos2> },
    MoveSelection { grab: Pos2, orig: (i32, i32) },
    TextBox { start: Pos2, end: Pos2 },
    TextMove { grab: Pos2, orig: Rect },
    /// Dragging a canvas edge handle (index into `handle_points`: 3, 4 or 5).
    CanvasResize { handle: usize, size: (u32, u32), off: egui::Vec2 },
    /// Dragging a selection handle: scales the selected pixels.
    SelectionHandle { handle: usize, min0: Pos2, max0: Pos2 },
}

/// The canvas edge handles that resize the canvas: right, bottom-right, bottom.
const CANVAS_HANDLES: [usize; 3] = [3, 4, 5];

/// Move one side or corner of a box to `q`, by handle index.
fn drag_handle(handle: usize, min0: Pos2, max0: Pos2, q: Pos2) -> (Pos2, Pos2) {
    let (mut mn, mut mx) = (min0, max0);
    match handle {
        0 => mn = q,
        1 => mn.y = q.y,
        2 => {
            mn.y = q.y;
            mx.x = q.x;
        }
        3 => mx.x = q.x,
        4 => mx = q,
        5 => mx.y = q.y,
        6 => {
            mn.x = q.x;
            mx.y = q.y;
        }
        _ => mn.x = q.x,
    }
    (mn, mx)
}

fn px(p: Pos2) -> (i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32)
}

/// Snap to pixel centers so 1px strokes stay crisp.
fn snap(p: Pos2) -> Pos2 {
    pos2(p.x.floor() + 0.5, p.y.floor() + 0.5)
}

fn handle_points(r: Rect) -> [Pos2; 8] {
    [
        r.left_top(),
        r.center_top(),
        r.right_top(),
        r.right_center(),
        r.right_bottom(),
        r.center_bottom(),
        r.left_bottom(),
        r.left_center(),
    ]
}

fn handle_cursor(i: usize) -> CursorIcon {
    match i {
        0 | 4 => CursorIcon::ResizeNwSe,
        2 | 6 => CursorIcon::ResizeNeSw,
        1 | 5 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeHorizontal,
    }
}

impl DabbleApp {
    pub fn canvas_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        // With the native interface, the toolbar and inspector float over the
        // canvas: keep the image clear of them.
        let (top, right) = self.native_insets();
        self.view.area = Rect::from_min_max(rect.min + vec2(0.0, top), rect.max - vec2(right, 0.0));
        if self.view.fit_next {
            self.fit();
            self.view.fit_next = false;
        }

        // Zoom (pinch / ⌘-scroll) and scroll-to-pan.
        if resp.contains_pointer() {
            let (zd, scroll, hover) = ctx.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos()));
            if (zd - 1.0).abs() > 1e-4 {
                let anchor = hover.unwrap_or(rect.center());
                self.zoom_at(self.view.zoom * zd, anchor);
            }
            self.view.pan += scroll;
        }

        if self.dialog.is_none() {
            self.handle_keys(&ctx);
            self.handle_pointer(&ctx, &resp);
        }
        self.clamp_pan();

        // Upload the document when it changed.
        if self.tex_dirty || self.tex.is_none() {
            let img = raster::to_color_image(&self.doc);
            let opts = canvas_texture_options();
            match &mut self.tex {
                Some(t) => t.set(img, opts),
                None => self.tex = Some(ctx.load_texture("canvas", img, opts)),
            }
            self.tex_dirty = false;
        }

        let painter = ui.painter_at(rect);
        let crect = self.view.rect_to_screen(Rect::from_min_size(
            Pos2::ZERO,
            vec2(self.doc.width() as f32, self.doc.height() as f32),
        ));
        // Soft shadow + checkerboard for transparency + the image.
        painter.add(egui::epaint::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(40) }
            .as_shape(crect, 0));
        painter.image(
            self.checker.id(),
            crect,
            Rect::from_min_max(Pos2::ZERO, pos2(crect.width() / 16.0, crect.height() / 16.0)),
            Color32::WHITE,
        );
        // Layers bottom to top; the GPU blends them with each layer's opacity.
        self.sync_layer_textures(&ctx);
        for (i, l) in self.layers.iter().enumerate() {
            if !l.visible {
                continue;
            }
            if let Some(t) = self.layer_texture(i) {
                let tint = Color32::from_white_alpha((l.opacity * 255.0).round() as u8);
                painter.image(t, crect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
            }
        }

        self.draw_overlays(&painter, &ctx);
        self.update_cursor(&ctx, &resp);
    }

    fn clamp_pan(&mut self) {
        let a = self.view.area;
        let size = vec2(self.doc.width() as f32, self.doc.height() as f32) * self.view.zoom;
        let margin = 24.0;
        let axis = |pan: f32, len: f32, avail: f32| {
            if len + 2.0 * margin <= avail {
                ((avail - len) / 2.0).round()
            } else {
                pan.clamp(avail - len - margin, margin)
            }
        };
        self.view.pan.x = axis(self.view.pan.x, size.x, a.width());
        self.view.pan.y = axis(self.view.pan.y, size.y, a.height());
    }

    // ------------------------------------------------------------ keyboard

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.focused().is_some()) {
            return; // a text field in the ribbon has focus
        }
        let events = ctx.input(|i| i.events.clone());
        let editing_text = matches!(self.pending, Some(Pending { kind: PendingKind::Text(_), .. }));
        if editing_text {
            for e in events {
                match e {
                    Event::Text(s) => self.text_insert(&s),
                    Event::Key { key, pressed: true, modifiers, .. } => self.text_key(key, modifiers),
                    _ => {}
                }
            }
            return;
        }
        for e in events {
            if let Event::Key { key, pressed: true, modifiers, .. } = e {
                if modifiers.command || modifiers.ctrl {
                    continue;
                }
                match key {
                    Key::Delete | Key::Backspace => self.perform(Action::Delete, ctx),
                    Key::Escape | Key::Enter => self.commit_pending(),
                    _ => {}
                }
            }
        }
    }

    fn text_key(&mut self, key: Key, _m: Modifiers) {
        if key == Key::Escape {
            self.commit_pending();
            return;
        }
        let Some(Pending { kind: PendingKind::Text(t), .. }) = &self.pending else { return };
        let (mut text, mut caret, rect) = (t.text.clone(), t.caret, t.rect);
        let vertical = |app: &mut Self, dir: f32, caret: usize| -> usize {
            let t = TextState { rect, text: text.clone(), caret };
            let Some((_, lay)) = app.text_layout_for(&t) else { return caret };
            let cur = lay.carets[caret];
            let target_y = cur.y + dir * lay.line_height;
            lay.carets
                .iter()
                .enumerate()
                .filter(|(_, p)| (p.y - target_y).abs() < lay.line_height * 0.5)
                .min_by(|a, b| (a.1.x - cur.x).abs().total_cmp(&(b.1.x - cur.x).abs()))
                .map_or(caret, |(i, _)| i)
        };
        match key {
            Key::Backspace if caret > 0 => {
                caret -= 1;
                text.remove(caret);
            }
            Key::Delete if caret < text.len() => {
                text.remove(caret);
            }
            Key::Enter => {
                text.insert(caret, '\n');
                caret += 1;
            }
            Key::ArrowLeft => caret = caret.saturating_sub(1),
            Key::ArrowRight => caret = (caret + 1).min(text.len()),
            Key::ArrowUp => caret = vertical(self, -1.0, caret),
            Key::ArrowDown => caret = vertical(self, 1.0, caret),
            Key::Home => caret = 0,
            Key::End => caret = text.len(),
            _ => return,
        }
        if let Some(Pending { kind: PendingKind::Text(t), .. }) = &mut self.pending {
            t.text = text;
            t.caret = caret;
        }
        self.rerender_pending();
    }

    // ------------------------------------------------------------- pointer

    fn handle_pointer(&mut self, ctx: &egui::Context, resp: &egui::Response) {
        let (events, mods, any_down, space, hover) = ctx.input(|i| {
            (i.events.clone(), i.modifiers, i.pointer.any_down(), i.key_down(Key::Space), i.pointer.hover_pos())
        });
        let over_canvas = resp.contains_pointer();
        self.cursor_px = hover.filter(|_| over_canvas).map(|h| px(self.view.to_canvas(h)));

        let mut gesture = self.gesture;
        let mut last_pos = hover;

        for e in events {
            match e {
                Event::PointerButton { pos, button, pressed: true, modifiers, .. } => {
                    if gesture.is_none() && over_canvas && self.view.area.contains(pos) {
                        let started = self.pointer_down(pos, button, modifiers, space);
                        if started {
                            gesture = Some(button);
                        }
                    }
                }
                Event::PointerMoved(pos) => {
                    last_pos = Some(pos);
                    if gesture.is_some() {
                        self.pointer_move(pos, mods);
                    }
                }
                Event::PointerButton { pos, button, pressed: false, .. }
                    if gesture == Some(button) => {
                        self.pointer_move(pos, mods);
                        self.pointer_up();
                        gesture = None;
                    }
                _ => {}
            }
        }
        // The airbrush keeps spraying while held still.
        if gesture.is_some()
            && let Op::Brush { stroke } = &mut self.op
            && stroke.kind.continuous()
        {
            let dt = ctx.input(|i| i.stable_dt).min(0.1);
            stroke.tick(&mut self.doc, dt);
            self.tex_dirty = true;
            ctx.request_repaint();
        }
        // Missed release (e.g. focus change): finish the gesture anyway.
        if gesture.is_some() && !any_down {
            if let Some(p) = last_pos {
                self.pointer_move(p, mods);
            }
            self.pointer_up();
            gesture = None;
        }
        self.gesture = gesture;
    }

    /// Returns true if a drag gesture started (so moves/release are tracked).
    fn pointer_down(&mut self, pos: Pos2, button: PointerButton, _mods: Modifiers, space: bool) -> bool {
        let editing_text = matches!(self.pending, Some(Pending { kind: PendingKind::Text(_), .. }));
        if button == PointerButton::Middle || (button == PointerButton::Primary && space && !editing_text) {
            self.op = Op::Pan { last: pos };
            return true;
        }
        let right = match button {
            PointerButton::Primary => false,
            PointerButton::Secondary => true,
            _ => return false,
        };
        let p = self.view.to_canvas(pos);
        if !right && let Some(handle) = self.canvas_handle_at(pos) {
            self.commit_pending();
            let (w, h) = (self.doc.width(), self.doc.height());
            let edge = handle_points(Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32)))[handle];
            self.op = Op::CanvasResize { handle, size: (w, h), off: edge - p };
            return true;
        }
        let (fg, _bg) = if right { (self.secondary, self.primary) } else { (self.primary, self.secondary) };

        match self.tool {
            Tool::Pencil => {
                let before = self.doc.clone();
                let (x, y) = px(p);
                raster::stamp_square(&mut self.doc, x, y, self.sizes.pencil as i32, raster::premul(fg));
                self.op = Op::Pencil { last: (x, y), before, color: fg };
                self.mark_changed();
            }
            Tool::Eraser => {
                let before = self.doc.clone();
                let (x, y) = px(p);
                self.erase_at(x, y, right);
                self.op = Op::Eraser { last: (x, y), before, replace_only: right };
                self.mark_changed();
            }
            Tool::Brush => {
                let mut stroke = crate::brushes::Stroke::new(self.brush_kind, fg, self.sizes.brush, self.doc.clone());
                stroke.add_point(&mut self.doc, p);
                self.op = Op::Brush { stroke };
                self.mark_changed();
            }
            Tool::Fill => {
                let before = self.doc.clone();
                let (x, y) = px(p);
                if raster::flood_fill(&mut self.doc, x, y, fg) {
                    self.history.record(self.active, &before, &self.doc);
                    self.mark_changed();
                }
                return false;
            }
            Tool::Picker => {
                // Picks what you see: all visible layers together.
                let flat = if self.layers.len() > 1 { Some(self.flatten()) } else { None };
                if let Some(c) = raster::pixel(flat.as_ref().unwrap_or(&self.doc), px(p).0, px(p).1) {
                    let [r, g, b, _] = c.to_srgba_unmultiplied();
                    let c = Color32::from_rgb(r, g, b);
                    if right { self.secondary = c } else { self.primary = c }
                    let back = self.prev_tool;
                    self.tool = back;
                }
                return false;
            }
            Tool::Magnifier => {
                let z = self.view.zoom;
                let next = if right {
                    ZOOM_STEPS.iter().rev().copied().find(|s| *s < z - 1e-3)
                } else {
                    ZOOM_STEPS.iter().copied().find(|s| *s > z + 1e-3)
                };
                if let Some(n) = next {
                    self.zoom_at(n, pos);
                }
                return false;
            }
            Tool::Select => {
                if let Some(handle) = self.selection_handle_at(pos) {
                    let r = self.pending_selection().unwrap().current_rect();
                    self.op = Op::SelectionHandle { handle, min0: r.min, max0: r.max };
                    return true;
                }
                if let Some(s) = self.pending_selection()
                    && s.current_rect().contains(p) {
                        let orig = s.pos;
                        self.lift_selection();
                        self.op = Op::MoveSelection { grab: p, orig };
                        return true;
                    }
                self.commit_pending();
                self.op = match self.select_mode {
                    SelectMode::Rect => Op::SelectRect { start: p, end: p },
                    SelectMode::Free => Op::SelectFree { points: vec![p] },
                };
            }
            Tool::Shape => {
                if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &mut self.pending {
                    if s.kind.by_points() {
                        let verts = s.vertices();
                        let near = |q: Pos2| self.view.to_screen(q).distance(pos) <= HANDLE_PX;
                        // Clicking the first corner closes a polygon.
                        if s.kind == ShapeKind::Polygon && s.stage == 1 && verts.len() > 2 && near(verts[0]) {
                            s.stage = 2;
                            self.rerender_pending();
                            return false;
                        }
                        if let Some(idx) = verts.iter().position(|v| near(*v)) {
                            self.op = Op::Vertex { idx };
                            return true;
                        }
                        match (s.kind, s.stage) {
                            (ShapeKind::Curve, 1 | 2) => {
                                let q = snap(p);
                                let first = s.stage == 1;
                                if first {
                                    s.pts = vec![q, q];
                                } else {
                                    s.pts[1] = q;
                                }
                                self.op = Op::CurveBend { first };
                                self.rerender_pending();
                                return true;
                            }
                            (ShapeKind::Polygon, 1) => {
                                s.pts.push(snap(p));
                                self.op = Op::Vertex { idx: s.pts.len() - 1 };
                                self.rerender_pending();
                                return true;
                            }
                            _ => {}
                        }
                    } else {
                        let bb = s.bbox();
                        for (i, h) in handle_points(bb).into_iter().enumerate() {
                            if self.view.to_screen(h).distance(pos) <= HANDLE_PX {
                                self.op = Op::ShapeHandle { handle: i, min0: bb.min, max0: bb.max };
                                return true;
                            }
                        }
                    }
                    let snapshot = s.clone();
                    if self.shape_hit(&snapshot, p) {
                        self.op = Op::ShapeMove { grab: p, orig: snapshot };
                        return true;
                    }
                }
                self.commit_pending();
                let a = snap(p);
                self.pending = Some(Pending {
                    base: self.doc.clone(),
                    kind: PendingKind::Shape(ShapeState::new(self.shape_kind, a, right)),
                });
                self.op = Op::ShapeDraw { start: a };
            }
            Tool::Text => {
                if let Some(t) = self.pending_text() {
                    let r = self.text_box_rect(&t);
                    if r.expand(4.0 / self.view.zoom).contains(p) {
                        self.op = Op::TextMove { grab: p, orig: t.rect };
                        return true;
                    }
                }
                self.commit_pending();
                self.op = Op::TextBox { start: p, end: p };
            }
        }
        true
    }

    fn pointer_move(&mut self, pos: Pos2, mods: Modifiers) {
        let p = self.view.to_canvas(pos);
        let mut op = std::mem::replace(&mut self.op, Op::Idle);
        match &mut op {
            Op::Idle => {}
            Op::Pan { last } => {
                self.view.pan += pos - *last;
                *last = pos;
            }
            Op::Pencil { last, color, .. } => {
                let (x, y) = px(p);
                let c = raster::premul(*color);
                let size = self.sizes.pencil as i32;
                let doc = &mut self.doc;
                raster::walk_line(last.0, last.1, x, y, |xx, yy| raster::stamp_square(doc, xx, yy, size, c));
                *last = (x, y);
                self.mark_changed();
            }
            Op::Eraser { last, replace_only, .. } => {
                let (x, y) = px(p);
                let mut pts = vec![];
                raster::walk_line(last.0, last.1, x, y, |xx, yy| pts.push((xx, yy)));
                for (xx, yy) in pts {
                    self.erase_at(xx, yy, *replace_only);
                }
                *last = (x, y);
                self.mark_changed();
            }
            Op::Brush { stroke } => {
                stroke.add_point(&mut self.doc, p);
                self.mark_changed();
            }
            Op::ShapeDraw { start } => {
                if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &mut self.pending {
                    let mut b = snap(p);
                    if mods.shift {
                        b = shapes::constrain(s.kind, *start, b);
                    }
                    s.b = b;
                }
                self.rerender_pending();
            }
            Op::ShapeHandle { handle, min0, max0 } => {
                let (mn, mx) = drag_handle(*handle, *min0, *max0, snap(p));
                if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &mut self.pending {
                    s.a = mn;
                    s.b = mx;
                }
                self.rerender_pending();
            }
            Op::Vertex { idx } => {
                if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &mut self.pending {
                    let mut q = snap(p);
                    let verts = s.vertices();
                    // Shift snaps a line end (or polygon corner) to 45° from its neighbor.
                    if mods.shift && *idx < verts.len() {
                        let other = match s.kind {
                            ShapeKind::Polygon => verts[if *idx == 0 { verts.len() - 1 } else { *idx - 1 }],
                            _ if *idx == 0 => verts[1],
                            _ if *idx == 1 => verts[0],
                            _ => q,
                        };
                        q = shapes::constrain(ShapeKind::Line, other, q);
                    }
                    s.set_vertex(*idx, q);
                }
                self.rerender_pending();
            }
            Op::CurveBend { first } => {
                if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &mut self.pending {
                    let q = snap(p);
                    if *first {
                        s.pts = vec![q, q];
                    } else {
                        s.pts[1] = q;
                    }
                }
                self.rerender_pending();
            }
            Op::ShapeMove { grab, orig } => {
                let d = (p - *grab).round();
                if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &mut self.pending {
                    *s = orig.clone();
                    s.translate(d);
                }
                self.rerender_pending();
            }
            Op::SelectRect { start, end } => {
                *end = p;
                if mods.shift {
                    *end = shapes::constrain(shapes::ShapeKind::Rect, *start, p);
                }
            }
            Op::SelectFree { points } => {
                if points.last().is_none_or(|l| l.distance(p) >= 0.5 / self.view.zoom.max(0.5)) {
                    points.push(p);
                }
            }
            Op::MoveSelection { grab, orig } => {
                let d = (p - *grab).round();
                if let Some(Pending { kind: PendingKind::Selection(s), .. }) = &mut self.pending {
                    s.pos = (orig.0 + d.x as i32, orig.1 + d.y as i32);
                }
                self.rerender_pending();
            }
            Op::TextBox { end, .. } => *end = p,
            Op::CanvasResize { handle, size, off } => {
                let p = p + *off;
                let clamp = |v: f32| (v.round().max(1.0) as u32).min(raster::MAX_DIM);
                if *handle != 5 {
                    size.0 = clamp(p.x);
                }
                if *handle != 3 {
                    size.1 = clamp(p.y);
                }
            }
            Op::SelectionHandle { handle, min0, max0 } => {
                let (mn, mx) = drag_handle(*handle, *min0, *max0, p.round());
                let r = Rect::from_two_pos(mn, mx);
                let (w, h) = (r.width().max(1.0) as u32, r.height().max(1.0) as u32);
                let cur = self.pending_selection().map(|s| (s.pos, s.rect.width(), s.rect.height()));
                let pos = (r.min.x as i32, r.min.y as i32);
                if cur != Some((pos, w, h)) {
                    self.scale_selection(pos, w, h);
                }
            }
            Op::TextMove { grab, orig } => {
                let d = (p - *grab).round();
                if let Some(Pending { kind: PendingKind::Text(t), .. }) = &mut self.pending {
                    t.rect = orig.translate(d);
                }
                self.rerender_pending();
            }
        }
        self.op = op;
    }

    fn pointer_up(&mut self) {
        let op = std::mem::replace(&mut self.op, Op::Idle);
        match op {
            Op::Pencil { before, .. } | Op::Eraser { before, .. } => {
                self.history.record(self.active, &before, &self.doc);
            }
            Op::Brush { stroke } => {
                self.history.record(self.active, &stroke.before, &self.doc);
            }
            Op::ShapeDraw { .. } => {
                if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &mut self.pending {
                    if s.a.distance(s.b) < 1.0 {
                        // A click without a drag draws nothing.
                        self.cancel_pending();
                    } else if s.kind == ShapeKind::Polygon {
                        s.pts = vec![s.a, s.b];
                        s.stage = 1;
                    } else if s.kind == ShapeKind::Curve {
                        s.stage = 1;
                    }
                }
            }
            Op::CurveBend { .. } => {
                if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &mut self.pending {
                    s.stage += 1;
                }
            }
            Op::SelectRect { start, end } => {
                let w = self.doc.width() as f32;
                let h = self.doc.height() as f32;
                let r = Rect::from_two_pos(start, end);
                let (x0, y0) = (r.min.x.round().clamp(0.0, w), r.min.y.round().clamp(0.0, h));
                let (x1, y1) = (r.max.x.round().clamp(0.0, w), r.max.y.round().clamp(0.0, h));
                if let Some(ir) = IntRect::from_ltrb(x0 as i32, y0 as i32, x1 as i32, y1 as i32) {
                    self.start_selection(ir, None, None);
                }
            }
            Op::SelectFree { points } => self.finish_free_selection(points),
            Op::CanvasResize { size, .. } => self.set_canvas_size(size.0, size.1),
            Op::TextBox { start, end } => {
                let mut r = Rect::from_two_pos(start, end);
                let line = self.text_style.px() * 1.3;
                if r.width() < 8.0 {
                    // Click: a box that reaches toward the right edge.
                    let room = self.doc.width() as f32 - r.min.x;
                    r = Rect::from_min_size(r.min, vec2(room.clamp(40.0, 320.0), line));
                }
                r.max.y = r.max.y.max(r.min.y + line);
                let r = Rect::from_min_max(r.min.round(), r.max.round());
                self.ensure_fonts();
                self.pending = Some(Pending {
                    base: self.doc.clone(),
                    kind: PendingKind::Text(TextState { rect: r, text: vec![], caret: 0 }),
                });
            }
            _ => {}
        }
        self.mark_changed();
    }

    fn canvas_rect_screen(&self) -> Rect {
        self.view.rect_to_screen(Rect::from_min_size(
            Pos2::ZERO,
            vec2(self.doc.width() as f32, self.doc.height() as f32),
        ))
    }

    /// Screen position of each canvas-resize handle, with its index.
    fn canvas_handles(&self) -> [(usize, Pos2); 3] {
        let pts = handle_points(self.canvas_rect_screen().expand(4.0));
        CANVAS_HANDLES.map(|i| (i, pts[i]))
    }

    /// A canvas handle under `pos`, unless a pending shape/selection handle is there too.
    fn canvas_handle_at(&self, pos: Pos2) -> Option<usize> {
        let near = |p: Pos2| self.view.to_screen(p).distance(pos) <= HANDLE_PX;
        let object_handle = match &self.pending {
            Some(Pending { kind: PendingKind::Selection(s), .. }) => handle_points(s.current_rect()).into_iter().any(near),
            Some(Pending { kind: PendingKind::Shape(s), .. }) => {
                s.vertices().into_iter().chain(handle_points(s.bbox())).any(near)
            }
            _ => false,
        };
        if object_handle {
            return None;
        }
        self.canvas_handles().into_iter().find(|(_, h)| h.distance(pos) <= HANDLE_PX).map(|(i, _)| i)
    }

    fn selection_handle_at(&self, pos: Pos2) -> Option<usize> {
        if self.tool != Tool::Select {
            return None;
        }
        // Smaller reach than shape handles so small selections can still be grabbed.
        let r = self.view.rect_to_screen(self.pending_selection()?.current_rect());
        handle_points(r)
            .into_iter()
            .enumerate()
            .map(|(i, h)| (i, h.distance(pos)))
            .filter(|(_, d)| *d <= 5.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    fn finish_free_selection(&mut self, points: Vec<Pos2>) {
        if points.len() < 3 {
            return;
        }
        let w = self.doc.width() as f32;
        let h = self.doc.height() as f32;
        let pts: Vec<Pos2> = points.iter().map(|p| pos2(p.x.clamp(0.0, w), p.y.clamp(0.0, h))).collect();
        let bb = Rect::from_points(&pts);
        let x0 = bb.min.x.floor() as i32;
        let y0 = bb.min.y.floor() as i32;
        let x1 = (bb.max.x.ceil() as i32).min(w as i32);
        let y1 = (bb.max.y.ceil() as i32).min(h as i32);
        let Some(ir) = IntRect::from_ltrb(x0, y0, x1.max(x0 + 1), y1.max(y0 + 1)) else { return };
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(pts[0].x, pts[0].y);
        for p in &pts[1..] {
            pb.line_to(p.x, p.y);
        }
        pb.close();
        let Some(path) = pb.finish() else { return };
        let Some(mut mask) = tiny_skia::Mask::new(ir.width(), ir.height()) else { return };
        mask.fill_path(
            &path,
            tiny_skia::FillRule::Winding,
            false,
            tiny_skia::Transform::from_translate(-x0 as f32, -y0 as f32),
        );
        if mask.data().iter().all(|&m| m == 0) {
            return;
        }
        let rel: Vec<Pos2> = pts.iter().map(|p| pos2(p.x - x0 as f32, p.y - y0 as f32)).collect();
        self.start_selection(ir, Some(rel), Some(mask.data().to_vec()));
    }

    fn erase_at(&mut self, x: i32, y: i32, replace_only: bool) {
        let size = self.sizes.eraser as i32;
        if replace_only {
            // Right-drag: only replace Color 1 with Color 2 (classic Paint behavior).
            let from = raster::premul(self.primary);
            let to = raster::premul(self.secondary);
            let half = (size - 1) / 2;
            let w = self.doc.width() as i32;
            let h = self.doc.height() as i32;
            let pxs = self.doc.pixels_mut();
            for yy in (y - half).max(0)..(y - half + size).min(h) {
                for xx in (x - half).max(0)..(x - half + size).min(w) {
                    let i = (yy * w + xx) as usize;
                    if pxs[i] == from {
                        pxs[i] = to;
                    }
                }
            }
        } else {
            let c = raster::premul(self.erase_color());
            raster::stamp_square(&mut self.doc, x, y, size, c);
        }
    }

    fn shape_hit(&self, s: &ShapeState, p: Pos2) -> bool {
        if s.kind.is_open() {
            // Near the drawn path.
            let reach = (self.sizes.shape / 2.0).max(5.0 / self.view.zoom);
            let lines = s.path().map(|path| shapes::flatten(&path)).unwrap_or_default();
            lines.iter().any(|l| {
                l.windows(2).any(|w| {
                    let (a, b) = (w[0], w[1]);
                    let ab = b - a;
                    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    (a + ab * t).distance(p) <= reach
                })
            })
        } else {
            s.bbox().contains(p)
        }
    }

    pub fn text_box_rect(&mut self, t: &TextState) -> Rect {
        let h = self.text_layout_for(t).map_or(0.0, |(_, l)| l.height);
        t.rect.with_max_y(t.rect.min.y + t.rect.height().max(h))
    }

    // ------------------------------------------------------------ overlays

    fn draw_overlays(&mut self, painter: &egui::Painter, ctx: &egui::Context) {
        let v = &self.view;
        let ants = |painter: &egui::Painter, pts: &[Pos2]| {
            painter.add(Shape::line(pts.to_vec(), Stroke::new(1.0, Color32::WHITE)));
            painter.extend(Shape::dashed_line(pts, Stroke::new(1.0, Color32::from_gray(30)), 4.0, 4.0));
        };
        let rect_pts = |r: Rect| vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        let handle = |painter: &egui::Painter, c: Pos2| {
            let r = Rect::from_center_size(c, vec2(7.0, 7.0));
            painter.rect_filled(r, 1.0, Color32::WHITE);
            painter.rect_stroke(r, 1.0, Stroke::new(1.0, theme::accent()), StrokeKind::Inside);
        };

        match &self.op {
            Op::SelectRect { start, end } => ants(painter, &rect_pts(v.rect_to_screen(Rect::from_two_pos(*start, *end)))),
            Op::SelectFree { points } => {
                let s: Vec<Pos2> = points.iter().map(|p| v.to_screen(*p)).collect();
                if s.len() > 1 {
                    ants(painter, &s);
                }
            }
            Op::TextBox { start, end } => ants(painter, &rect_pts(v.rect_to_screen(Rect::from_two_pos(*start, *end)))),
            _ => {}
        }

        // Canvas resize handles, and the new outline while dragging one.
        if let Op::CanvasResize { size, .. } = &self.op {
            let r = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
            ants(painter, &rect_pts(v.rect_to_screen(r)));
        } else {
            for (_, h) in self.canvas_handles() {
                let r = Rect::from_center_size(h, vec2(7.0, 7.0));
                painter.rect_filled(r, 1.0, Color32::WHITE);
                painter.rect_stroke(r, 1.0, Stroke::new(1.0, Color32::from_gray(110)), StrokeKind::Inside);
            }
        }

        let text_info = self.pending_text();
        match &self.pending {
            Some(Pending { kind: PendingKind::Selection(s), .. }) => {
                let r = s.current_rect();
                match &s.outline {
                    Some(o) => {
                        let off = r.min.to_vec2();
                        let mut pts: Vec<Pos2> = o.iter().map(|p| v.to_screen(*p + off)).collect();
                        if let Some(&f) = pts.first() {
                            pts.push(f);
                        }
                        ants(painter, &pts);
                    }
                    None => ants(painter, &rect_pts(v.rect_to_screen(r))),
                }
                if self.tool == Tool::Select && !matches!(self.op, Op::MoveSelection { .. }) {
                    for h in handle_points(v.rect_to_screen(r)) {
                        handle(painter, h);
                    }
                }
            }
            Some(Pending { kind: PendingKind::Shape(s), .. }) => {
                if s.kind.by_points() {
                    let verts = s.vertices();
                    // Guide lines from each curve end to its control point.
                    if s.kind == ShapeKind::Curve && verts.len() == 4 {
                        let guide = Stroke::new(1.0, theme::accent().gamma_multiply(0.6));
                        painter.line_segment([v.to_screen(verts[0]), v.to_screen(verts[2])], guide);
                        painter.line_segment([v.to_screen(verts[1]), v.to_screen(verts[3])], guide);
                    }
                    for p in verts {
                        handle(painter, v.to_screen(p));
                    }
                } else {
                    let r = v.rect_to_screen(s.bbox());
                    ants(painter, &rect_pts(r));
                    for h in handle_points(r) {
                        handle(painter, h);
                    }
                }
            }
            _ => {}
        }
        if let Some(t) = text_info {
            let br = self.text_box_rect(&t);
            let v = &self.view;
            ants(painter, &rect_pts(v.rect_to_screen(br).expand(1.0)));
            if let Some((_, lay)) = self.text_layout_for(&t) {
                let c = lay.carets[t.caret.min(lay.carets.len() - 1)];
                let top = self.view.to_screen(t.rect.min + c.to_vec2());
                let bottom = top + vec2(0.0, lay.line_height * self.view.zoom);
                let blink = (ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0;
                if blink {
                    painter.line_segment([top, bottom], Stroke::new(1.5, Color32::BLACK));
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }

        // Brush/eraser footprint under the cursor.
        if let (Some((x, y)), true) = (self.cursor_px, matches!(self.op, Op::Idle | Op::Brush { .. } | Op::Eraser { .. })) {
            let z = self.view.zoom;
            match self.tool {
                Tool::Brush if self.sizes.brush * z >= 4.0 => {
                    let hover = ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
                    painter.circle_stroke(hover, self.sizes.brush * z / 2.0, Stroke::new(1.0, Color32::from_gray(90)));
                }
                Tool::Eraser => {
                    let s = self.sizes.eraser;
                    let half = ((s as i32 - 1) / 2) as f32;
                    let r = Rect::from_min_size(pos2(x as f32 - half, y as f32 - half), vec2(s, s));
                    let sr = self.view.rect_to_screen(r);
                    painter.rect_filled(sr, 0.0, self.erase_color());
                    painter.rect_stroke(sr, 0.0, Stroke::new(1.0, Color32::from_gray(60)), StrokeKind::Outside);
                }
                _ => {}
            }
        }
    }

    fn update_cursor(&mut self, ctx: &egui::Context, resp: &egui::Response) {
        if !resp.contains_pointer() && matches!(self.op, Op::Idle) {
            return;
        }
        let hover = ctx.input(|i| i.pointer.hover_pos());
        let space = ctx.input(|i| i.key_down(Key::Space));
        let icon = match &self.op {
            Op::Pan { .. } => CursorIcon::Grabbing,
            Op::MoveSelection { .. } | Op::ShapeMove { .. } | Op::TextMove { .. } => CursorIcon::Move,
            Op::ShapeHandle { handle, .. }
            | Op::CanvasResize { handle, .. }
            | Op::SelectionHandle { handle, .. } => handle_cursor(*handle),
            _ => {
                let Some(h) = hover else { return };
                let p = self.view.to_canvas(h);
                let editing_text = matches!(self.pending, Some(Pending { kind: PendingKind::Text(_), .. }));
                if space && !editing_text {
                    CursorIcon::Grab
                } else if let Some(i) = self.canvas_handle_at(h).or_else(|| self.selection_handle_at(h)) {
                    handle_cursor(i)
                } else {
                    match self.tool {
                        Tool::Select if self.pending_selection().is_some_and(|s| s.current_rect().contains(p)) => {
                            CursorIcon::Move
                        }
                        Tool::Shape => self.shape_cursor(h, p),
                        Tool::Text => {
                            let in_box = match self.pending_text() {
                                Some(t) => self.text_box_rect(&t).expand(4.0 / self.view.zoom).contains(p),
                                None => false,
                            };
                            if in_box { CursorIcon::Move } else { CursorIcon::Text }
                        }
                        Tool::Magnifier => CursorIcon::ZoomIn,
                        Tool::Brush if self.sizes.brush * self.view.zoom >= 4.0 => CursorIcon::None,
                        Tool::Eraser => CursorIcon::None,
                        _ => CursorIcon::Crosshair,
                    }
                }
            }
        };
        ctx.set_cursor_icon(icon);
    }

    fn shape_cursor(&self, h: Pos2, p: Pos2) -> CursorIcon {
        if let Some(Pending { kind: PendingKind::Shape(s), .. }) = &self.pending {
            if s.kind.by_points() {
                if s.vertices().iter().any(|e| self.view.to_screen(*e).distance(h) <= HANDLE_PX) {
                    return CursorIcon::Grab;
                }
                // Mid-edit curves and polygons take more clicks.
                if matches!((s.kind, s.stage), (ShapeKind::Curve, 1 | 2) | (ShapeKind::Polygon, 1)) {
                    return CursorIcon::Crosshair;
                }
            } else {
                for (i, hp) in handle_points(s.bbox()).into_iter().enumerate() {
                    if self.view.to_screen(hp).distance(h) <= HANDLE_PX {
                        return handle_cursor(i);
                    }
                }
            }
            if self.shape_hit(s, p) {
                return CursorIcon::Move;
            }
        }
        CursorIcon::Crosshair
    }

    // ---------------------------------------------------------- status bar

    pub fn status_ui(&mut self, ui: &mut Ui) {
        use egui_phosphor::regular as ic;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let small = |ui: &mut Ui, s: String| {
                ui.label(egui::RichText::new(s).size(12.0).color(theme::subtle_text()));
            };
            small(ui, ic::CURSOR.to_string());
            small(ui, self.cursor_px.map(|(x, y)| format!("{x}, {y}px")).unwrap_or_default());
            ui.add_space(16.0);
            let sel = match (&self.op, &self.pending) {
                (Op::SelectRect { start, end }, _) | (Op::TextBox { start, end }, _) => {
                    let r = Rect::from_two_pos(*start, *end);
                    Some((r.width().round() as i32, r.height().round() as i32))
                }
                (_, Some(Pending { kind: PendingKind::Selection(s), .. })) => {
                    Some((s.rect.width() as i32, s.rect.height() as i32))
                }
                (_, Some(Pending { kind: PendingKind::Shape(s), .. })) => {
                    let r = s.bbox();
                    Some((r.width().round() as i32, r.height().round() as i32))
                }
                _ => None,
            };
            small(ui, ic::SELECTION.to_string());
            small(ui, sel.map(|(w, h)| format!("{w} × {h}px")).unwrap_or_default());
            ui.add_space(16.0);
            small(ui, ic::FRAME_CORNERS.to_string());
            let size = match &self.op {
                Op::CanvasResize { size, .. } => *size,
                _ => (self.doc.width(), self.doc.height()),
            };
            small(ui, format!("{} × {}px", size.0, size.1));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(ic::CORNERS_OUT).on_hover_text("Fit to window (⌘9)").clicked() {
                    self.view.fit_next = true;
                }
                if ui.small_button(ic::PLUS).on_hover_text("Zoom in (⌘=)").clicked() {
                    self.zoom_step(1);
                }
                let mut z = self.view.zoom.ln();
                let lo = ZOOM_STEPS[0].ln();
                let hi = ZOOM_STEPS[ZOOM_STEPS.len() - 1].ln();
                let r = ui.add_sized([120.0, 16.0], egui::Slider::new(&mut z, lo..=hi).show_value(false));
                if r.changed() {
                    let c = self.view.area.center();
                    self.zoom_at(z.exp(), c);
                }
                if ui.small_button(ic::MINUS).on_hover_text("Zoom out (⌘-)").clicked() {
                    self.zoom_step(-1);
                }
                small(ui, format!("{:.0}%", self.view.zoom * 100.0));
            });
        });
    }
}

pub fn canvas_texture_options() -> egui::TextureOptions {
    egui::TextureOptions {
        magnification: egui::TextureFilter::Nearest,
        minification: egui::TextureFilter::Linear,
        wrap_mode: egui::TextureWrapMode::ClampToEdge,
        mipmap_mode: None,
    }
}
