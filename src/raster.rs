//! Low-level pixel helpers on top of `tiny_skia::Pixmap` (premultiplied RGBA8).

use egui::Color32;
use tiny_skia::{IntRect, Paint, Pixmap, PixmapPaint, PremultipliedColorU8, Transform};

pub fn new_canvas(w: u32, h: u32, bg: Color32) -> Pixmap {
    let mut pm = Pixmap::new(w.max(1), h.max(1)).expect("canvas size");
    pm.fill(to_sk(bg));
    pm
}

pub fn to_sk(c: Color32) -> tiny_skia::Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

pub fn premul(c: Color32) -> PremultipliedColorU8 {
    // Color32 is already premultiplied.
    PremultipliedColorU8::from_rgba(c.r(), c.g(), c.b(), c.a()).expect("premultiplied")
}

pub fn from_premul(p: PremultipliedColorU8) -> Color32 {
    Color32::from_rgba_premultiplied(p.red(), p.green(), p.blue(), p.alpha())
}

pub fn paint(c: Color32, anti_alias: bool) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(to_sk(c));
    p.anti_alias = anti_alias;
    p
}

pub fn pixel(pm: &Pixmap, x: i32, y: i32) -> Option<Color32> {
    if x < 0 || y < 0 {
        return None;
    }
    pm.pixel(x as u32, y as u32).map(from_premul)
}

#[inline]
pub fn set_px(pm: &mut Pixmap, x: i32, y: i32, c: PremultipliedColorU8) {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    if x >= 0 && y >= 0 && x < w && y < h {
        pm.pixels_mut()[(y * w + x) as usize] = c;
    }
}

/// Source-over blend of an opaque-ish color with fractional coverage.
#[inline]
pub fn blend_px(pm: &mut Pixmap, x: i32, y: i32, c: Color32, coverage: f32) {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    if x < 0 || y < 0 || x >= w || y >= h || coverage <= 0.0 {
        return;
    }
    let cov = coverage.min(1.0);
    let px = &mut pm.pixels_mut()[(y * w + x) as usize];
    let sa = c.a() as f32 / 255.0 * cov;
    let inv = 1.0 - sa;
    let r = c.r() as f32 * cov + px.red() as f32 * inv;
    let g = c.g() as f32 * cov + px.green() as f32 * inv;
    let b = c.b() as f32 * cov + px.blue() as f32 * inv;
    let a = sa * 255.0 + px.alpha() as f32 * inv;
    let a = a.round().clamp(0.0, 255.0) as u8;
    let clampc = |v: f32| (v.round().clamp(0.0, 255.0) as u8).min(a);
    *px = PremultipliedColorU8::from_rgba(clampc(r), clampc(g), clampc(b), a).unwrap();
}

/// Fill an axis-aligned square of `size` pixels centered on (cx, cy).
pub fn stamp_square(pm: &mut Pixmap, cx: i32, cy: i32, size: i32, c: PremultipliedColorU8) {
    let half = (size - 1) / 2;
    for y in cy - half..cy - half + size {
        for x in cx - half..cx - half + size {
            set_px(pm, x, y, c);
        }
    }
}

