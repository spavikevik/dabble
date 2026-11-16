//! Draws Dabble's app icon and writes a macOS .iconset folder.
//! Run by `scripts/bundle.sh`: `cargo run --example make_icon -- <out.iconset>`

use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LinearGradient, Paint, PathBuilder, Pixmap, Point, Rect, SpreadMode,
    Stroke, Transform,
};

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> tiny_skia::Path {
    let k = 0.552_284_8 * r;
    let (l, t, rt, b) = (x, y, x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(l + r, t);
    pb.line_to(rt - r, t);
    pb.cubic_to(rt - r + k, t, rt, t + r - k, rt, t + r);
    pb.line_to(rt, b - r);
    pb.cubic_to(rt, b - r + k, rt - r + k, b, rt - r, b);
    pb.line_to(l + r, b);
    pb.cubic_to(l + r - k, b, l, b - r + k, l, b - r);
    pb.line_to(l, t + r);
    pb.cubic_to(l, t + r - k, l + r - k, t, l + r, t);
    pb.close();
    pb.finish().unwrap()
}

fn solid(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
    p.anti_alias = true;
    p
}

/// The icon at 1024×1024, in the macOS icon grid (824px body, 100px margin).
fn draw() -> Pixmap {
    let mut pm = Pixmap::new(1024, 1024).unwrap();
    let id = Transform::identity();

    // Soft shadow, then the body with a diagonal blue-to-teal gradient.
    let shadow = rounded(100.0, 112.0, 824.0, 824.0, 185.0);
    pm.fill_path(&shadow, &solid(0, 0, 0, 50), FillRule::Winding, Transform::from_translate(0.0, 6.0), None);
    let body = rounded(100.0, 100.0, 824.0, 824.0, 185.0);
    let grad = LinearGradient::new(
        Point::from_xy(100.0, 100.0),
        Point::from_xy(924.0, 924.0),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(36, 99, 235, 255)),
            GradientStop::new(1.0, Color::from_rgba8(13, 178, 170, 255)),
        ],
        SpreadMode::Pad,
        id,
    )
    .unwrap();
    let paint = Paint { shader: grad, anti_alias: true, ..Default::default() };
    pm.fill_path(&body, &paint, FillRule::Winding, id, None);

    // A white sheet, slightly tilted.
    let sheet = rounded(250.0, 230.0, 524.0, 564.0, 40.0);
    let tilt = Transform::from_rotate_at(-6.0, 512.0, 512.0);
    pm.fill_path(&sheet, &solid(0, 0, 0, 40), FillRule::Winding, tilt.pre_translate(0.0, 14.0), None);
    pm.fill_path(&sheet, &solid(255, 255, 255, 255), FillRule::Winding, tilt, None);

    // Three dabbles of paint across the sheet.
    type Dabble = ((u8, u8, u8), [(f32, f32); 4], f32);
    let strokes: [Dabble; 3] = [
        ((249, 115, 22), [(320.0, 420.0), (430.0, 300.0), (560.0, 470.0), (700.0, 350.0)], 78.0),
        ((236, 72, 153), [(310.0, 560.0), (440.0, 470.0), (560.0, 640.0), (710.0, 520.0)], 78.0),
        ((250, 204, 21), [(330.0, 700.0), (450.0, 640.0), (560.0, 760.0), (690.0, 680.0)], 64.0),
    ];
    for ((r, g, b), pts, w) in strokes {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        pb.cubic_to(pts[1].0, pts[1].1, pts[2].0, pts[2].1, pts[3].0, pts[3].1);
        let path = pb.finish().unwrap();
        let stroke = Stroke { width: w, line_cap: LineCap::Round, ..Default::default() };
        pm.stroke_path(&path, &solid(r, g, b, 255), &stroke, tilt, None);
    }

    // Brush: handle and ferrule lying across the corner.
    let brush = Transform::from_rotate_at(40.0, 512.0, 512.0);
    let handle = rounded(470.0, 30.0, 84.0, 360.0, 42.0);
    pm.fill_path(&handle, &solid(30, 41, 59, 255), FillRule::Winding, brush.pre_translate(250.0, 0.0), None);
    let ferrule = Rect::from_xywh(478.0, 370.0, 68.0, 70.0).unwrap();
    pm.fill_rect(ferrule, &solid(203, 213, 225, 255), brush.pre_translate(250.0, 0.0), None);
    let mut tip = PathBuilder::new();
    tip.move_to(478.0, 440.0);
    tip.cubic_to(470.0, 500.0, 500.0, 540.0, 512.0, 560.0);
    tip.cubic_to(524.0, 540.0, 554.0, 500.0, 546.0, 440.0);
    tip.close();
    pm.fill_path(&tip.finish().unwrap(), &solid(249, 115, 22, 255), FillRule::Winding, brush.pre_translate(250.0, 0.0), None);
    pm
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: make_icon <out.iconset>"));
    std::fs::create_dir_all(&out).unwrap();
    let big = draw();
    let rgba: Vec<u8> = big
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let full = image::RgbaImage::from_raw(1024, 1024, rgba).unwrap();
    for (size, name) in [
        (16, "icon_16x16"),
        (32, "icon_16x16@2x"),
        (32, "icon_32x32"),
        (64, "icon_32x32@2x"),
        (128, "icon_128x128"),
        (256, "icon_128x128@2x"),
        (256, "icon_256x256"),
        (512, "icon_256x256@2x"),
        (512, "icon_512x512"),
        (1024, "icon_512x512@2x"),
    ] {
        // Downscale with a proper filter so small sizes don't alias.
        let img = image::imageops::resize(&full, size, size, image::imageops::FilterType::Lanczos3);
        img.save(out.join(format!("{name}.png"))).unwrap();
    }
}
