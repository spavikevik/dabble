//! Loading and saving images.

use std::path::Path;
use tiny_skia::Pixmap;

use crate::raster;

pub const OPEN_EXTS: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "ico"];
pub const SAVE_FORMATS: &[(&str, &[&str])] = &[
    ("PNG", &["png"]),
    ("JPEG", &["jpg", "jpeg"]),
    ("BMP", &["bmp"]),
    ("GIF", &["gif"]),
    ("TIFF", &["tif", "tiff"]),
    ("WebP", &["webp"]),
];

pub fn load(path: &Path) -> Result<Pixmap, String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    raster::from_rgba_unmul(img.width(), img.height(), img.as_raw())
        .ok_or_else(|| "image has zero size".to_string())
}

pub fn save(pm: &Pixmap, path: &Path) -> Result<(), String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    let (w, h) = (pm.width(), pm.height());
    let rgba = raster::to_rgba_unmul(pm);
    match ext.as_str() {
        // No alpha channel in these formats: flatten onto white.
        "jpg" | "jpeg" | "bmp" => {
            let mut rgb = Vec::with_capacity((w * h * 3) as usize);
            for p in pm.pixels() {
                let a = p.alpha() as u16;
                let over = |c: u8| (c as u16 + (255 - a)).min(255) as u8;
                rgb.extend_from_slice(&[over(p.red()), over(p.green()), over(p.blue())]);
            }
            let img = image::RgbImage::from_raw(w, h, rgb).ok_or("bad buffer")?;
            if ext == "bmp" {
                img.save(path).map_err(|e| e.to_string())
            } else {
                let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
                let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    std::io::BufWriter::new(file),
                    92,
                );
                enc.encode_image(&img).map_err(|e| e.to_string())
            }
        }
        _ => {
            let img = image::RgbaImage::from_raw(w, h, rgba).ok_or("bad buffer")?;
            img.save(path).map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Color32;

    #[test]
    fn roundtrip_png_and_jpg() {
        let dir = std::env::temp_dir().join(format!("daub-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut pm = raster::new_canvas(8, 6, Color32::WHITE);
        raster::set_px(&mut pm, 2, 3, raster::premul(Color32::from_rgb(10, 200, 30)));
        for name in ["a.png", "a.jpg", "a.bmp", "a.gif", "a.webp", "a.tiff"] {
            let p = dir.join(name);
            save(&pm, &p).unwrap();
            let back = load(&p).unwrap();
            assert_eq!((back.width(), back.height()), (8, 6), "{name}");
        }
        let back = load(&dir.join("a.png")).unwrap();
        assert_eq!(back.data(), pm.data());
        std::fs::remove_dir_all(dir).ok();
    }
}
