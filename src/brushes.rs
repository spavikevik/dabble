//! Brush types and the stroke engine.
//!
//! A stroke keeps a per-pixel coverage buffer. Each new segment adds coverage
//! (taking the max, so a stroke never darkens where it crosses itself), and
//! only the pixels it touched are recomposed from the pre-stroke image. That
//! keeps long strokes cheap.

use egui::{Color32, Pos2, pos2};
use tiny_skia::{FillRule, IntRect, LineCap, Mask, PathBuilder, Pixmap, PremultipliedColorU8, Transform};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrushKind {
    Brush,
    Calligraphy1,
    Calligraphy2,
    Airbrush,
    Oil,
    Crayon,
    Marker,
    NaturalPencil,
    Watercolor,
}

impl BrushKind {
    pub const ALL: [BrushKind; 9] = [
        BrushKind::Brush,
        BrushKind::Calligraphy1,
        BrushKind::Calligraphy2,
        BrushKind::Airbrush,
        BrushKind::Oil,
        BrushKind::Crayon,
        BrushKind::Marker,
        BrushKind::NaturalPencil,
        BrushKind::Watercolor,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BrushKind::Brush => "Brush",
            BrushKind::Calligraphy1 => "Calligraphy brush 1",
            BrushKind::Calligraphy2 => "Calligraphy brush 2",
            BrushKind::Airbrush => "Airbrush",
            BrushKind::Oil => "Oil brush",
            BrushKind::Crayon => "Crayon",
            BrushKind::Marker => "Marker",
            BrushKind::NaturalPencil => "Natural pencil",
            BrushKind::Watercolor => "Watercolor brush",
        }
    }

    pub fn icon(self) -> &'static str {
        use egui_phosphor::regular as ic;
        match self {
            BrushKind::Brush => ic::PAINT_BRUSH,
            BrushKind::Calligraphy1 | BrushKind::Calligraphy2 => ic::PEN_NIB,
            BrushKind::Airbrush => ic::SPRAY_BOTTLE,
            BrushKind::Oil => ic::PAINT_BRUSH_BROAD,
            BrushKind::Crayon => ic::PENCIL_SIMPLE_LINE,
            BrushKind::Marker => ic::HIGHLIGHTER,
            BrushKind::NaturalPencil => ic::PENCIL_LINE,
            BrushKind::Watercolor => ic::DROP,
        }
    }

    /// Overall opacity of the paint.
    pub fn alpha(self) -> f32 {
        match self {
            BrushKind::Marker => 0.5,
            BrushKind::Watercolor => 0.4,
            BrushKind::NaturalPencil => 0.85,
            BrushKind::Crayon => 0.95,
            _ => 1.0,
        }
    }

    /// Paper/brush texture at a pixel, 0..=1. Fixed per pixel, so it reads as
    /// paper grain rather than noise that changes on every stroke.
    pub fn texture(self, x: i32, y: i32) -> f32 {
        match self {
            BrushKind::Crayon => smoothstep(0.15, 0.6, hash(x, y)),
            BrushKind::NaturalPencil => 0.45 + 0.55 * hash(x, y),
            BrushKind::Watercolor => 0.8 + 0.2 * value_noise(x as f32 / 7.0, y as f32 / 7.0),
            BrushKind::Oil => 0.7 + 0.3 * hash(x.div_euclid(5), y),
            _ => 1.0,
        }
    }

    /// Brushes that keep painting while the pointer is held still.
    pub fn continuous(self) -> bool {
        self == BrushKind::Airbrush
    }
}

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}

