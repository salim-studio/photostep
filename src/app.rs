//! Copyright (c) 2026 salim-slimani. Licensed under MIT. See LICENSE-MIT.
//! PhotoStep desktop app: layer-based editor layout in egui/eframe.
//! Left: toolbox. Center: canvas. Right: layers + adjustments. Top: menu. Bottom: status.

use egui::{Color32, TextureHandle, Vec2};
use crate::core::{Adjustment, BlendMode, Document, History, LayerKind};
use crate::{io, ops};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Move,
    Brush,
    Eraser,
    CloneStamp,
    Fill,
    Gradient,
    Eyedropper,
    SelectRect,
    SelectEllipse,
    MagicWand,
    ShapeRect,
    ShapeEllipse,
    ShapeLine,
    Crop,
    Zoom,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Move => "Move (V)",
            Tool::Brush => "Brush (B)",
            Tool::Eraser => "Eraser (E)",
            Tool::CloneStamp => "Clone (S)",
            Tool::Fill => "Fill (G)",
            Tool::Gradient => "Gradient",
            Tool::Eyedropper => "Picker (I)",
            Tool::SelectRect => "Rect Select (M)",
            Tool::SelectEllipse => "Ellipse Select",
            Tool::MagicWand => "Wand (W)",
            Tool::ShapeRect => "Shape Rect",
            Tool::ShapeEllipse => "Shape Ellipse",
            Tool::ShapeLine => "Shape Line",
            Tool::Crop => "Crop (C)",
            Tool::Zoom => "Zoom (Z)",
        }
    }
    fn tip(self) -> &'static str {
        match self {
            Tool::Move => "Drag to move the active layer (V)",
            Tool::Brush => "Soft paintbrush (B)",
            Tool::Eraser => "Soft eraser (E)",
            Tool::CloneStamp => "Copy from an Alt-clicked source (S)",
            Tool::Fill => "Fill the layer or selection (G)",
            Tool::Gradient => "Drag for a foreground → background blend",
            Tool::Eyedropper => "Pick a color from the canvas (I)",
            Tool::SelectRect => "Rectangular marquee (M)",
            Tool::SelectEllipse => "Elliptical marquee",
            Tool::MagicWand => "Select similar colors (W) — Shift adds",
            Tool::ShapeRect => "Filled rectangle on a new layer",
            Tool::ShapeEllipse => "Filled ellipse on a new layer",
            Tool::ShapeLine => "Thick line on a new layer",
            Tool::Crop => "Drag, release to crop (C)",
            Tool::Zoom => "Click to zoom, Shift-click to zoom out (Z)",
        }
    }
}

// ---------- Hand-drawn toolbox icons (brand palette, no emoji) ----------

fn lerp_brand(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t).round() as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t).round() as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t).round() as u8,
    )
}

fn dashed_line(p: &egui::Painter, a: egui::Pos2, b: egui::Pos2, stroke: egui::Stroke) {
    let len = a.distance(b);
    if len <= 0.0 {
        return;
    }
    let (dash, gap) = (4.0, 3.0);
    let mut t = 0.0;
    while t < len {
        let t0 = t / len;
        let t1 = ((t + dash) / len).min(1.0);
        p.line_segment([a.lerp(b, t0), a.lerp(b, t1)], stroke);
        t += dash + gap;
    }
}

fn dashed_rect(p: &egui::Painter, r: egui::Rect, stroke: egui::Stroke) {
    dashed_line(p, r.left_top(), r.right_top(), stroke);
    dashed_line(p, r.right_top(), r.right_bottom(), stroke);
    dashed_line(p, r.right_bottom(), r.left_bottom(), stroke);
    dashed_line(p, r.left_bottom(), r.left_top(), stroke);
}

fn dashed_circle(p: &egui::Painter, c: egui::Pos2, radius: f32, stroke: egui::Stroke) {
    for k in 0..10 {
        let a0 = k as f32 * 36.0_f32.to_radians();
        let a1 = a0 + 23.0_f32.to_radians();
        let mut prev = None;
        for i in 0..=4 {
            let a = a0 + (a1 - a0) * i as f32 / 4.0;
            let pt = egui::pos2(c.x + radius * a.cos(), c.y + radius * a.sin());
            if let Some(q) = prev {
                p.line_segment([q, pt], stroke);
            }
            prev = Some(pt);
        }
    }
}

/// Paint a brand-style vector glyph for a tool inside `r`.
fn paint_tool_icon(p: &egui::Painter, r: egui::Rect, tool: Tool, fg: Color32) {
    let stroke = |w: f32| egui::Stroke::new(w, fg);
    let c = r.center();
    let s = r.width().min(r.height()) / 2.0;
    let pt = |dx: f32, dy: f32| egui::pos2(c.x + dx * s, c.y + dy * s);
    match tool {
        Tool::Move => {
            p.line_segment([pt(-0.7, 0.0), pt(0.7, 0.0)], stroke(2.0));
            p.line_segment([pt(0.0, -0.7), pt(0.0, 0.7)], stroke(2.0));
            for (dx, dy) in [(-0.7, 0.0), (0.7, 0.0), (0.0, -0.7), (0.0, 0.7)] {
                p.circle_filled(pt(dx, dy), 2.4, fg);
            }
        }
        Tool::Brush => {
            p.line_segment([pt(-0.55, 0.55), pt(0.2, -0.2)], egui::Stroke::new(3.2, fg));
            // solid tip triangle (must have real area: degenerate polygons
            // make tessellators emit stray streaks on some GPUs)
            p.add(egui::Shape::convex_polygon(
                vec![pt(0.65, -0.65), pt(0.264, -0.136), pt(0.136, -0.264)],
                fg,
                egui::Stroke::NONE,
            ));
        }
        Tool::Eraser => {
            p.add(egui::Shape::convex_polygon(
                vec![pt(-0.6, 0.2), pt(-0.05, -0.45), pt(0.6, 0.2), pt(0.05, 0.7)],
                fg,
                egui::Stroke::NONE,
            ));
            p.line_segment([pt(-0.42, -0.06), pt(0.23, 0.44)], egui::Stroke::new(1.6, BRAND_INK_SOFT));
            for (dx, dy) in [(-0.62, 0.62), (-0.78, 0.34)] {
                p.circle_filled(pt(dx, dy), 1.6, fg);
            }
        }
        Tool::CloneStamp => {
            p.rect_stroke(
                egui::Rect::from_two_pos(pt(-0.35, -0.1), pt(0.35, 0.3)),
                2.0,
                stroke(2.0),
                egui::StrokeKind::Middle,
            );
            p.line_segment([pt(0.0, -0.1), pt(0.0, -0.55)], stroke(2.0));
            p.line_segment([pt(-0.35, -0.55), pt(0.35, -0.55)], stroke(2.0));
            p.line_segment([pt(-0.5, 0.62), pt(0.5, 0.62)], stroke(2.0));
        }
        Tool::Fill => {
            p.line_segment([pt(-0.6, -0.15), pt(0.1, -0.15)], stroke(2.2));
            p.line_segment([pt(-0.6, -0.15), pt(-0.35, 0.6)], stroke(2.2));
            p.line_segment([pt(0.1, -0.15), pt(-0.15, 0.6)], stroke(2.2));
            p.circle_filled(pt(0.45, 0.42), 3.0, BRAND_AQUA);
        }
        Tool::Gradient => {
            let n = 6;
            for i in 0..n {
                let t0 = i as f32 / n as f32;
                let t1 = (i + 1) as f32 / n as f32;
                let col = lerp_brand(BRAND_ORANGE, BRAND_AMBER, t0);
                let x0 = c.x - 0.62 * s + 1.24 * s * t0;
                let x1 = c.x - 0.62 * s + 1.24 * s * t1;
                p.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, c.y - 0.34 * s),
                        egui::pos2(x1, c.y + 0.34 * s),
                    ),
                    0.0,
                    col,
                );
            }
        }
        Tool::Eyedropper => {
            p.circle_filled(pt(0.05, 0.2), s * 0.4, fg);
            p.add(egui::Shape::convex_polygon(
                vec![pt(-0.22, -0.05), pt(0.32, -0.05), pt(0.05, -0.68)],
                fg,
                egui::Stroke::NONE,
            ));
        }
        Tool::SelectRect => {
            dashed_rect(
                p,
                egui::Rect::from_two_pos(pt(-0.6, -0.45), pt(0.6, 0.45)),
                stroke(1.8),
            );
        }
        Tool::SelectEllipse => {
            dashed_circle(p, c, s * 0.62, stroke(1.8));
        }
        Tool::MagicWand => {
            p.line_segment([pt(-0.55, 0.55), pt(0.05, -0.05)], stroke(2.6));
            for (dx, dy, sz) in [(0.38, -0.38, 0.2), (0.62, 0.02, 0.13), (-0.02, -0.52, 0.13)] {
                let q = pt(dx, dy);
                let r = sz * s;
                p.line_segment(
                    [egui::pos2(q.x - r, q.y), egui::pos2(q.x + r, q.y)],
                    egui::Stroke::new(1.8, BRAND_AQUA),
                );
                p.line_segment(
                    [egui::pos2(q.x, q.y - r), egui::pos2(q.x, q.y + r)],
                    egui::Stroke::new(1.8, BRAND_AQUA),
                );
            }
        }
        Tool::ShapeRect => {
            p.rect_filled(
                egui::Rect::from_two_pos(pt(-0.58, -0.42), pt(0.58, 0.42)),
                3.0,
                fg,
            );
        }
        Tool::ShapeEllipse => {
            p.circle_filled(c, s * 0.62, fg);
        }
        Tool::ShapeLine => {
            p.line_segment([pt(-0.6, 0.6), pt(0.6, -0.6)], egui::Stroke::new(4.2, fg));
        }
        Tool::Crop => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let ex = c.x + sx * 0.62 * s;
                let ey = c.y + sy * 0.62 * s;
                let ix = c.x + sx * 0.3 * s;
                let iy = c.y + sy * 0.3 * s;
                p.line_segment([egui::pos2(ex, ey), egui::pos2(ix, ey)], stroke(2.2));
                p.line_segment([egui::pos2(ex, ey), egui::pos2(ex, iy)], stroke(2.2));
            }
        }
        Tool::Zoom => {
            let zc = egui::pos2(c.x - 0.12 * s, c.y - 0.12 * s);
            p.circle_stroke(zc, s * 0.48, stroke(2.2));
            p.line_segment([pt(0.22, 0.22), pt(0.62, 0.62)], stroke(2.6));
        }
    }
}

