//! Shape geometry. Every shape is defined inside a unit square and mapped onto
//! the rectangle the user drags out, except `Line`, `Curve` and `Polygon`,
//! which are defined by their points.

use egui::{Pos2, Rect, pos2};
use tiny_skia::{Path, PathBuilder, PathSegment};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeKind {
    Line,
    Curve,
    Polygon,
    Rect,
    RoundRect,
    Ellipse,
    Triangle,
    RightTriangle,
    Diamond,
    Pentagon,
    Hexagon,
    ArrowRight,
    ArrowLeft,
    ArrowUp,
    ArrowDown,
    Star4,
    Star5,
    Star6,
    Heart,
    Lightning,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 20] = [
        ShapeKind::Line,
        ShapeKind::Curve,
        ShapeKind::Polygon,
        ShapeKind::Rect,
        ShapeKind::RoundRect,
        ShapeKind::Ellipse,
        ShapeKind::Triangle,
        ShapeKind::RightTriangle,
        ShapeKind::Diamond,
        ShapeKind::Pentagon,
        ShapeKind::Hexagon,
        ShapeKind::ArrowRight,
        ShapeKind::ArrowLeft,
        ShapeKind::ArrowUp,
        ShapeKind::ArrowDown,
        ShapeKind::Star4,
        ShapeKind::Star5,
        ShapeKind::Star6,
        ShapeKind::Heart,
        ShapeKind::Lightning,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ShapeKind::Line => "Line",
            ShapeKind::Curve => "Curve (drag to draw, then drag twice to bend)",
            ShapeKind::Polygon => "Polygon (drag the first side, click to add corners, click the start to close)",
            ShapeKind::Rect => "Rectangle",
            ShapeKind::RoundRect => "Rounded rectangle",
            ShapeKind::Ellipse => "Oval",
            ShapeKind::Triangle => "Triangle",
            ShapeKind::RightTriangle => "Right triangle",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Pentagon => "Pentagon",
            ShapeKind::Hexagon => "Hexagon",
            ShapeKind::ArrowRight => "Right arrow",
            ShapeKind::ArrowLeft => "Left arrow",
            ShapeKind::ArrowUp => "Up arrow",
            ShapeKind::ArrowDown => "Down arrow",
            ShapeKind::Star4 => "Four-point star",
            ShapeKind::Star5 => "Five-point star",
            ShapeKind::Star6 => "Six-point star",
            ShapeKind::Heart => "Heart",
            ShapeKind::Lightning => "Lightning",
        }
    }

    pub fn is_line(self) -> bool {
        self == ShapeKind::Line
    }

    /// Open paths: never filled, drawn with round caps.
    pub fn is_open(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Curve)
    }

    /// Edited by dragging its points rather than a bounding box.
    pub fn by_points(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Curve | ShapeKind::Polygon)
    }

    pub fn smooth_joins(self) -> bool {
        matches!(self, ShapeKind::Ellipse | ShapeKind::Heart | ShapeKind::RoundRect)
    }
}

