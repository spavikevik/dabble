//! Text tool: system font lookup (fontdb), layout with word wrap, and
//! rasterizing glyphs (ab_glyph) straight into the canvas.

use ab_glyph::{Font, FontVec, GlyphId, PxScale, ScaleFont, point};
use egui::{Color32, Pos2, Rect, pos2};
use std::collections::HashMap;
use std::sync::Arc;
use tiny_skia::Pixmap;

use crate::raster;

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub family: String,
    /// Size in points; rendered at 96 dpi like Windows (px = pt * 4/3).
    pub size_pt: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub opaque_bg: bool,
}

impl TextStyle {
    pub fn px(&self) -> f32 {
        self.size_pt * 96.0 / 72.0
    }
}

pub struct Fonts {
    db: fontdb::Database,
    pub families: Vec<String>,
    cache: HashMap<(String, bool, bool), Option<Arc<FontVec>>>,
}

impl Fonts {
    pub fn load() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut families: Vec<String> = db
            .faces()
            .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
            .filter(|n| !n.starts_with('.'))
            .collect();
        families.sort_by_key(|a| a.to_lowercase());
        families.dedup();
        Self { db, families, cache: HashMap::new() }
    }

    pub fn default_family(&self) -> String {
        for want in ["Helvetica Neue", "Helvetica", "Arial", "SF Pro", "Geneva"] {
            if self.families.iter().any(|f| f == want) {
                return want.to_string();
            }
        }
        self.families.first().cloned().unwrap_or_default()
    }

    pub fn font(&mut self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontVec>> {
        let key = (family.to_string(), bold, italic);
        if let Some(f) = self.cache.get(&key) {
            return f.clone();
        }
        let q = fontdb::Query {
            families: &[fontdb::Family::Name(family), fontdb::Family::SansSerif],
            weight: if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
            stretch: fontdb::Stretch::Normal,
            style: if italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
        };
        let font = self.db.query(&q).and_then(|id| {
            self.db
                .with_face_data(id, |data, index| FontVec::try_from_vec_and_index(data.to_vec(), index).ok())
                .flatten()
        });
        let font = font.map(Arc::new);
        self.cache.insert(key, font.clone());
        font
    }
}

pub struct Layout {
    pub glyphs: Vec<(GlyphId, Pos2)>,
    /// Caret position (top of line) for every char index 0..=len, box-local.
    pub carets: Vec<Pos2>,
    /// (x_start, x_end, baseline_y) per visual line, box-local.
    pub lines: Vec<(f32, f32, f32)>,
    pub line_height: f32,
    pub ascent: f32,
    pub height: f32,
}

pub fn layout(font: &FontVec, px: f32, text: &[char], width: f32) -> Layout {
    let sf = font.as_scaled(PxScale::from(px));
    let ascent = sf.ascent();
    let line_height = sf.ascent() - sf.descent() + sf.line_gap();
    let adv = |c: char| sf.h_advance(font.glyph_id(c));

    // Greedy word wrap into char ranges.
    let mut ranges: Vec<(usize, usize)> = vec![];
    let mut start = 0;
    let mut x = 0.0;
    let mut last_break: Option<usize> = None;
    for (i, &c) in text.iter().enumerate() {
        if c == '\n' {
            ranges.push((start, i));
            start = i + 1;
            x = 0.0;
            last_break = None;
            continue;
        }
        let a = adv(c);
        if x + a > width && i > start && c != ' ' {
            match last_break {
                Some(b) if b > start => {
                    ranges.push((start, b));
                    start = b;
                    x = text[b..i].iter().map(|&c| adv(c)).sum();
                }
                _ => {
                    ranges.push((start, i));
                    start = i;
                    x = 0.0;
                }
            }
            last_break = None;
        }
        x += a;
        if c == ' ' {
            last_break = Some(i + 1);
        }
    }
    ranges.push((start, text.len()));

    let mut glyphs = vec![];
    let mut carets = vec![Pos2::ZERO; text.len() + 1];
    let mut lines = vec![];
    for (li, &(s, e)) in ranges.iter().enumerate() {
        let top = li as f32 * line_height;
        let baseline = top + ascent;
        let mut pen = 0.0;
        let mut prev: Option<GlyphId> = None;
        for (i, &c) in text.iter().enumerate().take(e).skip(s) {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                pen += sf.kern(p, id);
            }
            carets[i] = pos2(pen, top);
            if !c.is_whitespace() {
                glyphs.push((id, pos2(pen, baseline)));
            }
            pen += sf.h_advance(id);
            prev = Some(id);
        }
        carets[e] = pos2(pen, top);
        lines.push((0.0, pen, baseline));
    }
    let height = ranges.len() as f32 * line_height;
    Layout { glyphs, carets, lines, line_height, ascent, height }
}

/// Draw laid-out text into `pm` with its box at `rect` (canvas coords).
pub fn render(
    pm: &mut Pixmap,
    font: &FontVec,
    style: &TextStyle,
    lay: &Layout,
    rect: Rect,
    color: Color32,
    bg: Option<Color32>,
) {
    if let Some(bg) = bg {
        let r = rect.with_max_y(rect.min.y + rect.height().max(lay.height));
        if let Some(sr) = tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y) {
            pm.fill_rect(sr, &raster::paint(bg, false), tiny_skia::Transform::identity(), None);
        }
    }
    let px = style.px();
    for &(id, p) in &lay.glyphs {
        let g = id.with_scale_and_position(px, point(rect.min.x + p.x, rect.min.y + p.y));
        if let Some(og) = font.outline_glyph(g) {
            let b = og.px_bounds();
            og.draw(|x, y, cov| {
                raster::blend_px(pm, b.min.x as i32 + x as i32, b.min.y as i32 + y as i32, color, cov);
            });
        }
    }
    let thick = (px / 15.0).max(1.0);
    for &(x0, x1, baseline) in &lay.lines {
        if x1 <= x0 {
            continue;
        }
        let mut bar = |y: f32| {
            if let Some(sr) =
                tiny_skia::Rect::from_xywh(rect.min.x + x0, rect.min.y + y, x1 - x0, thick)
            {
                pm.fill_rect(sr, &raster::paint(color, true), tiny_skia::Transform::identity(), None);
            }
        };
        if style.underline {
            bar(baseline + px * 0.1);
        }
        if style.strike {
            bar(baseline - lay.ascent * 0.32);
        }
    }
}