fn value_noise(x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash(x0, y0) + (hash(x0 + 1, y0) - hash(x0, y0)) * sx;
    let b = hash(x0, y0 + 1) + (hash(x0 + 1, y0 + 1) - hash(x0, y0 + 1)) * sx;
    a + (b - a) * sy
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Blend `color` (straight alpha ignored; treated as opaque RGB with `a`) over `under`.
#[inline]
fn mix(under: PremultipliedColorU8, color: Color32, a: f32) -> PremultipliedColorU8 {
    if a <= 0.0 {
        return under;
    }
    let a = a.min(1.0);
    let inv = 1.0 - a;
    let f = |c: u8, u: u8| c as f32 * a + u as f32 * inv;
    let na = (255.0 * a + under.alpha() as f32 * inv).round().clamp(0.0, 255.0) as u8;
    let ch = |v: f32| (v.round().clamp(0.0, 255.0) as u8).min(na);
    PremultipliedColorU8::from_rgba(
        ch(f(color.r(), under.red())),
        ch(f(color.g(), under.green())),
        ch(f(color.b(), under.blue())),
        na,
    )
    .unwrap()
}

/// Composite `mask` coverage of `color` onto `pm` using a brush's opacity and
/// texture. Used for textured shape outlines and fills.
pub fn apply_mask(pm: &mut Pixmap, mask: &Mask, color: Color32, kind: BrushKind) {
    let w = pm.width() as i32;
    let alpha = kind.alpha();
    let m = mask.data();
    let px = pm.pixels_mut();
    for (i, &c) in m.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let (x, y) = (i as i32 % w, i as i32 / w);
        px[i] = mix(px[i], color, c as f32 / 255.0 * alpha * kind.texture(x, y));
    }
}

pub struct Stroke {
    pub kind: BrushKind,
    pub color: Color32,
    pub size: f32,
    pub before: Pixmap,
    cov: Vec<u8>,
    last: Option<Pos2>,
    travelled: f32,
    rng: u64,
}

