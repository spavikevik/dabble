//! Headless interaction tests: run real egui frames with synthetic input
//! and check the resulting pixels.

use egui::{Color32, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

use crate::app::{DaubApp, PendingKind, SelectMode, Tool};
use crate::raster;
use crate::shapes::ShapeKind;

struct H {
    ctx: egui::Context,
    app: DaubApp,
    t: f64,
    mods: Modifiers,
}

const RED: Color32 = Color32::from_rgb(220, 20, 30);

impl H {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let mut app = DaubApp::create(&ctx, None, false);
        app.clipboard = None; // never touch the real clipboard from tests
        let mut h = H { ctx, app, t: 0.0, mods: Modifiers::NONE };
        h.app.replace_doc(raster::new_canvas(200, 150, Color32::WHITE), None);
        h.frame(vec![]);
        h.frame(vec![]);
        assert_eq!(h.app.view.zoom, 1.0);
        h
    }

    fn frame(&mut self, mut events: Vec<Event>) {
        events.insert(0, Event::ModifiersChanged(self.mods));
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 840.0))),
            events,
            time: Some(self.t),
            ..Default::default()
        };
        self.t += 1.0 / 60.0;
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.frame_ui(ui));
        out.textures_delta.clear();
    }

    fn s(&self, x: f32, y: f32) -> Pos2 {
        self.app.view.to_screen(pos2(x, y))
    }

    fn button(&mut self, x: f32, y: f32, b: PointerButton, pressed: bool) {
        let pos = self.s(x, y);
        let m = self.mods;
        self.frame(vec![
            Event::PointerMoved(pos),
            Event::PointerButton { pos, button: b, pressed, modifiers: m },
        ]);
    }

    fn drag(&mut self, from: (f32, f32), to: (f32, f32), b: PointerButton) {
        self.frame(vec![Event::PointerMoved(self.s(from.0, from.1))]);
        self.button(from.0, from.1, b, true);
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let p = self.s(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            self.frame(vec![Event::PointerMoved(p)]);
        }
        self.button(to.0, to.1, b, false);
    }

    fn click(&mut self, x: f32, y: f32, b: PointerButton) {
        self.frame(vec![Event::PointerMoved(self.s(x, y))]);
        self.button(x, y, b, true);
        self.button(x, y, b, false);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }]);
        self.frame(vec![Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers,
        }]);
    }

    fn px(&self, x: i32, y: i32) -> Color32 {
        raster::pixel(&self.app.doc, x, y).unwrap()
    }
}

const L: PointerButton = PointerButton::Primary;
const R: PointerButton = PointerButton::Secondary;

#[test]
fn pencil_draws_and_undo_redo() {
    let mut h = H::new();
    h.app.set_tool(Tool::Pencil);
    h.drag((10.5, 10.5), (40.5, 10.5), L);
    for x in 10..=40 {
        assert_eq!(h.px(x, 10), Color32::BLACK, "x={x}");
    }
    assert_eq!(h.px(10, 11), Color32::WHITE);
    assert!(h.app.is_dirty());
    h.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(h.px(20, 10), Color32::WHITE);
    assert!(!h.app.is_dirty());
    h.key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(h.px(20, 10), Color32::BLACK);
}

#[test]
fn right_button_uses_color_two() {
    let mut h = H::new();
    h.app.secondary = RED;
    h.app.set_tool(Tool::Brush);
    h.app.sizes.brush = 6.0;
    h.drag((20.0, 50.0), (80.0, 50.0), R);
    assert_eq!(h.px(50, 50), RED);
}

#[test]
fn fill_bucket_floods_region() {
    let mut h = H::new();
    h.app.primary = RED;
    h.app.set_tool(Tool::Fill);
    h.click(5.0, 5.0, L);
    assert_eq!(h.px(0, 0), RED);
    assert_eq!(h.px(199, 149), RED);
}

#[test]
fn eraser_paints_color_two_and_right_drag_replaces_only_color_one() {
    let mut h = H::new();
    h.app.primary = RED;
    h.app.set_tool(Tool::Fill);
    h.click(5.0, 5.0, L);
    h.app.set_tool(Tool::Eraser);
    h.app.sizes.eraser = 4.0;
    h.drag((20.5, 20.5), (60.5, 20.5), L);
    assert_eq!(h.px(40, 20), Color32::WHITE);
    // Right-drag replaces red (Color 1) with white (Color 2) only.
    h.app.primary = Color32::BLACK;
    h.drag((20.5, 40.5), (60.5, 40.5), R);
    assert_eq!(h.px(40, 40), RED, "red is not Color 1 now, so it stays");
    h.app.primary = RED;
    h.drag((20.5, 60.5), (60.5, 60.5), R);
    assert_eq!(h.px(40, 60), Color32::WHITE, "Color 1 replaced by Color 2");
}

#[test]
fn picker_sets_color_and_returns_to_previous_tool() {
    let mut h = H::new();
    raster::set_px(&mut h.app.doc, 30, 30, raster::premul(RED));
    h.app.set_tool(Tool::Brush);
    h.app.set_tool(Tool::Picker);
    h.click(30.5, 30.5, L);
    assert_eq!(h.app.primary, RED);
    assert_eq!(h.app.tool, Tool::Brush);
}

