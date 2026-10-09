//! PhotoStep IO: images + native .pstep project files.

use anyhow::{Context, Result};
use crate::core::Document;

/// Load any image format supported by `image` crate into a new document.
pub fn load_image(path: &str) -> Result<Document> {
    let img = image::open(path).with_context(|| format!("cannot open {path}"))?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    let mut doc = Document::new(w, h, [0, 0, 0, 0]);
    doc.layers[0].name = "Background".into();
    doc.layers[0].pixels = img.into_raw();
    Ok(doc)
}

/// Save flattened composite. Format deduced from extension.
pub fn save_image(doc: &Document, path: &str) -> Result<()> {
    let flat = doc.composite();
    let buf: image::RgbaImage =
        image::ImageBuffer::from_raw(doc.width, doc.height, flat).context("bad buffer")?;
    if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        let rgb = image::DynamicImage::ImageRgba8(buf).to_rgb8();
        rgb.save(path)?;
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