impl Stroke {
    pub fn new(kind: BrushKind, color: Color32, size: f32, before: Pixmap) -> Self {
        let n = (before.width() * before.height()) as usize;
        Self { kind, color, size: size.max(1.0), before, cov: vec![0; n], last: None, travelled: 0.0, rng: 0x9e37_79b9_7f4a_7c15 }
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Extend the stroke to `p` and repaint the affected pixels of `doc`.
    pub fn add_point(&mut self, doc: &mut Pixmap, p: Pos2) {
        let a = self.last.unwrap_or(p);
        if self.last.is_some() && a.distance(p) < 0.5 {
            return;
        }
        self.last = Some(p);
        let dirty = match self.kind {
            BrushKind::Airbrush => {
                // Moving sprays along the path; holding still sprays over time.
                let n = (a.distance(p) * self.size * 0.6) as usize + 1;
                let mut r = None;
                for i in 0..n {
                    let q = a + (p - a) * (i as f32 / n as f32);
                    r = union(r, self.spray_dots(q, (self.size * 0.6) as usize + 1));
                }
                r
            }
            BrushKind::Watercolor => self.soft_segment(a, p),
            BrushKind::Oil => self.bristles(a, p),
            BrushKind::Calligraphy1 => self.nib(a, p, 45.0),
            BrushKind::Calligraphy2 => self.nib(a, p, 135.0),
            BrushKind::Marker => self.capsule(a, p, self.size, LineCap::Square, 255),
            _ => self.capsule(a, p, self.size, LineCap::Round, 255),
        };
        self.travelled += a.distance(p);
        if let Some(r) = dirty {
            self.recompose(doc, r);
        }
    }

    /// Airbrush while the pointer is held: spray for `dt` seconds at the last point.
    pub fn tick(&mut self, doc: &mut Pixmap, dt: f32) {
        let Some(p) = self.last else { return };
        if !self.kind.continuous() {
            return;
        }
        let n = (self.size * self.size * 3.0 * dt).ceil() as usize;
        if let Some(r) = self.spray_dots(p, n) {
            self.recompose(doc, r);
        }
    }

    fn spray_dots(&mut self, c: Pos2, n: usize) -> Option<IntRect> {
        let rad = self.size / 2.0;
        let (w, h) = (self.before.width() as i32, self.before.height() as i32);
        for _ in 0..n {
            // Uniform point in the disc.
            let (u, v) = (self.rand() * 2.0 - 1.0, self.rand() * 2.0 - 1.0);
            if u * u + v * v > 1.0 {
                continue;
            }
            let (x, y) = ((c.x + u * rad).floor() as i32, (c.y + v * rad).floor() as i32);
            if x >= 0 && y >= 0 && x < w && y < h {
                self.cov[(y * w + x) as usize] = 255;
            }
        }
        clip_rect(c.x - rad - 1.0, c.y - rad - 1.0, c.x + rad + 1.0, c.y + rad + 1.0, w, h)
    }

    /// Round or square capped line from `a` to `b`, coverage scaled by `strength`.
    fn capsule(&mut self, a: Pos2, b: Pos2, width: f32, cap: LineCap, strength: u8) -> Option<IntRect> {
        let mut pb = PathBuilder::new();
        pb.move_to(a.x, a.y);
        // A zero-length segment still needs a cap drawn.
        let b = if a == b { pos2(b.x + 0.01, b.y) } else { b };
        pb.line_to(b.x, b.y);
        let path = pb.finish()?;
        let stroke = tiny_skia::Stroke { width, line_cap: cap, ..Default::default() };
        let path = path.stroke(&stroke, 1.0)?;
        self.max_path(&path, strength)
    }

    /// Flat calligraphy nib at `deg` degrees swept from `a` to `b`.
    fn nib(&mut self, a: Pos2, b: Pos2, deg: f32) -> Option<IntRect> {
        let half = self.size / 2.0;
        let (s, c) = deg.to_radians().sin_cos();
        let n = egui::vec2(c, -s) * half;
        let mut pb = PathBuilder::new();
        pb.move_to(a.x + n.x, a.y + n.y);
        pb.line_to(b.x + n.x, b.y + n.y);
        pb.line_to(b.x - n.x, b.y - n.y);
        pb.line_to(a.x - n.x, a.y - n.y);
        pb.close();
        let r1 = pb.finish().and_then(|p| self.max_path(&p, 255));
        // The nib itself, so slow strokes along the nib direction still show.
        let r2 = self.capsule(b - n, b + n, 1.5, LineCap::Butt, 255);
        union(r1, r2)
    }

    /// Several thin streaks across the brush width, fading as paint runs out.
    fn bristles(&mut self, a: Pos2, b: Pos2) -> Option<IntRect> {
        let dir = if a == b { egui::vec2(1.0, 0.0) } else { (b - a).normalized() };
        let perp = egui::vec2(-dir.y, dir.x);
        let count = (self.size / 1.5).clamp(3.0, 24.0) as usize;
        let fade = (1.0 - self.travelled / (self.size * 50.0 + 200.0)).max(0.45);
        let mut r = None;
        for i in 0..count {
            let t = (i as f32 + 0.5) / count as f32 - 0.5;
            let off = perp * (t * self.size);
            let strength = (0.55 + 0.45 * hash(i as i32, 7)) * fade;
            let w = (self.size / count as f32 * 1.6).max(1.0);
            r = union(r, self.capsule(a + off, b + off, w, LineCap::Round, (strength * 255.0) as u8));
        }
        r
    }

    /// Soft-edged round segment (watercolor).
    fn soft_segment(&mut self, a: Pos2, b: Pos2) -> Option<IntRect> {
        let rad = self.size / 2.0 + 0.5;
        let (w, h) = (self.before.width() as i32, self.before.height() as i32);
        let r = clip_rect(a.x.min(b.x) - rad, a.y.min(b.y) - rad, a.x.max(b.x) + rad, a.y.max(b.y) + rad, w, h)?;
        let ab = b - a;
        let len2 = ab.length_sq().max(1e-6);
        for y in r.y()..r.y() + r.height() as i32 {
            for x in r.x()..r.x() + r.width() as i32 {
                let p = pos2(x as f32 + 0.5, y as f32 + 0.5);
                let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
                let d = (a + ab * t).distance(p) / rad;
                if d < 1.0 {
                    let v = (smoothstep(1.0, 0.35, d) * 255.0) as u8;
                    let c = &mut self.cov[(y * w + x) as usize];
                    *c = (*c).max(v);
                }
            }
        }
        Some(r)
    }

    /// Rasterize a filled path and max it into the coverage buffer.
    fn max_path(&mut self, path: &tiny_skia::Path, strength: u8) -> Option<IntRect> {
        let (w, h) = (self.before.width() as i32, self.before.height() as i32);
        let b = path.bounds();
        let r = clip_rect(b.left() - 1.0, b.top() - 1.0, b.right() + 1.0, b.bottom() + 1.0, w, h)?;
        let mut m = Mask::new(r.width(), r.height())?;
        m.fill_path(path, FillRule::Winding, true, Transform::from_translate(-r.x() as f32, -r.y() as f32));
        let rw = r.width() as i32;
        for (i, &v) in m.data().iter().enumerate() {
            if v == 0 {
                continue;
            }
            let v = (v as u16 * strength as u16 / 255) as u8;
            let (x, y) = (r.x() + i as i32 % rw, r.y() + i as i32 / rw);
            let c = &mut self.cov[(y * w + x) as usize];
            *c = (*c).max(v);
        }
        Some(r)
    }

    fn recompose(&self, doc: &mut Pixmap, r: IntRect) {
        let w = doc.width() as i32;
        let alpha = self.kind.alpha() * self.color.a() as f32 / 255.0;
        let color = Color32::from_rgb(self.color.r(), self.color.g(), self.color.b());
        let before = self.before.pixels();
        let px = doc.pixels_mut();
        for y in r.y()..r.y() + r.height() as i32 {
            for x in r.x()..r.x() + r.width() as i32 {
                let i = (y * w + x) as usize;
                let c = self.cov[i];
                px[i] = if c == 0 {
                    before[i]
                } else {
                    mix(before[i], color, c as f32 / 255.0 * alpha * self.kind.texture(x, y))
                };
            }
        }
    }
}

fn clip_rect(l: f32, t: f32, r: f32, b: f32, w: i32, h: i32) -> Option<IntRect> {
    let (l, t) = ((l.floor() as i32).max(0), (t.floor() as i32).max(0));
    let (r, b) = ((r.ceil() as i32).min(w), (b.ceil() as i32).min(h));
    IntRect::from_ltrb(l, t, r, b)
}

fn union(a: Option<IntRect>, b: Option<IntRect>) -> Option<IntRect> {
    match (a, b) {
        (Some(a), Some(b)) => IntRect::from_ltrb(
            a.left().min(b.left()),
            a.top().min(b.top()),
            a.right().max(b.right()),
            a.bottom().max(b.bottom()),
        ),
        (a, None) => a,
        (None, b) => b,
    }
}

/// Paint a sample stroke for the brush menu.
pub fn preview(kind: BrushKind, w: u32, h: u32, color: Color32) -> Pixmap {
    let mut pm = Pixmap::new(w, h).unwrap();
    let size = (h as f32 * 0.35).max(3.0);
    let mut s = Stroke::new(kind, color, size, pm.clone());
    let n = 60;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let x = size + t * (w as f32 - 2.0 * size);
        let y = h as f32 / 2.0 + (t * std::f32::consts::TAU).sin() * (h as f32 / 2.0 - size * 0.8);
        s.add_point(&mut pm, pos2(x, y));
    }
    if kind.continuous() {
        s.tick(&mut pm, 0.05);
    }
    pm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster;

