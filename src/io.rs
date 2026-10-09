//! Copyright (c) 2026 salim-slimani. Licensed under MIT. See LICENSE-MIT.
//! PhotoStep IO: images + PSD + native .pstep project files.

use anyhow::{Context, Result};
use crate::core::{BlendMode, Document, Layer};

/// Load any image format supported by `image` crate into a new document.
/// Applies the EXIF orientation flag so phone photos open upright, and
/// resamples nothing — pixels stay exactly as decoded.
pub fn load_image(path: &str) -> Result<Document> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {path}"))?;
    load_image_bytes(&bytes, path)
}

/// Decode image bytes (file picker, browser upload, tests) into a document.
pub fn load_image_bytes(bytes: &[u8], name: &str) -> Result<Document> {
    let decoded = image::load_from_memory(bytes).with_context(|| format!("cannot decode {name}"))?;
    let img = apply_exif_orientation(decoded, bytes);
    let (w, h) = (img.width(), img.height());
    let mut doc = Document::new(w, h, [0, 0, 0, 0]);
    doc.layers[0].name = "Background".into();
    doc.layers[0].pixels = img.into_raw();
    Ok(doc)
}

/// Encode the flattened composite as PNG bytes (browser download).
pub fn encode_png(doc: &Document) -> Result<Vec<u8>> {
    use image::ImageEncoder as _;
    let flat = doc.composite();
    let mut out = Vec::new();
    let mut enc = image::codecs::png::PngEncoder::new(&mut out);
    enc.write_image(&flat, doc.width, doc.height, image::ExtendedColorType::Rgba8)?;
    Ok(out)
}

/// Honor the EXIF orientation flag (phones store rotation here instead of
/// rotating pixels). Unknown/missing flags leave the image untouched.
fn apply_exif_orientation(img: image::DynamicImage, bytes: &[u8]) -> image::RgbaImage {
    use image::imageops::{flip_horizontal, flip_vertical, rotate180, rotate270, rotate90};
    let mut cursor = std::io::Cursor::new(bytes);
    let orientation = exif::Reader::new()
        .read_from_container(&mut cursor)
        .ok()
        .and_then(|ex| ex.get_field(exif::Tag::Orientation, exif::In::PRIMARY).cloned())
        .and_then(|f| f.value.get_uint(0));
    let rgba = img.to_rgba8();
    match orientation.unwrap_or(1) {
        2 => flip_horizontal(&rgba),
        3 => rotate180(&rgba),
        4 => flip_vertical(&rgba),
        5 => flip_horizontal(&rotate90(&rgba)),
        6 => rotate90(&rgba),
        7 => flip_horizontal(&rotate270(&rgba)),
        8 => rotate270(&rgba),
        _ => rgba,
    }
}

/// Load a Photoshop PSD file: every pixel layer becomes a PhotoStep layer
/// (name, visibility, opacity and blend mode preserved where mappable).
/// Adjustment/text/shape layers arrive as their rendered pixels, if present.
pub fn load_psd(path: &str) -> Result<Document> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {path}"))?;
    load_psd_bytes(&bytes)
}

pub fn load_psd_bytes(bytes: &[u8]) -> Result<Document> {
    let psd = psd::Psd::from_bytes(bytes).map_err(|e| anyhow::anyhow!("psd parse failed: {e:?}"))?;
    let (w, h) = (psd.width(), psd.height());
    anyhow::ensure!(w > 0 && h > 0, "empty psd canvas");
    let mut doc = Document::new(w, h, [0, 0, 0, 0]);
    doc.layers.clear();
    let layers = psd.layers();
    if layers.is_empty() {
        // single-background file: use the flattened composite
        let flat = psd.rgba();
        if flat.len() == w as usize * h as usize * 4 {
            doc.layers.push(Layer::from_rgba("Background", w, h, flat)?);
        }
    } else {
        for layer in layers.iter() {
            let rgba = layer.rgba();
            if rgba.len() != w as usize * h as usize * 4 {
                continue;
            }
            let mut l = Layer::from_rgba(layer.name(), w, h, rgba)?;
            l.visible = layer.visible();
            l.opacity = (layer.opacity() as f32 / 255.0).clamp(0.0, 1.0);
            // map by debug name (robust to psd-crate variant renames)
            l.blend = BlendMode::from_name(&format!("{:?}", layer.blend_mode()));
            doc.layers.push(l);
        }
    }
    if doc.layers.is_empty() {
        doc.layers.push(Layer::new_solid("Background", w, h, [0, 0, 0, 0]));
    }
    doc.active = doc.layers.len() - 1;
    Ok(doc)
}

/// Save flattened composite. Format deduced from extension.
/// JPEG exports at quality 93 (visually lossless, sane file sizes).
pub fn save_image(doc: &Document, path: &str) -> Result<()> {
    let flat = doc.composite();
    let buf: image::RgbaImage =
        image::ImageBuffer::from_raw(doc.width, doc.height, flat).context("bad buffer")?;
    if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        let rgb = image::DynamicImage::ImageRgba8(buf).to_rgb8();
        let (w, h) = (rgb.width(), rgb.height());
        let file = std::fs::File::create(path)?;
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 93);
        use image::ImageEncoder as _;
        enc.write_image(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8)?;
    } else {
        buf.save(path)?;
    }
    Ok(())
}

/// Native project: JSON with base64-ish raw pixels stored as-is via serde_json (simple, portable).
pub fn save_project(doc: &Document, path: &str) -> Result<()> {
    let s = serde_json::to_string_pretty(doc)?;
    std::fs::write(path, s)?;
    Ok(())
}

pub fn load_project(path: &str) -> Result<Document> {
    let s = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}
