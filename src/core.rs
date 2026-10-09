//! PhotoStep core: document, layers, blending, masks, adjustments, effects, history.
//! Designed for speed: flat RGBA8 buffers + rayon parallelism in ops,
//! cheap snapshots with a bounded, labeled history stack.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Blend modes
// ---------------------------------------------------------------------------

/// Blend modes (Photoshop-compatible math, fastest first).
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
    VividLight,
    PinLight,
    HardMix,
    Subtract,
    Divide,
}

impl BlendMode {
    pub fn all() -> &'static [BlendMode] {
        use BlendMode::*;
        &[
            Normal, Multiply, Screen, Overlay, Darken, Lighten, Difference, Exclusion,
            HardLight, SoftLight, ColorDodge, ColorBurn, LinearLight, VividLight,
            PinLight, HardMix, Subtract, Divide,
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
            BlendMode::VividLight => "Vivid Light",
            BlendMode::PinLight => "Pin Light",
            BlendMode::HardMix => "Hard Mix",
            BlendMode::Subtract => "Subtract",
            BlendMode::Divide => "Divide",
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
        BlendMode::VividLight => {
            if s < 0.5 {
                if (2.0 * s) <= 0.0 { 0.0 } else { (1.0 - (1.0 - d) / (2.0 * s)).max(0.0) }
            } else if (2.0 * (s - 0.5)) >= 1.0 {
                1.0
            } else {
                (d / (1.0 - 2.0 * (s - 0.5))).min(1.0)
            }
        }
        BlendMode::PinLight => {
            if s < 0.5 { d.min(2.0 * s) } else { d.max(2.0 * (s - 0.5)) }
        }
        BlendMode::HardMix => {
            if s + d >= 1.0 { 1.0 } else { 0.0 }
        }
        BlendMode::Subtract => (d - s).max(0.0),
        BlendMode::Divide => {
            if s <= 0.0 { 1.0 } else { (d / s).min(1.0) }
        }
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

/// Plain src-over of an RGBA buffer onto another (used for layer effects).
fn src_over(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let sa = s[3] as f32 / 255.0;
        if sa <= 0.0 {
            continue;
        }
        if sa >= 1.0 {
            d.copy_from_slice(s);
            continue;
        }
        let da = d[3] as f32 / 255.0;
        let oa = sa + da * (1.0 - sa);
        for i in 0..3 {
            d[i] = ((s[i] as f32 * sa + d[i] as f32 * da * (1.0 - sa)) / oa.max(1e-6)).round() as u8;
        }
        d[3] = (oa * 255.0).round() as u8;
    }
}

// ---------------------------------------------------------------------------
// Adjustments (parameters live here; pixel math lives in ops)
// ---------------------------------------------------------------------------

/// A non-destructive adjustment: stored on an adjustment layer and evaluated
/// at composite time, or baked into pixels by the direct-apply commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Adjustment {
    BrightnessContrast { brightness: i16, contrast: f32 },
    Levels { in_lo: u8, in_hi: u8, gamma: f32 },
    Curves { points: Vec<(u8, u8)> },
    HueSaturation { hue_deg: f32, sat_mult: f32, lightness: f32 },
    Vibrance { amount: f32 },
    Exposure { ev: f32 },
    ColorBalance { dr: i16, dg: i16, db: i16 },
    BlackWhite { r: f32, g: f32, b: f32 },
    PhotoFilter { color: [u8; 3], density: f32 },
    ChannelMixer { r: [f32; 3], g: [f32; 3], b: [f32; 3] },
    GradientMap { dark: [u8; 3], light: [u8; 3] },
    ShadowsHighlights { shadows: f32, highlights: f32 },
    Threshold { t: u8 },
    Posterize { levels: u8 },
    Invert,
    Grayscale,
    AutoContrast,
}