/// Brush paint target: pixels or the layer mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaintTarget {
    #[default]
    Image,
    Mask,
}

// ---------- PhotoStep brand identity ----------
pub const BRAND_NAME: &str = "PhotoStep";
pub const BRAND_VERSION: &str = "0.2.1";
pub const BRAND_COPYRIGHT: &str = "© 2026 salim-slimani. All rights reserved.";
pub const BRAND_ORANGE: Color32 = Color32::from_rgb(255, 90, 40);
pub const BRAND_AMBER: Color32 = Color32::from_rgb(255, 176, 58);
pub const BRAND_AQUA: Color32 = Color32::from_rgb(53, 208, 197);
pub const BRAND_INK_SOFT: Color32 = Color32::from_rgb(20, 20, 43);

/// Apply the PhotoStep visual identity: deep-ink surfaces, step-orange accents.
/// Always starts from the dark theme so the studio looks identical on every OS.
fn apply_brand_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::from_rgb(24, 24, 40);
    style.visuals.window_fill = Color32::from_rgb(28, 28, 46);
    style.visuals.faint_bg_color = Color32::from_rgb(37, 37, 60);
    style.visuals.extreme_bg_color = Color32::from_rgb(13, 13, 25);
    style.visuals.selection.bg_fill = BRAND_ORANGE;
    style.visuals.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(255, 110, 62);
    style.visuals.widgets.active.bg_fill = BRAND_ORANGE;
    style.visuals.widgets.open.bg_fill = Color32::from_rgb(60, 60, 86);
    for w in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        w.corner_radius = egui::CornerRadius::same(6);
    }
    style.visuals.window_corner_radius = egui::CornerRadius::same(10);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(8);
    style.spacing.item_spacing = Vec2::new(6.0, 5.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    style.spacing.indent = 18.0;
    ctx.set_style(style);
}

pub struct PhotoStepApp {
    doc: Document,
    history: History,
    tex: Option<TextureHandle>,
    tex_size: (u32, u32),
    zoom: f32,
    tool: Tool,
    brush_size: f32,
    color: Color32,
    bg: Color32, // background color (gradients end here)
    paint_target: PaintTarget,
    painting: bool,
    sel: Option<egui::Rect>, // in image pixels
    sel_ellipse: bool,
    sel_mask: Option<Vec<u8>>, // full-size selection mask
    sel_feather: u32,
    sel_start: Option<(f32, f32)>,
    wand_tol: u8,
    clone_src: Option<(f32, f32)>,
    stroke_snap: Option<Vec<u8>>, // layer snapshot at stroke start (clone/move)
    move_start: Option<(f32, f32)>,
    drag_cur: Option<(f32, f32)>,
    last_dab: Option<(f32, f32)>, // stroke interpolation anchor
    grad_start: Option<(f32, f32)>,
    shape_start: Option<(f32, f32)>,
    curve_grab: Option<usize>,
    scale_x: f32,
    scale_y: f32,
    rot_deg: f32,
    // web-only: in-flight async file picker tasks, polled each frame
    #[cfg(target_arch = "wasm32")]
    open_pending: Option<poll_promise::Promise<Option<(String, Vec<u8>)>>>,
    #[cfg(target_arch = "wasm32")]
    save_pending: Option<poll_promise::Promise<Option<String>>>,
    msg: String,
    show_about: bool,
    // sliders
    bri: i16,
    con: f32,
    sat: f32,
    exp: f32,
    blur: u32,
}

