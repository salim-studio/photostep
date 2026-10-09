//! PhotoStep core: document, layers, blending, history.
//! Designed for speed: flat RGBA8 buffers + rayon parallelism in ops,
//! cheap snapshots with a bounded history stack.

use serde::{Deserialize, Serialize};

/// Blend modes (subset of Photoshop, fastest math first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Difference,
    Exclusion,
    HardLight,
    SoftLight,
    ColorDodge,
    ColorBurn,
    LinearLight,
}

impl BlendMode {
    pub fn all() -> &'static [BlendMode] {
        use BlendMode::*;
        &[
            Normal, Multiply, Screen, Overlay, Darken, Lighten, Difference, Exclusion,
            HardLight, SoftLight, ColorDodge, ColorBurn, LinearLight,
        ]
    }
    pub fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::HardLight => "Hard Light",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::LinearLight => "Linear Light",
        }
    }
    pub fn from_name(s: &str) -> Self {
        for m in Self::all() {
            if m.name().eq_ignore_ascii_case(s) {
                return *m;
            }
        }
        BlendMode::Normal
    }
}

#[inline]
fn ch_blend(mode: BlendMode, s: f32, d: f32) -> f32 {
    match mode {
        BlendMode::Normal => s,
        BlendMode::Multiply => s * d,
        BlendMode::Screen => 1.0 - (1.0 - s) * (1.0 - d),
        BlendMode::Overlay => {
            if d < 0.5 { 2.0 * s * d } else { 1.0 - 2.0 * (1.0 - s) * (1.0 - d) }
        }
        BlendMode::Darken => s.min(d),
        BlendMode::Lighten => s.max(d),
        BlendMode::Difference => (s - d).abs(),
        BlendMode::Exclusion => s + d - 2.0 * s * d,
        BlendMode::HardLight => {
            if s < 0.5 { 2.0 * s * d } else { 1.0 - 2.0 * (1.0 - s) * (1.0 - d) }
        }
        BlendMode::SoftLight => {
            // Pegtop approximation (fast)
            (1.0 - 2.0 * s) * d * d + 2.0 * s * d
        }
        BlendMode::ColorDodge => {
            if s >= 1.0 { 1.0 } else { (d / (1.0 - s)).min(1.0) }
        }
        BlendMode::ColorBurn => {
            if s <= 0.0 { 0.0 } else { (1.0 - (1.0 - d) / s).max(0.0) }
        }
        BlendMode::LinearLight => (d + 2.0 * s - 1.0).clamp(0.0, 1.0),
    }
}