/// Unit-square path for a shape (y down).
fn unit_path(kind: ShapeKind) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let poly = |pb: &mut PathBuilder, pts: &[(f32, f32)]| {
        pb.move_to(pts[0].0, pts[0].1);
        for p in &pts[1..] {
            pb.line_to(p.0, p.1);
        }
        pb.close();
    };
    let regular = |n: usize| -> Vec<(f32, f32)> {
        (0..n)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
                (0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin())
            })
            .collect()
    };
    let star = |n: usize, inner: f32| -> Vec<(f32, f32)> {
        (0..n * 2)
            .map(|i| {
                let r = if i % 2 == 0 { 0.5 } else { 0.5 * inner };
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / n as f32;
                (0.5 + r * a.cos(), 0.5 + r * a.sin())
            })
            .collect()
    };
    let arrow: [(f32, f32); 7] =
        [(0.0, 0.25), (0.5, 0.25), (0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.5, 0.75), (0.0, 0.75)];
    match kind {
        ShapeKind::Line | ShapeKind::Curve | ShapeKind::Polygon => return None,
        ShapeKind::Rect => poly(&mut pb, &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        ShapeKind::RoundRect | ShapeKind::Ellipse => unreachable!("handled in rect_path"),
        ShapeKind::Triangle => poly(&mut pb, &[(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        ShapeKind::RightTriangle => poly(&mut pb, &[(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        ShapeKind::Diamond => poly(&mut pb, &[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)]),
        ShapeKind::Pentagon => poly(&mut pb, &regular(5)),
        ShapeKind::Hexagon => poly(&mut pb, &regular(6)),
        ShapeKind::ArrowRight => poly(&mut pb, &arrow),
        ShapeKind::ArrowLeft => poly(&mut pb, &arrow.map(|(x, y)| (1.0 - x, y))),
        ShapeKind::ArrowDown => poly(&mut pb, &arrow.map(|(x, y)| (y, x))),
        ShapeKind::ArrowUp => poly(&mut pb, &arrow.map(|(x, y)| (y, 1.0 - x))),
        ShapeKind::Star4 => poly(&mut pb, &star(4, 0.38)),
        ShapeKind::Star5 => poly(&mut pb, &star(5, 0.4)),
        ShapeKind::Star6 => poly(&mut pb, &star(6, 0.55)),
        ShapeKind::Lightning => poly(
            &mut pb,
            &[
                (0.38, 0.0),
                (0.62, 0.0),
                (0.5, 0.36),
                (0.78, 0.36),
                (0.3, 1.0),
                (0.42, 0.52),
                (0.2, 0.52),
            ],
        ),
        ShapeKind::Heart => {
            pb.move_to(0.5, 0.28);
            pb.cubic_to(0.5, 0.0, 0.0, 0.0, 0.0, 0.3);
            pb.cubic_to(0.0, 0.62, 0.42, 0.78, 0.5, 1.0);
            pb.cubic_to(0.58, 0.78, 1.0, 0.62, 1.0, 0.3);
            pb.cubic_to(1.0, 0.0, 0.5, 0.0, 0.5, 0.28);
            pb.close();
        }
    }
    pb.finish()
}

fn rounded_rect(r: Rect, radius: f32) -> Option<Path> {
    let k = 0.552_284_8 * radius;
    let (l, t, rt, b) = (r.min.x, r.min.y, r.max.x, r.max.y);
    let mut pb = PathBuilder::new();
    pb.move_to(l + radius, t);
    pb.line_to(rt - radius, t);
    pb.cubic_to(rt - radius + k, t, rt, t + radius - k, rt, t + radius);
    pb.line_to(rt, b - radius);
    pb.cubic_to(rt, b - radius + k, rt - radius + k, b, rt - radius, b);
    pb.line_to(l + radius, b);
    pb.cubic_to(l + radius - k, b, l, b - radius + k, l, b - radius);
    pb.line_to(l, t + radius);
    pb.cubic_to(l, t + radius - k, l + radius - k, t, l + radius, t);
    pb.close();
    pb.finish()
}

/// Path for `kind` spanning from `a` to `b` in canvas coordinates.
pub fn path(kind: ShapeKind, a: Pos2, b: Pos2) -> Option<Path> {
    if kind.by_points() {
        let mut pb = PathBuilder::new();
        pb.move_to(a.x, a.y);
        pb.line_to(b.x, b.y);
        return pb.finish();
    }
    let r = Rect::from_two_pos(a, b);
    if r.width() < 1.0 || r.height() < 1.0 {
        return None;
    }
    match kind {
        ShapeKind::Ellipse => {
            PathBuilder::from_oval(tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y)?)
        }
        ShapeKind::RoundRect => rounded_rect(r, r.width().min(r.height()) * 0.18),
        _ => {
            let ts = tiny_skia::Transform::from_row(r.width(), 0.0, 0.0, r.height(), r.min.x, r.min.y);
            unit_path(kind)?.transform(ts)
        }
    }
}

/// Cubic curve from `a` to `b` bent by up to two control points.
pub fn curve_path(a: Pos2, b: Pos2, ctrl: &[Pos2]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    match ctrl {
        [c1, c2, ..] => pb.cubic_to(c1.x, c1.y, c2.x, c2.y, b.x, b.y),
        [c] => pb.quad_to(c.x, c.y, b.x, b.y),
        [] => pb.line_to(b.x, b.y),
    }
    pb.finish()
}

pub fn polygon_path(pts: &[Pos2], closed: bool) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(pts.first()?.x, pts[0].y);
    for p in &pts[1..] {
        pb.line_to(p.x, p.y);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

/// How a shape's outline or fill is painted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintStyle {
    None,
    Solid,
    Crayon,
    Marker,
    Oil,
    NaturalPencil,
    Watercolor,
}

impl PaintStyle {
    pub const ALL: [PaintStyle; 7] = [
        PaintStyle::None,
        PaintStyle::Solid,
        PaintStyle::Crayon,
        PaintStyle::Marker,
        PaintStyle::Oil,
        PaintStyle::NaturalPencil,
        PaintStyle::Watercolor,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PaintStyle::None => "None",
            PaintStyle::Solid => "Solid color",
            PaintStyle::Crayon => "Crayon",
            PaintStyle::Marker => "Marker",
            PaintStyle::Oil => "Oil",
            PaintStyle::NaturalPencil => "Natural pencil",
            PaintStyle::Watercolor => "Watercolor",
        }
    }

    /// The brush whose texture this style borrows, for textured styles.
    pub fn brush(self) -> Option<crate::brushes::BrushKind> {
        use crate::brushes::BrushKind as B;
        match self {
            PaintStyle::None | PaintStyle::Solid => None,
            PaintStyle::Crayon => Some(B::Crayon),
            PaintStyle::Marker => Some(B::Marker),
            PaintStyle::Oil => Some(B::Oil),
            PaintStyle::NaturalPencil => Some(B::NaturalPencil),
            PaintStyle::Watercolor => Some(B::Watercolor),
        }
    }
}

/// Flattened polylines of a shape inside `rect`, for drawing ribbon icons with egui.
pub fn icon_polylines(kind: ShapeKind, rect: Rect) -> Vec<Vec<Pos2>> {
    let at = |x: f32, y: f32| pos2(rect.min.x + x * rect.width(), rect.min.y + y * rect.height());
    match kind {
        ShapeKind::Curve => {
            let p = curve_path(at(0.0, 0.85), at(1.0, 0.15), &[at(0.15, -0.3), at(0.85, 1.3)]);
            return p.map(|p| flatten(&p)).unwrap_or_default();
        }
        ShapeKind::Polygon => {
            let pts = [at(0.1, 0.3), at(0.55, 0.0), at(1.0, 0.45), at(0.7, 1.0), at(0.15, 0.85)];
            return polygon_path(&pts, true).map(|p| flatten(&p)).unwrap_or_default();
        }
        _ => {}
    }
    let (a, b) = if kind.is_line() {
        (rect.left_bottom(), rect.right_top())
    } else {
        (rect.min, rect.max)
    };
    let Some(p) = path(kind, a, b) else { return vec![] };
    flatten(&p)
}

pub fn flatten(p: &Path) -> Vec<Vec<Pos2>> {
    let mut out: Vec<Vec<Pos2>> = vec![];
    let mut cur: Vec<Pos2> = vec![];
    let mut last = Pos2::ZERO;
    const STEPS: usize = 10;
    for seg in p.segments() {
        match seg {
            PathSegment::MoveTo(pt) => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                last = pos2(pt.x, pt.y);
                cur.push(last);
            }
            PathSegment::LineTo(pt) => {
                last = pos2(pt.x, pt.y);
                cur.push(last);
            }
            PathSegment::QuadTo(c, pt) => {
                let (p0, p1, p2) = (last, pos2(c.x, c.y), pos2(pt.x, pt.y));
                for i in 1..=STEPS {
                    let t = i as f32 / STEPS as f32;
                    let u = 1.0 - t;
                    cur.push((p0.to_vec2() * u * u + p1.to_vec2() * 2.0 * u * t + p2.to_vec2() * t * t).to_pos2());
                }
                last = p2;
            }
            PathSegment::CubicTo(c1, c2, pt) => {
                let (p0, p1, p2, p3) = (last, pos2(c1.x, c1.y), pos2(c2.x, c2.y), pos2(pt.x, pt.y));
                for i in 1..=STEPS {
                    let t = i as f32 / STEPS as f32;
                    let u = 1.0 - t;
                    cur.push(
                        (p0.to_vec2() * u * u * u
                            + p1.to_vec2() * 3.0 * u * u * t
                            + p2.to_vec2() * 3.0 * u * t * t
                            + p3.to_vec2() * t * t * t)
                            .to_pos2(),
                    );
                }
                last = p3;
            }
            PathSegment::Close => {
                if let Some(&f) = cur.first() {
                    cur.push(f);
                }
            }
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// Shift-constrain: squares for shapes, 45° steps for lines.
pub fn constrain(kind: ShapeKind, a: Pos2, b: Pos2) -> Pos2 {
    let d = b - a;
    if kind.by_points() {
        let ang = d.y.atan2(d.x);
        let step = std::f32::consts::FRAC_PI_4;
        let snapped = (ang / step).round() * step;
        let len = d.length();
        a + egui::vec2(snapped.cos(), snapped.sin()) * len
    } else {
        let m = d.x.abs().max(d.y.abs());
        a + egui::vec2(m * d.x.signum(), m * d.y.signum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_has_a_path() {
        for k in ShapeKind::ALL {
            assert!(path(k, pos2(10.0, 10.0), pos2(60.0, 40.0)).is_some(), "{k:?}");
            assert!(!icon_polylines(k, Rect::from_min_size(Pos2::ZERO, egui::vec2(16.0, 16.0))).is_empty());
        }
    }
}