impl PhotoStepApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_brand_theme(&cc.egui_ctx);
        Self {
            doc: Document::new(1280, 800, [45, 45, 48, 255]),
            history: History::new(30),
            tex: None,
            tex_size: (0, 0),
            zoom: 0.6,
            tool: Tool::Brush,
            brush_size: 24.0,
            color: BRAND_ORANGE,
            bg: Color32::from_rgb(240, 240, 240),
            paint_target: PaintTarget::Image,
            painting: false,
            sel: None,
            sel_ellipse: false,
            sel_mask: None,
            sel_feather: 0,
            sel_start: None,
            wand_tol: 32,
            clone_src: None,
            stroke_snap: None,
            move_start: None,
            drag_cur: None,
            last_dab: None,
            grad_start: None,
            shape_start: None,
            curve_grab: None,
            scale_x: 100.0,
            scale_y: 100.0,
            rot_deg: 0.0,
            #[cfg(target_arch = "wasm32")]
            open_pending: None,
            #[cfg(target_arch = "wasm32")]
            save_pending: None,
            msg: "Ready — File › Open an image, or paint on the canvas.".into(),
            show_about: false,
            bri: 0, con: 0.0, sat: 1.0, exp: 0.0, blur: 4,
        }
    }

    fn checkpoint(&mut self, label: &str) {
        self.history.push(label, &self.doc);
    }

    /// Point `active` at the nearest pixel layer (paint/ops target).
    /// Adjustment layers are non-destructive, so paint redirects below them.
    /// Returns false + status message when there is no pixel layer.
    fn ensure_pixel_target(&mut self) -> bool {
        match self.doc.paint_target() {
            Some(i) => {
                if i != self.doc.active && self.doc.active_layer().is_adjustment() {
                    self.doc.active = i;
                    self.msg = "Adjustment layers stay editable — painted on the pixel layer below.".into();
                }
                true
            }
            None => {
                self.msg = "No pixel layer to edit — add a layer first.".into();
                false
            }
        }
    }

    /// True when the active layer holds paintable pixels.
    fn active_is_pixel(&self) -> bool {
        matches!(self.doc.active_layer().kind, LayerKind::Pixel)
    }

    /// Run a pixel op on the paint target, masked by the selection when present.
    fn apply_layer_op(&mut self, label: &str, f: impl FnOnce(&mut Document)) {
        if !self.ensure_pixel_target() {
            return;
        }
        self.checkpoint(label);
        let n = self.doc.width as usize * self.doc.height as usize;
        match self.sel_mask.clone() {
            Some(m) if m.len() == n => {
                let mut tmp = Document::new(self.doc.width, self.doc.height, [0, 0, 0, 0]);
                tmp.layers[0].pixels = self.doc.active_layer().pixels.clone();
                f(&mut tmp);
                let res = tmp.layers.into_iter().next().unwrap().pixels;
                let dst = &mut self.doc.active_layer_mut().pixels;
                for ((d, s), mm) in dst.chunks_exact_mut(4).zip(res.chunks_exact(4)).zip(m.iter()) {
                    let k = *mm as f32 / 255.0;
                    for i in 0..4 {
                        d[i] = (s[i] as f32 * k + d[i] as f32 * (1.0 - k)).round() as u8;
                    }
                }
            }
            _ => {
                f(&mut self.doc);
            }
        }
        self.tex = None;
    }

    // ----- selections -----

    fn clear_selection(&mut self) {
        self.sel = None;
        self.sel_mask = None;
        self.sel_ellipse = false;
    }

    /// Combined selection mask from the wand mask or the rect/ellipse marquee.
    fn selection_mask(&self) -> Option<Vec<u8>> {
        let (w, h) = (self.doc.width as usize, self.doc.height as usize);
        if let Some(m) = &self.sel_mask {
            if m.len() == w * h {
                return Some(m.clone());
            }
        }
        self.sel.map(|r| {
            let mut m = vec![0u8; w * h];
            let x0 = r.min.x.floor().max(0.0) as usize;
            let y0 = r.min.y.floor().max(0.0) as usize;
            let x1 = (r.max.x.ceil() as usize).min(w);
            let y1 = (r.max.y.ceil() as usize).min(h);
            if self.sel_ellipse {
                let (cx, cy) = ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
                let (rx, ry) = ((x1 - x0) as f32 / 2.0, (y1 - y0) as f32 / 2.0);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let dx = (x as f32 + 0.5 - cx) / rx.max(0.5);
                        let dy = (y as f32 + 0.5 - cy) / ry.max(0.5);
                        if dx * dx + dy * dy <= 1.0 {
                            m[y * w + x] = 255;
                        }
                    }
                }
            } else {
                for y in y0..y1 {
                    for x in x0..x1 {
                        m[y * w + x] = 255;
                    }
                }
            }
            m
        })
    }

    fn feather_selection(&mut self) {
        let (w, h) = (self.doc.width as usize, self.doc.height as usize);
        match self.selection_mask() {
            Some(m) => {
                let r = self.sel_feather as usize;
                self.sel_mask = Some(if r == 0 { m } else { ops::blur_channel(&m, w, h, r) });
                self.sel = None;
                self.msg = format!("Selection feathered by {} px.", self.sel_feather);
            }
            None => self.msg = "No selection to feather.".into(),
        }
    }

    fn invert_selection(&mut self) {
        match self.selection_mask() {
            Some(mut m) => {
                ops::invert_mask(&mut m);
                self.sel_mask = Some(m);
                self.sel = None;
                self.msg = "Selection inverted.".into();
            }
            None => self.msg = "No selection to invert.".into(),
        }
    }

    fn crop_to_selection(&mut self) {
        let (w, h) = (self.doc.width as usize, self.doc.height as usize);
        let bbox = self
            .selection_mask()
            .as_ref()
            .and_then(|m| ops::mask_bounding_box(m, w, h));
        match bbox {
            Some((x0, y0, x1, y1)) => {
                self.checkpoint("Crop");
                ops::crop_document(&mut self.doc, x0, y0, x1, y1);
                self.clear_selection();
                self.tex = None;
                self.msg = format!("Cropped to {}×{}.", self.doc.width, self.doc.height);
            }
            None => self.msg = "Nothing selected to crop.".into(),
        }
    }

    // ----- paint helpers -----

    fn stamp_disk(buf: &mut [u8], w: u32, h: u32, cx: f32, cy: f32, r: f32, color: [u8; 4]) {
        let x0 = (cx - r).floor().max(0.0) as u32;
        let y0 = (cy - r).floor().max(0.0) as u32;
        let x1 = (cx + r).ceil().min(w as f32 - 1.0).max(0.0) as u32;
        let y1 = (cy + r).ceil().min(h as f32 - 1.0).max(0.0) as u32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                if dx * dx + dy * dy <= r * r {
                    let o = ((y * w + x) * 4) as usize;
                    if o + 4 <= buf.len() {
                        buf[o..o + 4].copy_from_slice(&color);
                    }
                }
            }
        }
    }

    fn render_gradient(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        if !self.ensure_pixel_target() {
            return;
        }
        self.checkpoint("Gradient");
        let fg = [self.color.r(), self.color.g(), self.color.b()];
        let bgc = [self.bg.r(), self.bg.g(), self.bg.b()];
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len2 = (dx * dx + dy * dy).max(1.0);
        let mask = self.sel_mask.clone();
        let layer = self.doc.active_layer_mut();
        let (w, h) = (layer.width, layer.height);
        for y in 0..h {
            for x in 0..w {
                let t = (((x as f32 - x0) * dx + (y as f32 - y0) * dy) / len2).clamp(0.0, 1.0);
                let mut c = [0u8; 4];
                let dh = ops::hash_noise(x, y) * 2.0; // dither kills banding
                for i in 0..3 {
                    c[i] = (fg[i] as f32 * (1.0 - t) + bgc[i] as f32 * t + dh).round().clamp(0.0, 255.0) as u8;
                }
                c[3] = 255;
                let o = ((y * w + x) * 4) as usize;
                match &mask {
                    Some(m) if (o / 4) < m.len() => {
                        let k = m[o / 4] as f32 / 255.0;
                        for i in 0..4 {
                            layer.pixels[o + i] =
                                (c[i] as f32 * k + layer.pixels[o + i] as f32 * (1.0 - k)).round() as u8;
                        }
                    }
                    _ => {
                        layer.pixels[o..o + 4].copy_from_slice(&c);
                    }
                }
            }
        }
        self.tex = None;
    }

    fn raster_shape(&mut self, kind: Tool, x0: f32, y0: f32, x1: f32, y1: f32) {
        let (w, h) = (self.doc.width, self.doc.height);
        let c = [self.color.r(), self.color.g(), self.color.b(), 255];
        let name = match kind {
            Tool::ShapeRect => "Rectangle",
            Tool::ShapeEllipse => "Ellipse",
            _ => "Line",
        };
        self.checkpoint(name);
        self.doc.add_solid_layer(name, [0, 0, 0, 0]);
        let layer = self.doc.active_layer_mut();
        match kind {
            Tool::ShapeRect => {
                let xa = x0.min(x1).floor().max(0.0) as u32;
                let xb = x0.max(x1).ceil().min(w as f32).max(0.0) as u32;
                let ya = y0.min(y1).floor().max(0.0) as u32;
                let yb = y0.max(y1).ceil().min(h as f32).max(0.0) as u32;
                for y in ya..yb {
                    for x in xa..xb {
                        let o = ((y * w + x) * 4) as usize;
                        if o + 4 <= layer.pixels.len() {
                            layer.pixels[o..o + 4].copy_from_slice(&c);
                        }
                    }
                }
            }
            Tool::ShapeEllipse => {
                let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let (rx, ry) = ((x1 - x0).abs() / 2.0, (y1 - y0).abs() / 2.0);
                for y in 0..h {
                    for x in 0..w {
                        let dx = (x as f32 + 0.5 - cx) / rx.max(0.5);
                        let dy = (y as f32 + 0.5 - cy) / ry.max(0.5);
                        if dx * dx + dy * dy <= 1.0 {
                            let o = ((y * w + x) * 4) as usize;
                            layer.pixels[o..o + 4].copy_from_slice(&c);
                        }
                    }
                }
            }
            _ => {
                let dx = x1 - x0;
                let dy = y1 - y0;
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                let r = (self.brush_size / 2.0).max(1.5);
                let steps = len.ceil() as usize;
                let buf = &mut layer.pixels;
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    Self::stamp_disk(buf, w, h, x0 + dx * t, y0 + dy * t, r, c);
                }
            }
        }
        self.tex = None;
    }

    fn clone_dab(&mut self, cx: f32, cy: f32) {
        let snap = match self.stroke_snap.clone() {
            Some(b) => b,
            None => return,
        };
        let (sx, sy) = match self.clone_src {
            Some(p) => p,
            None => {
                self.msg = "Alt-click to set the clone source first.".into();
                return;
            }
        };
        let (ox, oy) = match self.move_start {
            Some(p) => p,
            None => return,
        };
        let r = (self.brush_size / 2.0).max(1.0);
        let layer = self.doc.active_layer_mut();
        let (w, h) = (layer.width as f32, layer.height as f32);
        let x0 = (cx - r).floor().max(0.0) as u32;
        let y0 = (cy - r).floor().max(0.0) as u32;
        let x1 = ((cx + r).ceil().min(w - 1.0).max(0.0)) as u32;
        let y1 = ((cy + r).ceil().min(h - 1.0).max(0.0)) as u32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                if d <= r {
                    let a = (1.0 - d / r * 0.6).clamp(0.0, 1.0);
                    let px = (sx + (x as f32 - ox)).round() as i32;
                    let py = (sy + (y as f32 - oy)).round() as i32;
                    if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                        continue;
                    }
                    let s = ((py as u32 * layer.width + px as u32) * 4) as usize;
                    let o = ((y * layer.width + x) * 4) as usize;
                    for i in 0..3 {
                        layer.pixels[o + i] =
                            (snap[s + i] as f32 * a + layer.pixels[o + i] as f32 * (1.0 - a)).round() as u8;
                    }
                    layer.pixels[o + 3] = 255;
                }
            }
        }
        self.tex = None;
    }

    fn move_dab(&mut self, dx: f32, dy: f32) {
        let snap = match self.stroke_snap.clone() {
            Some(b) => b,
            None => return,
        };
        let layer = self.doc.active_layer_mut();
        let (w, h) = (layer.width as i32, layer.height as i32);
        let (ix, iy) = (dx.round() as i32, dy.round() as i32);
        let mut out = vec![0u8; snap.len()];
        for y in 0..h {
            for x in 0..w {
                let sx = x - ix;
                let sy = y - iy;
                if sx >= 0 && sy >= 0 && sx < w && sy < h {
                    let s = ((sy * w + sx) * 4) as usize;
                    let d = ((y * w + x) * 4) as usize;
                    out[d..d + 4].copy_from_slice(&snap[s..s + 4]);
                }
            }
        }
        layer.pixels = out;
        self.tex = None;
    }

    /// Curves editor: drag points, click empty space to add, right-click to remove.
    /// Returns true when the points changed.
    fn curve_editor(&mut self, ui: &mut egui::Ui, points: &mut Vec<(u8, u8)>) -> bool {
        let mut changed = false;
        points.sort_by_key(|p| p.0);
        let lut = ops::curves_lut(points);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(232.0, 140.0), egui::Sense::click_and_drag());
        let to_screen = |x: u8, y: u8| {
            egui::pos2(
                rect.min.x + x as f32 / 255.0 * rect.width(),
                rect.max.y - y as f32 / 255.0 * rect.height(),
            )
        };
        let p = ui.painter();
        p.rect_filled(rect, 4.0, Color32::from_gray(24));
        for i in 1..4 {
            let t = i as f32 / 4.0;
            p.line_segment(
                [egui::pos2(rect.min.x + rect.width() * t, rect.min.y), egui::pos2(rect.min.x + rect.width() * t, rect.max.y)],
                (1.0, Color32::from_gray(60)),
            );
            p.line_segment(
                [egui::pos2(rect.min.x, rect.min.y + rect.height() * t), egui::pos2(rect.max.x, rect.min.y + rect.height() * t)],
                (1.0, Color32::from_gray(60)),
            );
        }
        let mut prev = None;
        for (i, v) in lut.iter().enumerate().step_by(4) {
            let pt = to_screen(i as u8, *v);
            if let Some(q) = prev {
                p.line_segment([q, pt], (1.5, BRAND_ORANGE));
            }
            prev = Some(pt);
        }
        for (i, (x, y)) in points.iter().enumerate() {
            let pt = to_screen(*x, *y);
            let grab = self.curve_grab == Some(i);
            p.circle_filled(pt, if grab { 6.0 } else { 4.5 }, if grab { BRAND_AMBER } else { Color32::WHITE });
        }
        if resp.drag_started() {
            if let Some(mp) = resp.interact_pointer_pos() {
                let mut best = None;
                for (i, (x, y)) in points.iter().enumerate() {
                    if to_screen(*x, *y).distance(mp) < 12.0 {
                        best = Some(i);
                        break;
                    }
                }
                self.curve_grab = best;
                if best.is_none() {
                    let nx = ((mp.x - rect.min.x) / rect.width() * 255.0).round().clamp(0.0, 255.0) as u8;
                    let ny = ((rect.max.y - mp.y) / rect.height() * 255.0).round().clamp(0.0, 255.0) as u8;
                    points.push((nx, ny));
                    points.sort_by_key(|q| q.0);
                    changed = true;
                }
            }
        } else if resp.dragged() {
            if let (Some(g), Some(mp)) = (self.curve_grab, resp.interact_pointer_pos()) {
                if g < points.len() {
                    let first = g == 0;
                    let last = g + 1 == points.len();
                    let nx = ((mp.x - rect.min.x) / rect.width() * 255.0).round().clamp(0.0, 255.0) as u8;
                    let ny = ((rect.max.y - mp.y) / rect.height() * 255.0).round().clamp(0.0, 255.0) as u8;
                    points[g].1 = ny;
                    if !first && !last {
                        points[g].0 = nx;
                    }
                    points.sort_by_key(|q| q.0);
                    changed = true;
                }
            }
        } else if resp.drag_stopped() {
            self.curve_grab = None;
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            if let Some(mp) = resp.interact_pointer_pos() {
                if let Some(i) = points.iter().position(|(x, y)| to_screen(*x, *y).distance(mp) < 12.0) {
                    if points.len() > 2 {
                        points.remove(i);
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    fn undo(&mut self) {
        if let Some(d) = self.history.undo(&self.doc) {
            self.doc = d;
            self.tex = None;
            self.msg = "Undo".into();
        }
    }

    fn redo(&mut self) {
        if let Some(d) = self.history.redo(&self.doc) {
            self.doc = d;
            self.tex = None;
            self.msg = "Redo".into();
        }
    }

    fn refresh_texture(&mut self, ctx: &egui::Context) {
        let need = self.tex.is_none() || self.tex_size != (self.doc.width, self.doc.height);
        if need {
            let mut flat = self.doc.composite();
            // marching-ants substitute: dim everything outside the selection
            if let Some(m) = &self.sel_mask {
                if m.len() * 4 == flat.len() {
                    for (px, mm) in flat.chunks_exact_mut(4).zip(m.iter()) {
                        if *mm < 128 {
                            px[0] = (px[0] as f32 * 0.7 + 40.0).round() as u8;
                            px[1] = (px[1] as f32 * 0.7).round() as u8;
                            px[2] = (px[2] as f32 * 0.7).round() as u8;
                        }
                    }
                }
            }
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [self.doc.width as usize, self.doc.height as usize],
                &flat,
            );
            self.tex = Some(ctx.load_texture("canvas", img, egui::TextureOptions::LINEAR));
            self.tex_size = (self.doc.width, self.doc.height);
        }
    }

    fn apply_brush(&mut self, img_pos: egui::Pos2, erase: bool) {
        if self.doc.paint_target().is_none() {
            return;
        }
        // Paint on the layer mask (grayscale) instead of pixels.
        if self.paint_target == PaintTarget::Mask {
            self.doc.active_layer_mut().ensure_mask();
            let lum = (0.299 * self.color.r() as f32
                + 0.587 * self.color.g() as f32
                + 0.114 * self.color.b() as f32)
                .round() as u8;
            let target = if erase { 0u8 } else { lum };
            let (w, h) = {
                let l = self.doc.active_layer();
                (l.width, l.height)
            };
            let r = (self.brush_size / 2.0).max(1.0);
            let (cx, cy) = (img_pos.x, img_pos.y);
            let x0 = (cx - r).floor().max(0.0) as u32;
            let y0 = (cy - r).floor().max(0.0) as u32;
            let x1 = ((cx + r).ceil().min(w as f32 - 1.0).max(0.0)) as u32;
            let y1 = ((cy + r).ceil().min(h as f32 - 1.0).max(0.0)) as u32;
            let mask = self.doc.active_layer_mut().mask.as_mut().unwrap();
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let dx = x as f32 - cx;
                    let dy = y as f32 - cy;
                    if (dx * dx + dy * dy).sqrt() <= r {
                        let o = (y * w + x) as usize;
                        if o < mask.len() {
                            let a = 0.85;
                            mask[o] = (target as f32 * a + mask[o] as f32 * (1.0 - a)).round() as u8;
                        }
                    }
                }
            }
            self.tex = None;
            return;
        }
        let layer = self.doc.active_layer_mut();
        if layer.pixels.is_empty() {
            return;
        }
        let r = (self.brush_size / 2.0).max(1.0);
        let cx = img_pos.x;
        let cy = img_pos.y;
        let (w, h) = (layer.width as f32, layer.height as f32);
        let c = if erase { [0, 0, 0, 0] } else { [self.color.r(), self.color.g(), self.color.b(), 255] };
        let x0 = (cx - r).floor().max(0.0) as u32;
        let y0 = (cy - r).floor().max(0.0) as u32;
        let x1 = ((cx + r).ceil().min(w - 1.0)) as u32;
        let y1 = ((cy + r).ceil().min(h - 1.0)) as u32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                if d <= r {
                    // selection gates the stroke
                    let k = match &self.sel_mask {
                        Some(m) => m.get((y * layer.width + x) as usize).copied().unwrap_or(255) as f32 / 255.0,
                        None => 1.0,
                    };
                    if k <= 0.0 {
                        continue;
                    }
                    // soft edge
                    let a = if erase { 1.0 } else { (1.0 - d / r * 0.6).clamp(0.0, 1.0) } * k;
                    let o = ((y * layer.width + x) * 4) as usize;
                    if erase {
                        let old = layer.pixels[o + 3] as f32;
                        layer.pixels[o + 3] = (old * (1.0 - a)).round() as u8;
                    } else if a >= 1.0 {
                        layer.pixels[o..o + 4].copy_from_slice(&c);
                    } else {
                        for i in 0..3 {
                            let v = layer.pixels[o + i] as f32 * (1.0 - a) + c[i] as f32 * a;
                            layer.pixels[o + i] = v.round() as u8;
                        }
                        layer.pixels[o + 3] = 255;
                    }
                }
            }
        }
        self.tex = None; // force re-upload
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("images", &["png", "jpg", "jpeg", "tiff", "bmp", "webp", "gif", "qoi", "psd", "pstep", "json"])
            .pick_file()
        {
            let s = p.to_string_lossy().to_string();
            self.open_path(&s);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn open_dialog(&mut self) {
        if self.open_pending.is_some() {
            return;
        }
        let dialog = rfd::AsyncFileDialog::new().add_filter(
            "images",
            &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff", "tif", "qoi", "psd"],
        );
        let promise = poll_promise::Promise::spawn_local(async move {
            match dialog.pick_file().await {
                Some(file) => {
                    let name = file.file_name();
                    let bytes = file.read().await;
                    Some((name, bytes))
                }
                None => None,
            }
        });
        self.open_pending = Some(promise);
        self.msg = "Choose an image file…".into();
    }

    fn open_path(&mut self, s: &str) {
        let lower = s.to_lowercase();
        let r = if lower.ends_with(".pstep") || lower.ends_with(".json") {
            io::load_project(s)
        } else if lower.ends_with(".psd") {
            io::load_psd(s)
        } else {
            io::load_image(s)
        };
        match r {
            Ok(d) => {
                self.checkpoint("Open");
                let (w, h) = (d.width, d.height);
                self.doc = d;
                self.tex = None;
                self.zoom = (700.0 / w as f32).min(900.0 / h as f32).clamp(0.1, 2.0);
                self.msg = format!("Opened {s} — {w}×{h}");
            }
            Err(e) => self.msg = format!("Open failed: {e:#}"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("png", &["png"])
            .add_filter("jpeg", &["jpg", "jpeg"])
            .add_filter("project", &["pstep"])
            .save_file()
        {
            let s = p.to_string_lossy().to_string();
            let r = if s.ends_with(".pstep") {
                io::save_project(&self.doc, &s)
            } else {
                io::save_image(&self.doc, &s)
            };
            self.msg = match r {
                Ok(()) => format!("Saved {s}"),
                Err(e) => format!("Save failed: {e:#}"),
            };
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn save_dialog(&mut self) {
        if self.save_pending.is_some() {
            self.msg = "Export already in progress…".into();
            return;
        }
        let png = match io::encode_png(&self.doc) {
            Ok(p) => p,
            Err(e) => {
                self.msg = format!("Export failed: {e:#}");
                return;
            }
        };
        let dialog = rfd::AsyncFileDialog::new().set_file_name("photostep.png");
        let promise = poll_promise::Promise::spawn_local(async move {
            match dialog.save_file().await {
                // the browser prompts where to save on write
                Some(file) => match file.write(&png).await {
                    Ok(()) => Some("Exported — check your downloads.".to_string()),
                    Err(e) => Some(format!("Export failed: {e:?}")),
                },
                None => None,
            }
        });
        self.save_pending = Some(promise);
        self.msg = "Choose where to save…".into();
    }

    /// Poll completed browser file tasks (web only; called every frame).
    #[cfg(target_arch = "wasm32")]
    fn poll_web_files(&mut self) {
        if let Some(p) = self.open_pending.take() {
            match p.try_take() {
                Ok(Some((name, bytes))) => {
                    let lower = name.to_lowercase();
                    let r = if lower.ends_with(".psd") {
                        io::load_psd_bytes(&bytes)
                    } else {
                        io::load_image_bytes(&bytes, &name)
                    };
                    match r {
                        Ok(d) => {
                            self.checkpoint("Open");
                            let (w, h) = (d.width, d.height);
                            self.doc = d;
                            self.clear_selection();
                            self.tex = None;
                            self.zoom =
                                (700.0 / w as f32).min(900.0 / h as f32).clamp(0.1, 2.0);
                            self.msg = format!("Opened {name} — {w}×{h}");
                        }
                        Err(e) => self.msg = format!("Could not open {name}: {e:#}"),
                    }
                }
                Ok(None) => self.msg = "Open cancelled.".into(),
                Err(p) => self.open_pending = Some(p),
            }
        }
        if let Some(p) = self.save_pending.take() {
            match p.try_take() {
                Ok(Some(m)) => self.msg = m,
                Ok(None) => self.msg = "Export cancelled.".into(),
                Err(p) => self.save_pending = Some(p),
            }
        }
    }

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New  (white 1280×800)").clicked() {
                        self.checkpoint("New document");
                        self.doc = Document::new(1280, 800, [255, 255, 255, 255]);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Open…").clicked() {
                        self.open_dialog();
                        ui.close();
                    }
                    if ui.button("Save / Export…").clicked() {
                        self.save_dialog();
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.button("↩ Undo  (Ctrl+Z)").clicked() {
                        self.undo();
                        ui.close();
                    }
                    if ui.button("↪ Redo  (Ctrl+Y)").clicked() {
                        self.redo();
                        ui.close();
                    }
                    if ui.button("⧉ Duplicate layer  (Ctrl+J)").clicked() {
                        self.checkpoint("Duplicate layer");
                        self.doc.duplicate_active();
                        self.tex = None;
                        ui.close();
                    }
                });
                ui.menu_button("Image", |ui| {
                    for (label, f) in [
                        ("Invert  (Ctrl+I)", ops::invert as fn(&mut Document)),
                        ("Grayscale", ops::grayscale as fn(&mut Document)),
                        ("Auto Contrast", ops::auto_contrast as fn(&mut Document)),
                    ] {
                        if ui.button(label).clicked() {
                            let owned = label.to_string();
                            self.apply_layer_op(&format!("Image › {owned}"), move |d| f(d));
                            ui.close();
                        }
                    }
                    if ui.button("Rotate 90° CW").clicked() {
                        self.checkpoint("Rotate 90° CW");
                        ops::rotate90_cw(&mut self.doc);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Rotate 180°").clicked() {
                        self.checkpoint("Rotate 180°");
                        ops::rotate90_cw(&mut self.doc);
                        ops::rotate90_cw(&mut self.doc);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Flip Horizontal").clicked() {
                        self.apply_layer_op("Flip horizontal", |d| ops::flip_horizontal(d));
                        ui.close();
                    }
                    if ui.button("Flip Vertical").clicked() {
                        self.apply_layer_op("Flip vertical", |d| ops::flip_vertical(d));
                        ui.close();
                    }
                });
                ui.menu_button("Filter", |ui| {
                    if ui.button("Blur more").clicked() {
                        self.apply_layer_op("Gaussian blur", |d| ops::gaussian_blur(d, 6));
                        ui.close();
                    }
                    if ui.button("Sharpen").clicked() {
                        self.apply_layer_op("Sharpen", |d| ops::sharpen(d, 1.2));
                        ui.close();
                    }
                    if ui.button("Find Edges").clicked() {
                        self.apply_layer_op("Find edges", |d| ops::edge_detect(d));
                        ui.close();
                    }
                    if ui.button("Emboss").clicked() {
                        self.apply_layer_op("Emboss", |d| ops::emboss(d));
                        ui.close();
                    }
                    if ui.button("Pixelate ×8").clicked() {
                        self.apply_layer_op("Pixelate", |d| ops::pixelate(d, 8));
                        ui.close();
                    }
                    if ui.button("Vignette").clicked() {
                        self.apply_layer_op("Vignette", |d| ops::vignette(d, 0.6));
                        ui.close();
                    }
                    if ui.button("Motion Blur").clicked() {
                        self.apply_layer_op("Motion blur", |d| ops::motion_blur(d, 25.0, 12));
                        ui.close();
                    }
                    if ui.button("Radial Blur").clicked() {
                        self.apply_layer_op("Radial blur", |d| ops::radial_blur(d, 30.0));
                        ui.close();
                    }
                    if ui.button("Median (Dust & Scratches)").clicked() {
                        self.apply_layer_op("Median", |d| ops::median(d, 3));
                        ui.close();
                    }
                    if ui.button("High Pass").clicked() {
                        self.apply_layer_op("High pass", |d| ops::high_pass(d, 6));
                        ui.close();
                    }
                });
                ui.menu_button("Select", |ui| {
                    if ui.button("Invert selection").clicked() {
                        self.invert_selection();
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Feather selection").clicked() {
                        self.feather_selection();
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Deselect  (Ctrl+D)").clicked() {
                        self.clear_selection();
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("✂ Crop to selection").clicked() {
                        self.crop_to_selection();
                        ui.close();
                    }
                });
                ui.menu_button("Layer", |ui| {
                    if ui.button("＋ New layer").clicked() {
                        self.checkpoint("New layer");
                        let n = self.doc.layers.len() + 1;
                        self.doc.add_solid_layer(&format!("Layer {n}"), [0, 0, 0, 0]);
                        self.tex = None;
                        ui.close();
                    }
                    ui.menu_button("◑ New Adjustment Layer", |ui| {
                        for (name, adj) in Adjustment::gallery() {
                            if ui.button(name).clicked() {
                                self.checkpoint(&format!("Add {name} layer"));
                                let n = self.doc.layers.len() + 1;
                                self.doc.add_adjustment_layer(&format!("{name} {n}"), adj);
                                self.tex = None;
                                ui.close();
                            }
                        }
                    });
                    if ui.button("Add Layer Mask").clicked() {
                        if self.ensure_pixel_target() {
                            self.checkpoint("Add layer mask");
                            let sel = self.selection_mask();
                            let layer = self.doc.active_layer_mut();
                            layer.ensure_mask();
                            if let (Some(s), Some(mm)) = (sel, layer.mask.as_mut()) {
                                for (a, b) in mm.iter_mut().zip(s.iter()) {
                                    *a = (*a as u16 * *b as u16 / 255) as u8;
                                }
                            }
                            self.paint_target = PaintTarget::Mask;
                            self.tex = None;
                            self.msg = "Mask added — paint black to hide, white to reveal.".into();
                        }
                        ui.close();
                    }
                    if ui.button("Delete Layer Mask").clicked() {
                        self.checkpoint("Delete layer mask");
                        self.doc.active_layer_mut().mask = None;
                        self.paint_target = PaintTarget::Image;
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("⧉ Merge down  (Ctrl+E)").clicked() {
                        if self.doc.can_merge_down() {
                            self.checkpoint("Merge down");
                            self.doc.merge_down();
                            self.tex = None;
                        } else {
                            self.msg = "Merge down needs a pixel layer directly below.".into();
                        }
                        ui.close();
                    }
                    if ui.button("⛶ Flatten image").clicked() {
                        self.checkpoint("Flatten image");
                        self.doc.flatten();
                        self.tex = None;
                        ui.close();
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("★ About PhotoStep").clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                });
            });
        });
    }

    fn left_tools(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("tools").exact_width(148.0).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("◧ PHOTOSTEP").size(19.0).strong().color(BRAND_ORANGE));
                ui.label(egui::RichText::new(format!("STUDIO · v{}", BRAND_VERSION)).small().color(BRAND_AMBER));
            });
            ui.separator();
            for (group, tools) in [
                ("PAINT", &[Tool::Move, Tool::Brush, Tool::Eraser, Tool::CloneStamp, Tool::Fill, Tool::Gradient][..]),
                ("SELECT", &[Tool::Eyedropper, Tool::SelectRect, Tool::SelectEllipse, Tool::MagicWand][..]),
                ("SHAPE", &[Tool::ShapeRect, Tool::ShapeEllipse, Tool::ShapeLine, Tool::Crop][..]),
                ("VIEW", &[Tool::Zoom][..]),
            ] {
                ui.label(egui::RichText::new(group).small().weak());
                egui::Grid::new(group).num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
                    for (i, t) in tools.iter().enumerate() {
                        let active = self.tool == *t;
                        let (rect, resp) =
                            ui.allocate_exact_size(Vec2::new(46.0, 42.0), egui::Sense::click());
                        if active {
                            ui.painter().rect_filled(
                                rect,
                                8.0,
                                Color32::from_rgb(66, 30, 20),
                            );
                            ui.painter().rect_stroke(
                                rect,
                                8.0,
                                egui::Stroke::new(1.5, BRAND_ORANGE),
                                egui::StrokeKind::Middle,
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                8.0,
                                Color32::from_rgb(38, 38, 58),
                            );
                        }
                        let fg = if active {
                            BRAND_ORANGE
                        } else if resp.hovered() {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(154, 154, 176)
                        };
                        paint_tool_icon(ui.painter(), rect.shrink2(Vec2::new(10.0, 8.0)), *t, fg);
                        if resp.clicked() {
                            self.tool = *t;
                        }
                        resp.on_hover_text(format!("{}\n{}", t.name(), t.tip()));
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            }
            ui.separator();
            ui.label("Brush size");
            ui.add(egui::Slider::new(&mut self.brush_size, 1.0..=200.0));
            ui.label("Color");
            ui.color_edit_button_srgba(&mut self.color);
            ui.label("Background");
            ui.color_edit_button_srgba(&mut self.bg);
            ui.separator();
            ui.label("Wand tolerance");
            ui.add(egui::Slider::new(&mut self.wand_tol, 0..=128));
            ui.separator();
            ui.label("Zoom");
            ui.add(egui::Slider::new(&mut self.zoom, 0.1..=4.0).logarithmic(true));
            if ui.button("Fit").clicked() {
                self.zoom = (700.0 / self.doc.width as f32).min(900.0 / self.doc.height as f32).clamp(0.1, 2.0);
            }
        });
    }

    fn right_panels(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("right").exact_width(264.0).show(ctx, |ui| {
            ui.heading("Layers");
            let mut del = None;
            let mut vis_change: Option<(usize, bool)> = None;
            let mut select: Option<usize> = None;
            // snapshot to avoid borrow conflicts
            let infos: Vec<(String, bool, bool, bool)> = self.doc.layers.iter().map(|l| {
                (l.name.clone(), l.visible, l.is_adjustment(), l.mask.is_some())
            }).collect();
            for idx in (0..infos.len()).rev() {
                let (nm, vis, is_adj, has_mask) = &infos[idx];
                ui.horizontal(|ui| {
                    let mut v = *vis;
                    if ui.checkbox(&mut v, "").changed() {
                        vis_change = Some((idx, v));
                    }
                    let active = self.doc.active == idx;
                    let icon = if *is_adj { "◑" } else { "▦" };
                    let masked = if *has_mask { " ◐" } else { "" };
                    let label = format!("{} {} {}{}", if active { "▶" } else { "·" }, icon, nm, masked);
                    if ui.selectable_label(active, label).clicked() {
                        select = Some(idx);
                    }
                    if ui.small_button("✕").clicked() {
                        del = Some(idx);
                    }
                });
            }
            if let Some((i, v)) = vis_change {
                self.checkpoint("Toggle visibility");
                self.doc.layers[i].visible = v;
                self.tex = None;
            }
            if let Some(i) = select {
                self.doc.active = i;
            }
            if let Some(i) = del {
                self.checkpoint("Delete layer");
                self.doc.active = i;
                self.doc.remove_active();
                self.tex = None;
            }
            ui.horizontal(|ui| {
                if ui.button("＋ Add").clicked() {
                    self.checkpoint("New layer");
                    let k = self.doc.layers.len() + 1;
                    self.doc.add_solid_layer(&format!("Layer {k}"), [0, 0, 0, 0]);
                    self.tex = None;
                }
                if ui.button("⧉ Dup").clicked() {
                    self.checkpoint("Duplicate layer");
                    self.doc.duplicate_active();
                    self.tex = None;
                }
                if ui.button("▲").clicked() {
                    self.checkpoint("Move layer");
                    self.doc.move_active(true);
                    self.tex = None;
                }
                if ui.button("▼").clicked() {
                    self.checkpoint("Move layer");
                    self.doc.move_active(false);
                    self.tex = None;
                }
            });
            // active layer props
            ui.separator();
            ui.heading("Active layer");
            {
                let a = self.doc.active;
                let mut op = self.doc.layers[a].opacity;
                let mut blend = self.doc.layers[a].blend;
                let mut nm = self.doc.layers[a].name.clone();
                ui.horizontal(|ui| {
                    ui.label("Opacity");
                    if ui.add(egui::Slider::new(&mut op, 0.0..=1.0)).changed() {
                        self.checkpoint("Layer opacity");
                        self.doc.layers[a].opacity = op;
                        self.tex = None;
                    }
                });
                egui::ComboBox::from_label("Blend")
                    .selected_text(blend.name())
                    .show_ui(ui, |ui| {
                        for m in BlendMode::all() {
                            if ui.selectable_value(&mut blend, *m, m.name()).changed() {
                                self.checkpoint("Blend mode");
                                self.doc.layers[a].blend = blend;
                                self.tex = None;
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("Name");
                    if ui.text_edit_singleline(&mut nm).changed() {
                        self.doc.layers[a].name = nm;
                    }
                });
            }
            ui.separator();
            if self.doc.active_layer().is_adjustment() {
                self.adjustment_editor(ui);
            } else {
            ui.heading("Adjust (active layer)");
            let mut dirty = false;
            ui.add(egui::Slider::new(&mut self.bri, -100..=100).text("Brightness"));
            ui.add(egui::Slider::new(&mut self.con, -100.0..=100.0).text("Contrast"));
            ui.add(egui::Slider::new(&mut self.sat, 0.0..=3.0).text("Saturation"));
            ui.add(egui::Slider::new(&mut self.exp, -3.0..=3.0).text("Exposure"));
            ui.add(egui::Slider::new(&mut self.blur, 1..=24).text("Blur radius"));
            ui.horizontal(|ui| {
                if ui.button("Apply B/C/S").clicked() {
                    let (bri, con, sat, exp) = (self.bri, self.con, self.sat, self.exp);
                    if bri != 0 || con.abs() > 0.01 || (sat - 1.0).abs() > 0.01 || exp.abs() > 0.01 {
                        self.apply_layer_op("Adjust brightness/contrast", move |d| {
                            if bri != 0 { ops::brightness(d, bri); }
                            if con.abs() > 0.01 { ops::contrast(d, con); }
                            if (sat - 1.0).abs() > 0.01 { ops::hue_saturation(d, 0.0, sat); }
                            if exp.abs() > 0.01 { ops::exposure(d, exp); }
                        });
                        dirty = true;
                    }
                }
                if ui.button("Blur").clicked() {
                    let blur = self.blur;
                    self.apply_layer_op("Gaussian blur", move |d| ops::gaussian_blur(d, blur));
                    dirty = true;
                }
            });
            if dirty {
                self.bri = 0;
                self.con = 0.0;
                self.sat = 1.0;
                self.exp = 0.0;
            }
            } // end pixel-layer adjust branch
            self.selection_section(ui);
            self.history_section(ui);
            self.mask_section(ui);
            self.styles_section(ui);
            self.transform_section(ui);
        });
    }

    /// Live editor for the active adjustment layer (draft-then-commit keeps history clean).
    fn adjustment_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Adjustment (live)");
        let before = self.doc.active_layer().adjustment().cloned();
        let Some(mut draft) = before else {
            return;
        };
        let mut changed = false;
        match &mut draft {
            Adjustment::BrightnessContrast { brightness, contrast } => {
                changed |= ui.add(egui::Slider::new(brightness, -100..=100).text("Brightness")).changed();
                changed |= ui.add(egui::Slider::new(contrast, -100.0..=100.0).text("Contrast")).changed();
            }
            Adjustment::Levels { in_lo, in_hi, gamma } => {
                changed |= ui.add(egui::Slider::new(in_lo, 0..=254).text("In low")).changed();
                changed |= ui.add(egui::Slider::new(in_hi, 1..=255).text("In high")).changed();
                changed |= ui.add(egui::Slider::new(gamma, 0.1..=5.0).text("Gamma")).changed();
            }
            Adjustment::Curves { points } => {
                ui.horizontal(|ui| {
                    if ui.button("Identity").clicked() {
                        *points = vec![(0, 0), (255, 255)];
                        changed = true;
                    }
                    if ui.button("S-curve").clicked() {
                        *points = vec![(0, 0), (64, 56), (192, 200), (255, 255)];
                        changed = true;
                    }
                    if ui.button("Fade").clicked() {
                        *points = vec![(0, 24), (255, 232)];
                        changed = true;
                    }
                });
                ui.label("Drag points · click to add · right-click removes");
                changed |= self.curve_editor(ui, points);
            }
            Adjustment::HueSaturation { hue_deg, sat_mult, lightness } => {
                changed |= ui.add(egui::Slider::new(hue_deg, -180.0..=180.0).text("Hue")).changed();
                changed |= ui.add(egui::Slider::new(sat_mult, 0.0..=3.0).text("Saturation")).changed();
                changed |= ui.add(egui::Slider::new(lightness, -100.0..=100.0).text("Lightness")).changed();
            }
            Adjustment::Vibrance { amount } => {
                changed |= ui.add(egui::Slider::new(amount, -100.0..=100.0).text("Vibrance")).changed();
            }
            Adjustment::Exposure { ev } => {
                changed |= ui.add(egui::Slider::new(ev, -5.0..=5.0).text("Exposure")).changed();
            }
            Adjustment::ColorBalance { dr, dg, db } => {
                changed |= ui.add(egui::Slider::new(dr, -100..=100).text("Red")).changed();
                changed |= ui.add(egui::Slider::new(dg, -100..=100).text("Green")).changed();
                changed |= ui.add(egui::Slider::new(db, -100..=100).text("Blue")).changed();
            }
            Adjustment::BlackWhite { r, g, b } => {
                changed |= ui.add(egui::Slider::new(r, 0.0..=1.0).text("Reds")).changed();
                changed |= ui.add(egui::Slider::new(g, 0.0..=1.0).text("Greens")).changed();
                changed |= ui.add(egui::Slider::new(b, 0.0..=1.0).text("Blues")).changed();
            }
            Adjustment::PhotoFilter { color, density } => {
                ui.horizontal(|ui| {
                    ui.label("Color");
                    changed |= ui.color_edit_button_srgb(color).changed();
                });
                changed |= ui.add(egui::Slider::new(density, 0.0..=1.0).text("Density")).changed();
            }
            Adjustment::ChannelMixer { r, g, b } => {
                for (label, ch) in [("R", r), ("G", g), ("B", b)] {
                    ui.label(format!("Output {label}"));
                    changed |= ui.add(egui::Slider::new(&mut ch[0], -2.0..=2.0).text("from R")).changed();
                    changed |= ui.add(egui::Slider::new(&mut ch[1], -2.0..=2.0).text("from G")).changed();
                    changed |= ui.add(egui::Slider::new(&mut ch[2], -2.0..=2.0).text("from B")).changed();
                }
            }
            Adjustment::GradientMap { dark, light } => {
                ui.horizontal(|ui| {
                    ui.label("Shadows");
                    changed |= ui.color_edit_button_srgb(dark).changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Lights");
                    changed |= ui.color_edit_button_srgb(light).changed();
                });
            }
            Adjustment::ShadowsHighlights { shadows, highlights } => {
                changed |= ui.add(egui::Slider::new(shadows, 0.0..=100.0).text("Shadows")).changed();
                changed |= ui.add(egui::Slider::new(highlights, 0.0..=100.0).text("Highlights")).changed();
            }
            Adjustment::Threshold { t } => {
                changed |= ui.add(egui::Slider::new(t, 0..=255).text("Level")).changed();
            }
            Adjustment::Posterize { levels } => {
                changed |= ui.add(egui::Slider::new(levels, 2..=16).text("Levels")).changed();
            }
            Adjustment::Invert | Adjustment::Grayscale | Adjustment::AutoContrast => {
                ui.label("No parameters — always live.");
            }
        }
        if changed {
            let label = format!("Adjust {}", draft.name());
            self.checkpoint(&label);
            if let Some(a) = self.doc.active_layer_mut().adjustment_mut() {
                *a = draft;
            }
            self.tex = None;
        }
        if ui.button("Delete adjustment").clicked() {
            self.checkpoint("Delete layer");
            self.doc.remove_active();
            self.tex = None;
        }
    }

    fn selection_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Selection", |ui| {
            ui.add(egui::Slider::new(&mut self.sel_feather, 0..=64).text("Feather px"));
            ui.horizontal(|ui| {
                if ui.button("Feather").clicked() {
                    self.feather_selection();
                    self.tex = None;
                }
                if ui.button("Invert").clicked() {
                    self.invert_selection();
                    self.tex = None;
                }
                if ui.button("None").clicked() {
                    self.clear_selection();
                    self.tex = None;
                }
            });
            if ui.button("✂ Crop to selection").clicked() {
                self.crop_to_selection();
            }
        });
    }

    fn history_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("History", |ui| {
            let labels = self.history.labels();
            let pos = self.history.position();
            egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                if ui.selectable_label(pos == 0, "◇ Original state").clicked() {
                    let cur = self.doc.clone();
                    if let Some(d) = self.history.goto(&cur, 0) {
                        self.doc = d;
                        self.tex = None;
                    }
                }
                for (i, l) in labels.iter().enumerate() {
                    let here = pos == i + 1;
                    let text = format!("{} {}", if here { "●" } else { "○" }, l);
                    if ui.selectable_label(here, text).clicked() {
                        let cur = self.doc.clone();
                        if let Some(d) = self.history.goto(&cur, i + 1) {
                            self.doc = d;
                            self.tex = None;
                        }
                    }
                }
            });
        });
    }

    fn mask_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Layer Mask", |ui| {
            if !self.active_is_pixel() {
                ui.label("Select a pixel layer.");
                return;
            }
            let has = self.doc.active_layer().mask.is_some();
            ui.label(if has { "Mask: attached" } else { "Mask: none" });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.paint_target, PaintTarget::Image, "🖌 Image");
                ui.selectable_value(&mut self.paint_target, PaintTarget::Mask, "◐ Mask");
            });
            ui.horizontal(|ui| {
                if ui.button("Add").clicked() {
                    self.checkpoint("Add layer mask");
                    self.doc.active_layer_mut().ensure_mask();
                    self.paint_target = PaintTarget::Mask;
                    self.tex = None;
                }
                if has && ui.button("Delete").clicked() {
                    self.checkpoint("Delete layer mask");
                    self.doc.active_layer_mut().mask = None;
                    self.paint_target = PaintTarget::Image;
                    self.tex = None;
                }
            });
        });
    }

    fn styles_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Layer Styles", |ui| {
            if !self.active_is_pixel() {
                ui.label("Select a pixel layer.");
                return;
            }
            let a = self.doc.active;
            let mut fx = self.doc.layers[a].effects;
            let mut sh_on = fx.drop_shadow.is_some();
            if ui.checkbox(&mut sh_on, "Drop Shadow").changed() {
                fx.drop_shadow = if sh_on { Some(Default::default()) } else { None };
            }
            if let Some(sh) = fx.drop_shadow.as_mut() {
                ui.add(egui::Slider::new(&mut sh.dx, -64..=64).text("Offset X"));
                ui.add(egui::Slider::new(&mut sh.dy, -64..=64).text("Offset Y"));
                ui.add(egui::Slider::new(&mut sh.blur, 0..=64).text("Blur"));
                ui.add(egui::Slider::new(&mut sh.opacity, 0.0..=1.0).text("Opacity"));
                ui.color_edit_button_srgb(&mut sh.color);
            }
            let mut gl_on = fx.outer_glow.is_some();
            if ui.checkbox(&mut gl_on, "Outer Glow").changed() {
                fx.outer_glow = if gl_on { Some(Default::default()) } else { None };
            }
            if let Some(gl) = fx.outer_glow.as_mut() {
                ui.add(egui::Slider::new(&mut gl.blur, 0..=64).text("Spread"));
                ui.add(egui::Slider::new(&mut gl.opacity, 0.0..=1.0).text("Opacity"));
                ui.color_edit_button_srgb(&mut gl.color);
            }
            let mut st_on = fx.stroke.is_some();
            if ui.checkbox(&mut st_on, "Stroke").changed() {
                fx.stroke = if st_on { Some(Default::default()) } else { None };
            }
            if let Some(st) = fx.stroke.as_mut() {
                ui.add(egui::Slider::new(&mut st.width, 1..=32).text("Width"));
                ui.add(egui::Slider::new(&mut st.opacity, 0.0..=1.0).text("Opacity"));
                ui.color_edit_button_srgb(&mut st.color);
            }
            if fx != self.doc.layers[a].effects {
                self.checkpoint("Layer style");
                self.doc.layers[a].effects = fx;
                self.tex = None;
            }
        });
    }

    fn transform_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Transform", |ui| {
            ui.add(egui::Slider::new(&mut self.scale_x, 10.0..=400.0).text("Scale X %"));
            ui.add(egui::Slider::new(&mut self.scale_y, 10.0..=400.0).text("Scale Y %"));
            if ui.button("Apply Scale").clicked() {
                let (sx, sy) = (self.scale_x, self.scale_y);
                if self.ensure_pixel_target() {
                    self.checkpoint("Scale layer");
                    ops::scale_content(&mut self.doc, sx, sy);
                    self.tex = None;
                }
            }
            ui.add(egui::Slider::new(&mut self.rot_deg, -180.0..=180.0).text("Rotate °"));
            if ui.button("Apply Rotate").clicked() {
                let deg = self.rot_deg;
                if self.ensure_pixel_target() {
                    self.checkpoint("Rotate layer");
                    ops::rotate_arbitrary(&mut self.doc, deg);
                    self.tex = None;
                }
            }
        });
    }

    fn canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.refresh_texture(ctx);
            let Some(tex) = self.tex.clone() else { return };
            let size = Vec2::new(self.doc.width as f32 * self.zoom, self.doc.height as f32 * self.zoom);
            egui::ScrollArea::both().show(ui, |ui| {
                let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                // floating canvas: soft shadow frame + checkerboard for transparency
                ui.painter().rect_filled(rect.expand(10.0), 12.0, Color32::from_rgba_unmultiplied(0, 0, 0, 90));
                ui.painter().rect_filled(rect, 0.0, Color32::from_gray(32));
                ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                // selection overlay (skipped when degenerate to avoid streaks)
                if let Some(s) = self.sel {
                    if s.width() > 1.0 && s.height() > 1.0 {
                    let p0 = rect.min + Vec2::new(s.min.x * self.zoom, s.min.y * self.zoom);
                    let p1 = rect.min + Vec2::new(s.max.x * self.zoom, s.max.y * self.zoom);
                    ui.painter().rect_stroke(egui::Rect::from_min_max(p0.into(), p1.into()), 0.0, (1.5, Color32::YELLOW), egui::StrokeKind::Middle);
                    }
                }
                // interactions (zoom copied and sanitized so the helper closure doesn't borrow self)
                let zoom = self.zoom.max(0.05);
                let to_img = |p: egui::Pos2| {
                    egui::pos2((p.x - rect.min.x) / zoom, (p.y - rect.min.y) / zoom)
                };
                if resp.drag_started() {
                    self.painting = true;
                    let painty = matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::CloneStamp | Tool::Move);
                    if painty && self.ensure_pixel_target() {
                        if matches!(self.tool, Tool::CloneStamp | Tool::Move) {
                            self.stroke_snap = Some(self.doc.active_layer().pixels.clone());
                        }
                        let label = match self.tool {
                            Tool::Eraser => "Eraser",
                            Tool::CloneStamp => "Clone stamp",
                            Tool::Move => "Move layer",
                            _ => "Brush",
                        };
                        self.checkpoint(label);
                    }
                    if let Some(p) = resp.interact_pointer_pos() {
                        let ip = to_img(p);
                        self.sel_start = Some((ip.x, ip.y));
                        self.move_start = Some((ip.x, ip.y));
                        self.drag_cur = Some((ip.x, ip.y));
                        self.last_dab = Some((ip.x, ip.y));
                        self.grad_start = Some((ip.x, ip.y));
                        self.shape_start = Some((ip.x, ip.y));
                        if matches!(self.tool, Tool::Brush) {
                            self.apply_brush(ip, false);
                        } else if matches!(self.tool, Tool::Eraser) {
                            self.apply_brush(ip, true);
                        } else if matches!(self.tool, Tool::CloneStamp) {
                            self.clone_dab(ip.x, ip.y);
                        }
                    }
                } else if resp.dragged() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        let ip = to_img(p);
                        self.drag_cur = Some((ip.x, ip.y));
                        match self.tool {
                            // interpolated dabs: smooth lines even on fast strokes
                            Tool::Brush | Tool::Eraser | Tool::CloneStamp => {
                                let erase = matches!(self.tool, Tool::Eraser);
                                let spacing = (self.brush_size / 5.0).max(1.5);
                                let (px, py) = self.last_dab.unwrap_or((ip.x, ip.y));
                                let dist = ((ip.x - px).powi(2) + (ip.y - py).powi(2)).sqrt();
                                let steps = ((dist / spacing).floor() as usize).max(1);
                                for i in 1..=steps {
                                    let t = i as f32 / steps as f32;
                                    let qx = px + (ip.x - px) * t;
                                    let qy = py + (ip.y - py) * t;
                                    if matches!(self.tool, Tool::CloneStamp) {
                                        self.clone_dab(qx, qy);
                                    } else {
                                        self.apply_brush(egui::pos2(qx, qy), erase);
                                    }
                                }
                                self.last_dab = Some((ip.x, ip.y));
                            }
                            Tool::Move => {
                                if let Some((ox, oy)) = self.move_start {
                                    self.move_dab(ip.x - ox, ip.y - oy);
                                }
                            }
                            Tool::SelectRect | Tool::SelectEllipse => {
                                if let Some((x0, y0)) = self.sel_start {
                                    self.sel_ellipse = matches!(self.tool, Tool::SelectEllipse);
                                    self.sel = Some(egui::Rect::from_two_pos(
                                        egui::pos2(x0, y0),
                                        egui::pos2(ip.x.clamp(0.0, self.doc.width as f32), ip.y.clamp(0.0, self.doc.height as f32)),
                                    ));
                                }
                            }
                            _ => {}
                        }
                    }
                } else if resp.drag_stopped() {
                    self.painting = false;
                    self.stroke_snap = None;
                    match self.tool {
                        Tool::Gradient => {
                            if let (Some((x0, y0)), Some((x1, y1))) = (self.grad_start, self.drag_cur) {
                                let d = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
                                if d > 2.0 {
                                    self.render_gradient(x0, y0, x1, y1);
                                }
                            }
                        }
                        Tool::ShapeRect | Tool::ShapeEllipse | Tool::ShapeLine => {
                            if let (Some((x0, y0)), Some((x1, y1))) = (self.shape_start, self.drag_cur) {
                                let d = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
                                if d > 2.0 {
                                    let t = self.tool;
                                    self.raster_shape(t, x0, y0, x1, y1);
                                }
                            }
                        }
                        Tool::Crop => {
                            if let (Some((x0, y0)), Some((x1, y1))) = (self.shape_start, self.drag_cur) {
                                let d = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
                                if d > 4.0 {
                                    self.sel = Some(egui::Rect::from_two_pos(
                                        egui::pos2(x0, y0),
                                        egui::pos2(x1.clamp(0.0, self.doc.width as f32), y1.clamp(0.0, self.doc.height as f32)),
                                    ));
                                    self.sel_ellipse = false;
                                    self.crop_to_selection();
                                }
                            }
                        }
                        _ => {}
                    }
                    self.grad_start = None;
                    self.shape_start = None;
                    self.drag_cur = None;
                    self.last_dab = None;
                    self.move_start = None;
                    // re-upload once
                    ctx.request_repaint();
                }
                if resp.clicked() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        let ip = to_img(p);
                        let (ix, iy) = (ip.x as i32, ip.y as i32);
                        if ix >= 0 && iy >= 0 && ix < self.doc.width as i32 && iy < self.doc.height as i32 {
                            match self.tool {
                                Tool::CloneStamp if ui.input(|i| i.modifiers.alt) => {
                                    self.clone_src = Some((ip.x, ip.y));
                                    self.msg = "Clone source set — paint to copy.".into();
                                }
                                Tool::Eyedropper => {
                                    let o = ((iy as u32 * self.doc.width + ix as u32) * 4) as usize;
                                    let layer = self.doc.active_layer();
                                    if layer.pixels.len() >= o + 4 {
                                        let px = &layer.pixels[o..o + 4];
                                        self.color = Color32::from_rgb(px[0], px[1], px[2]);
                                        self.msg = format!("Picked rgb({},{},{})", px[0], px[1], px[2]);
                                    } else {
                                        self.msg = "Nothing to pick on this layer.".into();
                                    }
                                }
                                Tool::Fill => {
                                    if self.ensure_pixel_target() {
                                        self.checkpoint("Fill");
                                        let c = [self.color.r(), self.color.g(), self.color.b(), 255];
                                        // fill selection or whole layer
                                        if let Some(s) = self.sel {
                                            let layer = self.doc.active_layer_mut();
                                            let x0 = s.min.x.floor().max(0.0) as u32;
                                            let y0 = s.min.y.floor().max(0.0) as u32;
                                            let x1 = (s.max.x.ceil().min(layer.width as f32)) as u32;
                                            let y1 = (s.max.y.ceil().min(layer.height as f32)) as u32;
                                            for y in y0..y1 {
                                                for x in x0..x1 {
                                                    let o = ((y * layer.width + x) * 4) as usize;
                                                    if o + 4 <= layer.pixels.len() {
                                                        layer.pixels[o..o + 4].copy_from_slice(&c);
                                                    }
                                                }
                                            }
                                        } else {
                                            self.doc.active_layer_mut().fill(c);
                                        }
                                        self.tex = None;
                                    }
                                }
                                Tool::MagicWand => {
                                    let flat = self.doc.composite();
                                    let (w, h) = (self.doc.width as usize, self.doc.height as usize);
                                    let mut m = ops::magic_wand(&flat, w, h, ix as usize, iy as usize, self.wand_tol);
                                    if self.sel_feather > 0 {
                                        m = ops::blur_channel(&m, w, h, self.sel_feather as usize);
                                    }
                                    if ui.input(|i| i.modifiers.shift) {
                                        if let Some(old) = &self.sel_mask {
                                            for (a, b) in m.iter_mut().zip(old.iter()) {
                                                *a = (*a).max(*b);
                                            }
                                        }
                                    }
                                    self.sel_mask = Some(m);
                                    self.sel = None;
                                    self.msg = format!("Magic Wand selection (tolerance {}).", self.wand_tol);
                                    self.tex = None;
                                }
                                Tool::Zoom => {
                                    if ui.input(|i| i.modifiers.shift) {
                                        self.zoom = (self.zoom / 1.25).clamp(0.1, 4.0);
                                    } else {
                                        self.zoom = (self.zoom * 1.25).clamp(0.1, 4.0);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                // drag previews for gradient / shapes
                if resp.dragged() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        let ip = to_img(p);
                        let at = |x: f32, y: f32| rect.min + Vec2::new(x * zoom, y * zoom);
                        match self.tool {
                            Tool::Gradient => {
                                if let Some((x0, y0)) = self.grad_start {
                                    ui.painter().line_segment([at(x0, y0), at(ip.x, ip.y)], (2.0, Color32::WHITE));
                                }
                            }
                            Tool::ShapeRect | Tool::Crop => {
                                if let Some((x0, y0)) = self.shape_start {
                                    let r = egui::Rect::from_two_pos(at(x0, y0), at(ip.x, ip.y));
                                    ui.painter().rect_stroke(r, 0.0, (1.5, Color32::WHITE), egui::StrokeKind::Middle);
                                }
                            }
                            Tool::ShapeEllipse => {
                                if let Some((x0, y0)) = self.shape_start {
                                    let r = egui::Rect::from_two_pos(at(x0, y0), at(ip.x, ip.y));
                                    ui.painter().add(egui::Shape::ellipse_stroke(
                                        r.center(),
                                        r.size() / 2.0,
                                        (1.5, Color32::WHITE),
                                    ));
                                }
                            }
                            Tool::ShapeLine => {
                                if let Some((x0, y0)) = self.shape_start {
                                    ui.painter().line_segment([at(x0, y0), at(ip.x, ip.y)], (2.0, Color32::WHITE));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // keyboard shortcuts
                if ui.input(|i| i.key_pressed(egui::Key::Z) && i.modifiers.ctrl) {
                    self.undo();
                }
                if ui.input(|i| i.key_pressed(egui::Key::Y) && i.modifiers.ctrl) {
                    self.redo();
                }
                if ui.input(|i| i.key_pressed(egui::Key::D) && i.modifiers.ctrl) {
                    self.clear_selection();
                    self.tex = None;
                }
                let _ = self.painting;
            });
        });
    }
}

impl eframe::App for PhotoStepApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // eframe re-applies the OS theme and wipes custom visuals: re-assert ours.
        if !ctx.style().visuals.dark_mode {
            apply_brand_theme(ctx);
        }
        #[cfg(target_arch = "wasm32")]
        self.poll_web_files();
        self.menu_bar(ctx);
        self.left_tools(ctx);
        self.right_panels(ctx);
        if self.show_about {
            egui::Window::new("About PhotoStep")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("◧ PHOTOSTEP").size(26.0).strong().color(BRAND_ORANGE));
                        ui.label("Fast layer-based image editor in pure Rust.");
                        ui.add_space(6.0);
                        ui.label(format!("Version {}", BRAND_VERSION));
                        ui.label(BRAND_COPYRIGHT);
                        ui.add_space(8.0);
                        if ui.button("Close").clicked() {
                            self.show_about = false;
                        }
                    });
                });
        }
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("◧ {:?}", self.tool)).strong().color(BRAND_ORANGE));
                ui.label(format!("{}×{}  •  {} layers  •  {:.0}%",
                    self.doc.width, self.doc.height, self.doc.layers.len(), self.zoom * 100.0));
                ui.separator();
                ui.label(&self.msg);
            });
        });
        self.canvas(ctx);
    }
}