/// Bresenham walk from (x0,y0) to (x1,y1), calling `f` on each point.
pub fn walk_line(x0: i32, y0: i32, x1: i32, y1: i32, mut f: impl FnMut(i32, i32)) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let (mut x, mut y) = (x0, y0);
    let mut err = dx + dy;
    loop {
        f(x, y);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Scanline flood fill with exact color match (like MS Paint's default tolerance).
pub fn flood_fill(pm: &mut Pixmap, x: i32, y: i32, color: Color32) -> bool {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    if x < 0 || y < 0 || x >= w || y >= h {
        return false;
    }
    let new = premul(color);
    let px = pm.pixels_mut();
    let target = px[(y * w + x) as usize];
    if target == new {
        return false;
    }
    let mut stack = vec![(x, y)];
    while let Some((sx, sy)) = stack.pop() {
        let row = (sy * w) as usize;
        if px[row + sx as usize] != target {
            continue;
        }
        let mut l = sx;
        while l > 0 && px[row + (l - 1) as usize] == target {
            l -= 1;
        }
        let mut r = sx;
        while r < w - 1 && px[row + (r + 1) as usize] == target {
            r += 1;
        }
        for xx in l..=r {
            px[row + xx as usize] = new;
        }
        for ny in [sy - 1, sy + 1] {
            if ny < 0 || ny >= h {
                continue;
            }
            let nrow = (ny * w) as usize;
            let mut xx = l;
            while xx <= r {
                if px[nrow + xx as usize] == target {
                    stack.push((xx, ny));
                    while xx <= r && px[nrow + xx as usize] == target {
                        xx += 1;
                    }
                } else {
                    xx += 1;
                }
            }
        }
    }
    true
}

/// Replace (no blending) a region of `dst` with `src` placed at (x, y). Clips.
pub fn copy_into(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32) {
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + sw).min(dw);
    let y1 = (y + sh).min(dh);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let n = (x1 - x0) as usize * 4;
    let sdata = src.data();
    let ddata = dst.data_mut();
    for yy in y0..y1 {
        let s = (((yy - y) * sw + (x0 - x)) * 4) as usize;
        let d = ((yy * dw + x0) * 4) as usize;
        ddata[d..d + n].copy_from_slice(&sdata[s..s + n]);
    }
}

/// Source-over composite `src` onto `dst` at (x, y).
pub fn draw_over(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32) {
    dst.draw_pixmap(x, y, src.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

/// Overwrite `dst` with `src` (same size).
pub fn restore(dst: &mut Pixmap, src: &Pixmap) {
    if dst.width() == src.width() && dst.height() == src.height() {
        dst.data_mut().copy_from_slice(src.data());
    } else {
        *dst = src.clone();
    }
}

/// Bounding box of pixels that differ between two same-size pixmaps.
pub fn diff_bbox(a: &Pixmap, b: &Pixmap) -> Option<IntRect> {
    let w = a.width() as usize;
    let h = a.height() as usize;
    let pa = a.pixels();
    let pb = b.pixels();
    let (mut minx, mut miny, mut maxx, mut maxy) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..h {
        let ra = &pa[y * w..(y + 1) * w];
        let rb = &pb[y * w..(y + 1) * w];
        if ra == rb {
            continue;
        }
        let first = ra.iter().zip(rb).position(|(p, q)| p != q).unwrap();
        let last = w - 1 - ra.iter().rev().zip(rb.iter().rev()).position(|(p, q)| p != q).unwrap();
        minx = minx.min(first);
        maxx = maxx.max(last);
        miny = miny.min(y);
        maxy = y;
    }
    if minx == usize::MAX {
        return None;
    }
    IntRect::from_ltrb(minx as i32, miny as i32, maxx as i32 + 1, maxy as i32 + 1)
}

pub fn to_color_image(pm: &Pixmap) -> egui::ColorImage {
    egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data())
}

pub fn from_rgba_unmul(w: u32, h: u32, rgba: &[u8]) -> Option<Pixmap> {
    let mut pm = Pixmap::new(w, h)?;
    for (dst, s) in pm.pixels_mut().iter_mut().zip(rgba.chunks_exact(4)) {
        let a = s[3] as u16;
        let m = |c: u8| ((c as u16 * a + 127) / 255) as u8;
        *dst = PremultipliedColorU8::from_rgba(m(s[0]), m(s[1]), m(s[2]), s[3]).unwrap();
    }
    Some(pm)
}

pub fn to_rgba_unmul(pm: &Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}

/// Copy a rect out of `pm`, keeping only pixels where `mask` (same size as rect) is non-zero.
pub fn extract(pm: &Pixmap, rect: IntRect, mask: Option<&[u8]>) -> Option<Pixmap> {
    let mut out = pm.clone_rect(rect)?;
    if let Some(mask) = mask {
        for (p, &m) in out.pixels_mut().iter_mut().zip(mask) {
            if m == 0 {
                *p = PremultipliedColorU8::TRANSPARENT;
            }
        }
    }
    Some(out)
}

/// Fill a rect (optionally masked) with a solid color, no blending.
pub fn fill_region(pm: &mut Pixmap, rect: IntRect, mask: Option<&[u8]>, c: Color32) {
    let p = premul(c);
    let w = rect.width() as i32;
    for yy in 0..rect.height() as i32 {
        for xx in 0..w {
            if mask.is_none_or(|m| m[(yy * w + xx) as usize] != 0) {
                set_px(pm, rect.x() + xx, rect.y() + yy, p);
            }
        }
    }
}

/// Make pixels matching `key` fully transparent (MS Paint's "transparent selection").
pub fn key_out(pm: &mut Pixmap, key: Color32) {
    let k = premul(key);
    for p in pm.pixels_mut() {
        if *p == k {
            *p = PremultipliedColorU8::TRANSPARENT;
        }
    }
}

/// Grow the canvas to at least (w, h), filling new space with `bg`.
pub fn grow(pm: &Pixmap, w: u32, h: u32, bg: Color32) -> Pixmap {
    let mut out = new_canvas(w.max(pm.width()), h.max(pm.height()), bg);
    copy_into(&mut out, pm, 0, 0);
    out
}