/// Composite one src pixel over dst pixel with blend + opacity. All channels 0..255.
#[inline]
pub fn composite_pixel(mode: BlendMode, opacity: f32, src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let sa = (src[3] as f32 / 255.0) * opacity;
    if sa <= 0.0 {
        return dst;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mut out = [0u8; 4];
    for i in 0..3 {
        let s = src[i] as f32 / 255.0;
        let d = dst[i] as f32 / 255.0;
        let b = ch_blend(mode, s, d);
        // src-over with blend
        let v = (b * sa + d * da * (1.0 - sa)) / out_a;
        out[i] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub opacity: f32, // 0..1
    pub blend: BlendMode,
    pub width: u32,
    pub height: u32,
    /// RGBA8, len = w*h*4
    pub pixels: Vec<u8>,
}

impl Layer {
    pub fn new_solid(name: &str, w: u32, h: u32, color: [u8; 4]) -> Self {
        let n = (w as usize) * (h as usize);
        let mut pixels = Vec::with_capacity(n * 4);
        for _ in 0..n {
            pixels.extend_from_slice(&color);
        }
        Self { name: name.to_owned(), visible: true, opacity: 1.0, blend: BlendMode::Normal, width: w, height: h, pixels }
    }

    pub fn from_rgba(name: &str, w: u32, h: u32, pixels: Vec<u8>) -> anyhow::Result<Self> {
        anyhow::ensure!(pixels.len() == w as usize * h as usize * 4, "bad pixel buffer");
        Ok(Self { name: name.to_owned(), visible: true, opacity: 1.0, blend: BlendMode::Normal, width: w, height: h, pixels })
    }

    pub fn fill(&mut self, color: [u8; 4]) {
        for px in self.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&color);
        }
    }

    pub fn resize_to(&mut self, w: u32, h: u32) {
        if w == self.width && h == self.height {
            return;
        }
        let img: image::RgbaImage =
            image::ImageBuffer::from_raw(self.width, self.height, std::mem::take(&mut self.pixels))
                .unwrap_or_else(|| image::ImageBuffer::new(self.width, self.height));
        let resized = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
        self.width = w;
        self.height = h;
        self.pixels = resized.into_raw();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub active: usize,
}

impl Document {
    pub fn new(width: u32, height: u32, bg: [u8; 4]) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            width: w,
            height: h,
            layers: vec![Layer::new_solid("Background", w, h, bg)],
            active: 0,
        }
    }

    pub fn active_layer(&self) -> &Layer {
        &self.layers[self.active.min(self.layers.len().saturating_sub(1).max(0))]
    }

    pub fn active_layer_mut(&mut self) -> &mut Layer {
        let i = self.active.min(self.layers.len().saturating_sub(1).max(0));
        &mut self.layers[i]
    }

    pub fn add_layer(&mut self, layer: Layer) {
        let mut l = layer;
        l.resize_to(self.width, self.height);
        self.layers.push(l);
        self.active = self.layers.len() - 1;
    }

    pub fn add_solid_layer(&mut self, name: &str, color: [u8; 4]) {
        self.add_layer(Layer::new_solid(name, self.width, self.height, color));
    }

    pub fn duplicate_active(&mut self) {
        if self.layers.is_empty() {
            return;
        }
        let mut c = self.active_layer().clone();
        c.name = format!("{} copy", c.name);
        let pos = (self.active + 1).min(self.layers.len());
        self.layers.insert(pos, c);
        self.active = pos;
    }

    pub fn remove_active(&mut self) {
        if self.layers.len() <= 1 {
            return;
        }
        self.layers.remove(self.active);
        self.active = self.active.min(self.layers.len() - 1);
    }

    pub fn move_active(&mut self, up: bool) {
        let n = self.layers.len();
        if n < 2 {
            return;
        }
        if up && self.active + 1 < n {
            self.layers.swap(self.active, self.active + 1);
            self.active += 1;
        } else if !up && self.active > 0 {
            self.layers.swap(self.active, self.active - 1);
            self.active -= 1;
        }
    }

    /// Flatten all visible layers bottom-up. Returns RGBA8 buffer.
    pub fn composite(&self) -> Vec<u8> {
        let n = self.width as usize * self.height as usize * 4;
        let mut out = vec![0u8; n];
        for layer in &self.layers {
            if !layer.visible {
                continue;
            }
            if layer.width != self.width || layer.height != self.height {
                continue;
            }
            let op = layer.opacity.clamp(0.0, 1.0);
            let mode = layer.blend;
            if mode == BlendMode::Normal && (op - 1.0).abs() < 1e-6 {
                // fast path: plain src-over
                for (d, s) in out.chunks_exact_mut(4).zip(layer.pixels.chunks_exact(4)) {
                    let sa = s[3] as u32;
                    if sa == 0 {
                        continue;
                    }
                    if sa == 255 && d[3] == 0 {
                        d.copy_from_slice(s);
                        continue;
                    }
                    let sa_f = sa as f32 / 255.0;
                    let da_f = d[3] as f32 / 255.0;
                    let oa = sa_f + da_f * (1.0 - sa_f);
                    for i in 0..3 {
                        let v = (s[i] as f32 * sa_f + d[i] as f32 * da_f * (1.0 - sa_f)) / oa.max(1e-6);
                        d[i] = v.round() as u8;
                    }
                    d[3] = (oa * 255.0).round() as u8;
                }
            } else {
                for (d, s) in out.chunks_exact_mut(4).zip(layer.pixels.chunks_exact(4)) {
                    let dd = [d[0], d[1], d[2], d[3]];
                    let ss = [s[0], s[1], s[2], s[3]];
                    d.copy_from_slice(&composite_pixel(mode, op, ss, dd));
                }
            }
        }
        out
    }

    /// Merge active layer down into the layer below.
    pub fn merge_down(&mut self) {
        if self.active == 0 || self.layers.len() < 2 {
            return;
        }
        let top = self.layers.remove(self.active);
        let below = &mut self.layers[self.active - 1];
        if !top.visible {
            self.active -= 1;
            return;
        }
        let op = top.opacity.clamp(0.0, 1.0);
        for (d, s) in below.pixels.chunks_exact_mut(4).zip(top.pixels.chunks_exact(4)) {
            let dd = [d[0], d[1], d[2], d[3]];
            let ss = [s[0], s[1], s[2], s[3]];
            d.copy_from_slice(&composite_pixel(top.blend, op, ss, dd));
        }
        self.active -= 1;
    }

    pub fn flatten(&mut self) {
        let pixels = self.composite();
        self.layers = vec![Layer {
            name: "Background".into(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            width: self.width,
            height: self.height,
            pixels,
        }];
        self.active = 0;
    }
}

/// Bounded undo/redo stack of full document snapshots.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Document>,
    redo: Vec<Document>,
    limit: usize,
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit: limit.max(5).min(100) }
    }
    pub fn push(&mut self, doc: &Document) {
        if self.limit == 0 {
            return;
        }
        if self.undo.len() >= self.limit {
            self.undo.remove(0);
        }
        self.undo.push(doc.clone());
        self.redo.clear();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self, current: &Document) -> Option<Document> {
        let prev = self.undo.pop()?;
        if self.redo.len() >= self.limit {
            self.redo.remove(0);
        }
        self.redo.push(current.clone());
        Some(prev)
    }
    pub fn redo(&mut self, current: &Document) -> Option<Document> {
        let next = self.redo.pop()?;
        if self.undo.len() >= self.limit {
            self.undo.remove(0);
        }
        self.undo.push(current.clone());
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_normal_opaque() {
        let d = composite_pixel(BlendMode::Normal, 1.0, [255, 0, 0, 255], [0, 0, 255, 255]);
        assert_eq!(d, [255, 0, 0, 255]);
    }

    #[test]
    fn composite_two_layers() {
        let mut doc = Document::new(2, 2, [0, 0, 255, 255]);
        doc.add_solid_layer("red", [255, 0, 0, 128]);
        let out = doc.composite();
        assert_eq!(out.len(), 16);
        // red@50% over blue => purple-ish
        assert!(out[0] > 100 && out[2] > 100);
    }
}