impl Adjustment {
    pub fn name(&self) -> &'static str {
        match self {
            Adjustment::BrightnessContrast { .. } => "Brightness/Contrast",
            Adjustment::Levels { .. } => "Levels",
            Adjustment::Curves { .. } => "Curves",
            Adjustment::HueSaturation { .. } => "Hue/Saturation",
            Adjustment::Vibrance { .. } => "Vibrance",
            Adjustment::Exposure { .. } => "Exposure",
            Adjustment::ColorBalance { .. } => "Color Balance",
            Adjustment::BlackWhite { .. } => "Black & White",
            Adjustment::PhotoFilter { .. } => "Photo Filter",
            Adjustment::ChannelMixer { .. } => "Channel Mixer",
            Adjustment::GradientMap { .. } => "Gradient Map",
            Adjustment::ShadowsHighlights { .. } => "Shadows/Highlights",
            Adjustment::Threshold { .. } => "Threshold",
            Adjustment::Posterize { .. } => "Posterize",
            Adjustment::Invert => "Invert",
            Adjustment::Grayscale => "Grayscale",
            Adjustment::AutoContrast => "Auto Contrast",
        }
    }

    /// Default instances for the "New Adjustment Layer" gallery.
    pub fn gallery() -> Vec<(&'static str, Adjustment)> {
        vec![
            ("Brightness/Contrast", Adjustment::BrightnessContrast { brightness: 0, contrast: 0.0 }),
            ("Levels", Adjustment::Levels { in_lo: 0, in_hi: 255, gamma: 1.0 }),
            ("Curves", Adjustment::Curves { points: vec![(0, 0), (64, 56), (192, 200), (255, 255)] }),
            ("Hue/Saturation", Adjustment::HueSaturation { hue_deg: 0.0, sat_mult: 1.0, lightness: 0.0 }),
            ("Vibrance", Adjustment::Vibrance { amount: 0.0 }),
            ("Exposure", Adjustment::Exposure { ev: 0.0 }),
            ("Color Balance", Adjustment::ColorBalance { dr: 0, dg: 0, db: 0 }),
            ("Black & White", Adjustment::BlackWhite { r: 0.299, g: 0.587, b: 0.114 }),
            ("Photo Filter", Adjustment::PhotoFilter { color: [255, 128, 0], density: 0.25 }),
            ("Channel Mixer", Adjustment::ChannelMixer { r: [1.0, 0.0, 0.0], g: [0.0, 1.0, 0.0], b: [0.0, 0.0, 1.0] }),
            ("Gradient Map", Adjustment::GradientMap { dark: [0, 0, 0], light: [255, 255, 255] }),
            ("Shadows/Highlights", Adjustment::ShadowsHighlights { shadows: 0.0, highlights: 0.0 }),
            ("Threshold", Adjustment::Threshold { t: 128 }),
            ("Posterize", Adjustment::Posterize { levels: 4 }),
            ("Invert", Adjustment::Invert),
            ("Grayscale", Adjustment::Grayscale),
            ("Auto Contrast", Adjustment::AutoContrast),
        ]
    }
}

// ---------------------------------------------------------------------------
// Layer styles (effects)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DropShadow {
    pub dx: i32,
    pub dy: i32,
    pub blur: u32,
    pub opacity: f32,
    pub color: [u8; 3],
}