#[test]
fn rect_selection_moves_pixels_and_undoes_in_one_step() {
    let mut h = H::new();
    raster::set_px(&mut h.app.doc, 10, 10, raster::premul(RED));
    h.app.set_tool(Tool::Select);
    h.drag((5.0, 5.0), (20.0, 20.0), L);
    let sel = h.app.pending_selection().expect("selection");
    assert_eq!((sel.rect.x(), sel.rect.y(), sel.rect.width(), sel.rect.height()), (5, 5, 15, 15));
    h.drag((10.0, 10.0), (60.0, 60.0), L);
    assert_eq!(h.px(60, 60), RED);
    assert_eq!(h.px(10, 10), Color32::WHITE);
    h.key(Key::Escape, Modifiers::NONE);
    assert!(h.app.pending.is_none());
    assert_eq!(h.px(60, 60), RED);
    h.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(h.px(10, 10), RED);
    assert_eq!(h.px(60, 60), Color32::WHITE);
}

#[test]
fn delete_fills_selection_with_color_two() {
    let mut h = H::new();
    h.app.primary = RED;
    h.app.set_tool(Tool::Fill);
    h.click(5.0, 5.0, L);
    h.app.set_tool(Tool::Select);
    h.drag((10.0, 10.0), (30.0, 30.0), L);
    h.key(Key::Delete, Modifiers::NONE);
    assert_eq!(h.px(20, 20), Color32::WHITE);
    assert_eq!(h.px(40, 40), RED);
}

#[test]
fn free_form_selection_only_moves_inside_the_outline() {
    let mut h = H::new();
    h.app.primary = RED;
    h.app.set_tool(Tool::Fill);
    h.click(5.0, 5.0, L);
    h.app.select_mode = SelectMode::Free;
    h.app.set_tool(Tool::Select);
    // Triangle (10,10) (50,10) (10,50): corner (45,45) is outside it.
    let pts = [(10.0, 10.0), (50.0, 10.0), (10.0, 50.0), (10.0, 10.0)];
    h.frame(vec![Event::PointerMoved(h.s(pts[0].0, pts[0].1))]);
    h.button(pts[0].0, pts[0].1, L, true);
    for w in pts.windows(2) {
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let p = h.s(w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t);
            h.frame(vec![Event::PointerMoved(p)]);
        }
    }
    h.button(10.0, 10.0, L, false);
    let sel = h.app.pending_selection().expect("free-form selection");
    assert!(sel.mask.is_some());
    h.drag((15.0, 15.0), (115.0, 85.0), L);
    assert_eq!(h.px(15, 15), Color32::WHITE, "inside triangle: lifted, Color 2 left behind");
    assert_eq!(h.px(45, 45), RED, "outside triangle: untouched");
}

#[test]
fn shape_stays_editable_then_commits() {
    let mut h = H::new();
    h.app.shape_kind = ShapeKind::Rect;
    h.app.sizes.shape = 1.0;
    h.app.set_tool(Tool::Shape);
    h.drag((20.0, 20.0), (80.0, 60.0), L);
    assert!(matches!(h.app.pending.as_ref().map(|p| &p.kind), Some(PendingKind::Shape(_))));
    assert_eq!(h.px(50, 20), Color32::BLACK, "top edge");
    assert_eq!(h.px(50, 40), Color32::WHITE, "inside, no fill");
    // Drag the bottom-right handle out.
    h.drag((80.5, 60.5), (120.0, 100.0), L);
    assert_eq!(h.px(120, 70), Color32::BLACK, "right edge moved");
    assert_eq!(h.px(80, 40), Color32::WHITE, "old right edge gone");
    // Colors and fill update the pending shape live.
    h.app.shape_fill = true;
    h.app.secondary = RED;
    h.frame(vec![]);
    // style changes are picked up by the ribbon diff; force it like the ribbon does
    h.app.rerender_pending();
    assert_eq!(h.px(60, 60), RED);
    h.key(Key::Enter, Modifiers::NONE);
    assert!(h.app.pending.is_none());
    h.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(h.px(50, 20), Color32::WHITE);
}

#[test]
fn click_without_drag_makes_no_shape() {
    let mut h = H::new();
    h.app.set_tool(Tool::Shape);
    h.click(50.5, 50.5, L);
    assert!(h.app.pending.is_none());
    assert!(!h.app.is_dirty());
}

#[test]
fn line_shape_and_shift_constrain() {
    let mut h = H::new();
    h.app.shape_kind = ShapeKind::Line;
    h.app.sizes.shape = 1.0;
    h.app.set_tool(Tool::Shape);
    h.mods = Modifiers::SHIFT;
    h.drag((10.0, 100.0), (90.0, 103.0), L);
    h.mods = Modifiers::NONE;
    let Some(PendingKind::Shape(s)) = h.app.pending.as_ref().map(|p| &p.kind) else { panic!() };
    assert!((s.a.y - s.b.y).abs() < 0.01, "shift snaps to horizontal");
    assert_eq!(h.px(50, 100), Color32::BLACK);
}