    #[test]
    fn every_brush_paints_and_restarts_cleanly() {
        for k in BrushKind::ALL {
            let base = raster::new_canvas(60, 40, Color32::WHITE);
            let mut doc = base.clone();
            let mut s = Stroke::new(k, Color32::BLACK, 8.0, base.clone());
            for x in 10..50 {
                s.add_point(&mut doc, pos2(x as f32, 20.0));
            }
            s.tick(&mut doc, 0.2);
            let dark = doc.pixels().iter().filter(|p| p.red() < 200).count();
            assert!(dark > 20, "{k:?} painted only {dark} pixels");
            assert_eq!(raster::pixel(&doc, 2, 2), Some(Color32::WHITE), "{k:?} stays local");
        }
    }

    #[test]
    fn crossing_a_stroke_does_not_darken_it() {
        let base = raster::new_canvas(40, 40, Color32::WHITE);
        let mut doc = base.clone();
        let mut s = Stroke::new(BrushKind::Marker, Color32::BLACK, 6.0, base);
        s.add_point(&mut doc, pos2(5.0, 20.0));
        s.add_point(&mut doc, pos2(35.0, 20.0));
        let once = raster::pixel(&doc, 20, 20).unwrap();
        s.add_point(&mut doc, pos2(20.0, 5.0));
        s.add_point(&mut doc, pos2(20.0, 35.0));
        assert_eq!(raster::pixel(&doc, 20, 20).unwrap(), once);
        assert!(once.r() > 100 && once.r() < 160, "marker is see-through: {once:?}");
    }
}