impl Default for DropShadow {
    fn default() -> Self {
        Self { dx: 8, dy: 8, blur: 12, opacity: 0.6, color: [0, 0, 0] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OuterGlow {
    pub blur: u32,
    pub opacity: f32,
    pub color: [u8; 3],
}

impl Default for OuterGlow {
    fn default() -> Self {
        Self { blur: 16, opacity: 0.7, color: [255, 176, 58] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayerStroke {
    pub width: u32,
    pub opacity: f32,
    pub color: [u8; 3],
}

impl Default for LayerStroke {
    fn default() -> Self {
        Self { width: 4, opacity: 1.0, color: [255, 90, 40] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct LayerEffects {
    pub drop_shadow: Option<DropShadow>,
    pub outer_glow: Option<OuterGlow>,
    pub stroke: Option<LayerStroke>,
}

impl LayerEffects {
    pub fn has_any(&self) -> bool {
        self.drop_shadow.is_some() || self.outer_glow.is_some() || self.stroke.is_some()
    }
}

// ---------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum LayerKind {
    #[default]
    Pixel,
    Adjustment(Adjustment),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub opacity: f32, // 0..1
    pub blend: BlendMode,
    pub width: u32,
    pub height: u32,
    /// RGBA8, len = w*h*4 (empty for adjustment layers).
    pub pixels: Vec<u8>,
    /// Layer kind: plain pixels or a live adjustment.
    #[serde(default)]
    pub kind: LayerKind,
    /// Optional grayscale mask (0 = hidden, 255 = shown), len = w*h.
    #[serde(default)]
    pub mask: Option<Vec<u8>>,
    /// Drop shadow / glow / stroke, rendered under the layer at composite time.
    #[serde(default)]
    pub effects: LayerEffects,
}

impl Layer {
    pub fn new_solid(name: &str, w: u32, h: u32, color: [u8; 4]) -> Self {
        let n = (w as usize) * (h as usize);
        let mut pixels = Vec::with_capacity(n * 4);
        for _ in 0..n {
            pixels.extend_from_slice(&color);
        }
        Self {
            name: name.to_owned(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            width: w,
            height: h,
            pixels,
            kind: LayerKind::Pixel,
            mask: None,
            effects: LayerEffects::default(),
        }
    }

    pub fn new_adjustment(name: &str, w: u32, h: u32, adj: Adjustment) -> Self {
        Self {
            name: name.to_owned(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            width: w,
            height: h,
            pixels: Vec::new(),
            kind: LayerKind::Adjustment(adj),
            mask: None,
            effects: LayerEffects::default(),
        }
    }

    pub fn from_rgba(name: &str, w: u32, h: u32, pixels: Vec<u8>) -> anyhow::Result<Self> {
        anyhow::ensure!(pixels.len() == w as usize * h as usize * 4, "bad pixel buffer");
        Ok(Self {
            name: name.to_owned(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            width: w,
            height: h,
            pixels,
            kind: LayerKind::Pixel,
            mask: None,
            effects: LayerEffects::default(),
        })
    }

    pub fn is_adjustment(&self) -> bool {
        matches!(self.kind, LayerKind::Adjustment(_))
    }

    pub fn adjustment(&self) -> Option<&Adjustment> {
        match &self.kind {
            LayerKind::Adjustment(a) => Some(a),
            _ => None,
        }
    }

    pub fn adjustment_mut(&mut self) -> Option<&mut Adjustment> {
        match &mut self.kind {
            LayerKind::Adjustment(a) => Some(a),
            _ => None,
        }
    }

    /// Create an all-white (reveal-all) mask if there isn't one yet.
    pub fn ensure_mask(&mut self) {
        let n = self.width as usize * self.height as usize;
        if self.mask.as_ref().is_some_and(|m| m.len() == n) {
            return;
        }
        self.mask = Some(vec![255u8; n]);
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
        if !self.pixels.is_empty() {
            let img: image::RgbaImage =
                image::ImageBuffer::from_raw(self.width, self.height, std::mem::take(&mut self.pixels))
                    .unwrap_or_else(|| image::ImageBuffer::new(self.width, self.height));
            let resized = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
            self.pixels = resized.into_raw();
        }
        if let Some(m) = self.mask.take() {
            let img: image::ImageBuffer<image::Luma<u8>, Vec<u8>> =
                image::ImageBuffer::from_raw(self.width, self.height, m)
                    .unwrap_or_else(|| image::ImageBuffer::new(self.width, self.height));
            let resized = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
            self.mask = Some(resized.into_raw());
        }
        self.width = w;
        self.height = h;
    }
}

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

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

    /// Nearest pixel layer at or below `active` (paint/ops target).
    pub fn paint_target(&self) -> Option<usize> {
        let mut i = self.active.min(self.layers.len().saturating_sub(1));
        loop {
            if matches!(self.layers[i].kind, LayerKind::Pixel) {
                return Some(i);
            }
            if i == 0 {
                return None;
            }
            i -= 1;
        }
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

    pub fn add_adjustment_layer(&mut self, name: &str, adj: Adjustment) {
        self.add_layer(Layer::new_adjustment(name, self.width, self.height, adj));
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

    /// Effective per-pixel alpha of a layer (alpha channel x mask), 0..255.
    fn effective_alpha(layer: &Layer, n: usize) -> Vec<u8> {
        let mut a = Vec::with_capacity(n);
        match &layer.mask {
            Some(m) if m.len() == n => {
                for (px, mm) in layer.pixels.chunks_exact(4).zip(m.iter()) {
                    a.push(((px[3] as u16 * *mm as u16) / 255) as u8);
                }
            }
            _ => {
                for px in layer.pixels.chunks_exact(4) {
                    a.push(px[3]);
                }
            }
        }
        a
    }

    /// Render a pixel layer's effects (shadow/glow/stroke) into `fx`.
    fn render_effects(&self, layer: &Layer, fx: &mut [u8]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let n = w * h;
        let ea = Self::effective_alpha(layer, n);
        let layer_op = layer.opacity.clamp(0.0, 1.0);

        if let Some(sh) = layer.effects.drop_shadow {
            let b = crate::ops::blur_channel(&ea, w, h, sh.blur as usize);
            let k = (sh.opacity * layer_op).clamp(0.0, 1.0);
            if k > 0.0 {
                for y in 0..h {
                    for x in 0..w {
                        let sx = x as i32 - sh.dx;
                        let sy = y as i32 - sh.dy;
                        if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                            continue;
                        }
                        let v = b[sy as usize * w + sx as usize] as f32 / 255.0 * k;
                        if v <= 0.0 {
                            continue;
                        }
                        let o = (y * w + x) * 4;
                        // src-over tinted shadow onto fx
                        let da = fx[o + 3] as f32 / 255.0;
                        let oa = v + da * (1.0 - v);
                        for c in 0..3 {
                            fx[o + c] = ((sh.color[c] as f32 * v + fx[o + c] as f32 * da * (1.0 - v))
                                / oa.max(1e-6))
                            .round() as u8;
                        }
                        fx[o + 3] = (oa * 255.0).round() as u8;
                    }
                }
            }
        }

        if let Some(gl) = layer.effects.outer_glow {
            let b = crate::ops::blur_channel(&ea, w, h, gl.blur as usize);
            let k = (gl.opacity * layer_op).clamp(0.0, 1.0);
            if k > 0.0 {
                for (o4, chunk) in fx.chunks_exact_mut(4).enumerate() {
                    let v = b[o4] as f32 / 255.0 * k;
                    if v <= 0.0 {
                        continue;
                    }
                    let da = chunk[3] as f32 / 255.0;
                    let oa = v + da * (1.0 - v);
                    for c in 0..3 {
                        chunk[c] = ((gl.color[c] as f32 * v + chunk[c] as f32 * da * (1.0 - v))
                            / oa.max(1e-6))
                        .round() as u8;
                    }
                    chunk[3] = (oa * 255.0).round() as u8;
                }
            }
        }

        if let Some(st) = layer.effects.stroke {
            let rad = (st.width.max(1)) as usize;
            let b = crate::ops::blur_channel(&ea, w, h, rad);
            let k = (st.opacity * layer_op).clamp(0.0, 1.0);
            if k > 0.0 {
                for i in 0..n {
                    // band just outside the shape: blurred minus actual, amplified
                    let s = ((b[i] as f32 - ea[i] as f32).max(0.0) * 6.0).min(255.0) / 255.0 * k;
                    if s <= 0.0 {
                        continue;
                    }
                    let o = i * 4;
                    let da = fx[o + 3] as f32 / 255.0;
                    let oa = s + da * (1.0 - s);
                    for c in 0..3 {
                        fx[o + c] = ((st.color[c] as f32 * s + fx[o + c] as f32 * da * (1.0 - s))
                            / oa.max(1e-6))
                        .round() as u8;
                    }
                    fx[o + 3] = (oa * 255.0).round() as u8;
                }
            }
        }
    }

    /// Composite one pixel layer (with mask + effects) over `out`.
    fn composite_pixel_layer(&self, out: &mut [u8], layer: &Layer) {
        let n = self.width as usize * self.height as usize;
        if layer.effects.has_any() {
            let mut fx = vec![0u8; n * 4];
            self.render_effects(layer, &mut fx);
            src_over(out, &fx);
        }
        let op = layer.opacity.clamp(0.0, 1.0);
        let mode = layer.blend;
        let mask = layer.mask.as_ref().filter(|m| m.len() == n);
        let plain = mode == BlendMode::Normal && (op - 1.0).abs() < 1e-6 && mask.is_none();
        if plain {
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
            for ((d, s), mi) in out
                .chunks_exact_mut(4)
                .zip(layer.pixels.chunks_exact(4))
                .zip(0..n)
            {
                let mut ss = [s[0], s[1], s[2], s[3]];
                if let Some(m) = mask {
                    ss[3] = ((ss[3] as u16 * m[mi] as u16) / 255) as u8;
                }
                let dd = [d[0], d[1], d[2], d[3]];
                d.copy_from_slice(&composite_pixel(mode, op, ss, dd));
            }
        }
    }

    /// Apply an adjustment layer to the current composite in `out`.
    fn composite_adjustment_layer(&self, out: &mut [u8], layer: &Layer, adj: &Adjustment) {
        let n = self.width as usize * self.height as usize;
        let mut tmp = out.to_vec();
        crate::ops::apply_adjustment(adj, &mut tmp);
        let op = layer.opacity.clamp(0.0, 1.0);
        match layer.mask.as_ref().filter(|m| m.len() == n) {
            None => {
                if (op - 1.0).abs() < 1e-6 {
                    out.copy_from_slice(&tmp);
                } else {
                    for (d, s) in out.chunks_exact_mut(4).zip(tmp.chunks_exact(4)) {
                        for i in 0..3 {
                            d[i] = (s[i] as f32 * op + d[i] as f32 * (1.0 - op)).round() as u8;
                        }
                    }
                }
            }
            Some(m) => {
                for ((d, s), mm) in out
                    .chunks_exact_mut(4)
                    .zip(tmp.chunks_exact(4))
                    .zip(m.iter())
                {
                    let k = *mm as f32 / 255.0 * op;
                    for i in 0..3 {
                        d[i] = (s[i] as f32 * k + d[i] as f32 * (1.0 - k)).round() as u8;
                    }
                }
            }
        }
    }

    /// Flatten all visible layers bottom-up (masks, adjustments, effects baked in).
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
            match &layer.kind {
                LayerKind::Pixel => {
                    if layer.pixels.len() != n {
                        continue;
                    }
                    self.composite_pixel_layer(&mut out, layer);
                }
                LayerKind::Adjustment(adj) => {
                    self.composite_adjustment_layer(&mut out, layer, adj);
                }
            }
        }
        out
    }

    /// Merge the active layer down. Adjustment layers bake into the pixel
    /// layer below. Returns false when the merge is not defined (e.g. the
    /// layer below is itself an adjustment layer).
    pub fn can_merge_down(&self) -> bool {
        if self.active == 0 || self.layers.len() < 2 {
            return false;
        }
        !self.layers[self.active - 1].is_adjustment()
    }

    pub fn merge_down(&mut self) -> bool {
        if self.active == 0 || self.layers.len() < 2 {
            return false;
        }
        let below_is_adj = self.layers[self.active - 1].is_adjustment();
        let top_is_adj = self.layers[self.active].is_adjustment();
        if below_is_adj {
            return false;
        }
        let top = self.layers.remove(self.active);
        if !top.visible {
            self.active -= 1;
            return true;
        }
        let below = &mut self.layers[self.active - 1];
        if top_is_adj {
            if let LayerKind::Adjustment(adj) = &top.kind {
                let n = self.width as usize * self.height as usize;
                let mut tmp = below.pixels.clone();
                crate::ops::apply_adjustment(adj, &mut tmp);
                let op = top.opacity.clamp(0.0, 1.0);
                match top.mask.as_ref().filter(|m| m.len() == n) {
                    None => {
                        if (op - 1.0).abs() < 1e-6 {
                            below.pixels = tmp;
                        } else {
                            for (d, s) in below.pixels.chunks_exact_mut(4).zip(tmp.chunks_exact(4)) {
                                for i in 0..3 {
                                    d[i] = (s[i] as f32 * op + d[i] as f32 * (1.0 - op)).round() as u8;
                                }
                            }
                        }
                    }
                    Some(m) => {
                        for ((d, s), mm) in below
                            .pixels
                            .chunks_exact_mut(4)
                            .zip(tmp.chunks_exact(4))
                            .zip(m.iter())
                        {
                            let k = *mm as f32 / 255.0 * op;
                            for i in 0..3 {
                                d[i] = (s[i] as f32 * k + d[i] as f32 * (1.0 - k)).round() as u8;
                            }
                        }
                    }
                }
            }
            self.active -= 1;
            return true;
        }
        let op = top.opacity.clamp(0.0, 1.0);
        let n = self.width as usize * self.height as usize;
        let mask = top.mask.clone().filter(|m| m.len() == n);
        for ((d, s), mi) in below
            .pixels
            .chunks_exact_mut(4)
            .zip(top.pixels.chunks_exact(4))
            .zip(0..n)
        {
            let mut ss = [s[0], s[1], s[2], s[3]];
            if let Some(m) = &mask {
                ss[3] = ((ss[3] as u16 * m[mi] as u16) / 255) as u8;
            }
            let dd = [d[0], d[1], d[2], d[3]];
            d.copy_from_slice(&composite_pixel(top.blend, op, ss, dd));
        }
        self.active -= 1;
        true
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
            kind: LayerKind::Pixel,
            mask: None,
            effects: LayerEffects::default(),
        }];
        self.active = 0;
    }
}

// ---------------------------------------------------------------------------
// History: bounded, labeled undo/redo with jump-to support
// ---------------------------------------------------------------------------

/// Bounded undo/redo stack of full document snapshots with action labels.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<(String, Document)>,
    redo: Vec<(String, Document)>,
    limit: usize,
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit: limit.max(5).min(100) }
    }
    pub fn push(&mut self, label: &str, doc: &Document) {
        if self.limit == 0 {
            return;
        }
        if self.undo.len() >= self.limit {
            self.undo.remove(0);
        }
        self.undo.push((label.to_owned(), doc.clone()));
        self.redo.clear();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    /// Position of the live document on the timeline (== number of undo steps).
    pub fn position(&self) -> usize {
        self.undo.len()
    }
    /// Labels of undoable states, oldest first.
    pub fn labels(&self) -> Vec<String> {
        self.undo.iter().map(|(l, _)| l.clone()).collect()
    }
    pub fn undo(&mut self, current: &Document) -> Option<Document> {
        let (label, prev) = self.undo.pop()?;
        if self.redo.len() >= self.limit {
            self.redo.remove(0);
        }
        self.redo.push((label, current.clone()));
        Some(prev)
    }
    pub fn redo(&mut self, current: &Document) -> Option<Document> {
        let (label, next) = self.redo.pop()?;
        if self.undo.len() >= self.limit {
            self.undo.remove(0);
        }
        self.undo.push((label, current.clone()));
        Some(next)
    }
    /// Jump the timeline so that `undo.len() == index`. Index `undo.len()`
    /// means the live document (no-op returning a clone of it).
    pub fn goto(&mut self, current: &Document, index: usize) -> Option<Document> {
        let mut doc = current.clone();
        while self.undo.len() > index {
            doc = self.undo(doc)?;
        }
        while self.undo.len() < index && self.can_redo() {
            doc = self.redo(&doc)?;
        }
        Some(doc)
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

    #[test]
    fn mask_hides_layer() {
        let mut doc = Document::new(2, 2, [0, 0, 255, 255]);
        doc.add_solid_layer("red", [255, 0, 0, 255]);
        doc.layers[1].mask = Some(vec![0u8; 4]);
        let out = doc.composite();
        assert_eq!(&out[0..3], &[0, 0, 255]);
    }

    #[test]
    fn adjustment_layer_inverts() {
        let mut doc = Document::new(1, 1, [10, 20, 30, 255]);
        doc.add_adjustment_layer("inv", Adjustment::Invert);
        let out = doc.composite();
        assert_eq!(&out[0..3], &[245, 235, 225]);
    }

    #[test]
    fn history_labels_and_goto() {
        let mut h = History::new(10);
        let a = Document::new(1, 1, [0, 0, 0, 255]);
        let mut b = a.clone();
        b.add_solid_layer("x", [1, 2, 3, 255]);
        h.push("open", &a);
        h.push("add", &b);
        assert_eq!(h.labels(), vec!["open".to_string(), "add".to_string()]);
        let back = h.goto(&b, 0).unwrap();
        assert_eq!(back.layers.len(), 1);
        let fwd = h.goto(&back, 2).unwrap();
        assert_eq!(fwd.layers.len(), 2);
    }
}