#[test]
fn text_tool_types_and_commits() {
    let mut h = H::new();
    h.app.set_tool(Tool::Text);
    h.click(20.0, 20.0, L);
    assert!(h.app.pending_text().is_some());
    h.frame(vec![Event::Text("Hello".into())]);
    h.key(Key::Backspace, Modifiers::NONE);
    assert_eq!(h.app.pending_text().unwrap().text.iter().collect::<String>(), "Hell");
    let dark = (20..120).flat_map(|x| (20..50).map(move |y| (x, y))).filter(|&(x, y)| h.px(x, y).r() < 128).count();
    assert!(dark > 20, "glyphs rendered ({dark} dark pixels)");
    h.key(Key::Escape, Modifiers::NONE);
    assert!(h.app.pending.is_none());
    assert!(h.app.history.can_undo());
}

#[test]
fn magnifier_zooms_in_and_out() {
    let mut h = H::new();
    h.app.set_tool(Tool::Magnifier);
    h.click(100.0, 75.0, L);
    assert_eq!(h.app.view.zoom, 1.5);
    h.click(100.0, 75.0, R);
    assert_eq!(h.app.view.zoom, 1.0);
}

#[test]
fn undo_cancels_pending_selection_move() {
    let mut h = H::new();
    raster::set_px(&mut h.app.doc, 10, 10, raster::premul(RED));
    h.app.set_tool(Tool::Select);
    h.drag((5.0, 5.0), (20.0, 20.0), L);
    h.drag((10.0, 10.0), (50.0, 50.0), L);
    h.key(Key::Z, Modifiers::COMMAND);
    assert!(h.app.pending.is_none());
    assert_eq!(h.px(10, 10), RED);
    assert_eq!(h.px(50, 50), Color32::WHITE);
}

/// Renders the real UI offscreen to PNG files (for eyeballing the layout):
/// `DAUB_SHOT_DIR=/some/dir cargo test render_screenshots -- --ignored`
#[test]
#[ignore]
fn render_screenshots() {
    use egui_kittest::Harness;
    let dir = std::path::PathBuf::from(std::env::var("DAUB_SHOT_DIR").expect("DAUB_SHOT_DIR"));
    let mut harness = Harness::builder()
        .with_size(vec2(1280.0, 840.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(
            |ui, app: &mut Option<DaubApp>| {
                let a = app.get_or_insert_with(|| {
                    let mut a = DaubApp::create(ui.ctx(), None, false);
                    a.clipboard = None;
                    a
                });
                a.frame_ui(ui);
            },
            None,
        );
    harness.run();
    {
        let a = harness.state_mut().as_mut().unwrap();
        a.primary = Color32::from_rgb(0x1e, 0x88, 0xe5);
        a.sizes.brush = 8.0;
        let pts: Vec<Pos2> = (0..80).map(|i| pos2(100.0 + i as f32 * 5.0, 300.0 + (i as f32 * 0.2).sin() * 60.0)).collect();
        let before = a.doc.clone();
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(pts[0].x, pts[0].y);
        for p in &pts[1..] {
            pb.line_to(p.x, p.y);
        }
        let stroke = tiny_skia::Stroke { width: 8.0, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
        a.doc.stroke_path(&pb.finish().unwrap(), &raster::paint(a.primary, true), &stroke, tiny_skia::Transform::identity(), None);
        a.history.record(&before, &a.doc);
        a.shape_kind = ShapeKind::Star5;
        a.shape_fill = true;
        a.secondary = Color32::from_rgb(0xfd, 0xd8, 0x35);
        a.primary = Color32::BLACK;
        a.sizes.shape = 3.0;
        a.set_tool(Tool::Shape);
        a.pending = Some(crate::app::Pending {
            base: a.doc.clone(),
            kind: PendingKind::Shape(crate::app::ShapeState { kind: ShapeKind::Star5, a: pos2(650.5, 120.5), b: pos2(850.5, 310.5), swap: false }),
        });
        a.rerender_pending();
        a.mark_changed();
    }
    harness.run();
    harness.render().unwrap().save(dir.join("daub-shapes.png")).unwrap();

    {
        let a = harness.state_mut().as_mut().unwrap();
        a.commit_pending();
        a.set_tool(Tool::Text);
        a.text_style.size_pt = 28.0;
        a.text_style.bold = true;
        a.pending = Some(crate::app::Pending {
            base: a.doc.clone(),
            kind: PendingKind::Text(crate::app::TextState {
                rect: Rect::from_min_size(pos2(120.0, 420.0), vec2(420.0, 40.0)),
                text: "Hello from Daub".chars().collect(),
                caret: 15,
            }),
        });
        a.rerender_pending();
    }
    for _ in 0..5 {
        harness.step();
    }
    harness.render().unwrap().save(dir.join("daub-text.png")).unwrap();
}
