//! PhotoStep desktop app: layer-based editor layout in egui/eframe.
//! Left: toolbox. Center: canvas. Right: layers + adjustments. Top: menu. Bottom: status.

use egui::{Color32, TextureHandle, Vec2};
use crate::core::{BlendMode, Document, History};
use crate::{io, ops};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Move,
    Brush,
    Eraser,
    Fill,
    Eyedropper,
    SelectRect,
    SelectEllipse,
    Zoom,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Move => "✥ Move (V)",
            Tool::Brush => "🖌 Brush (B)",
            Tool::Eraser => "⌫ Eraser (E)",
            Tool::Fill => "🪣 Fill (G)",
            Tool::Eyedropper => "💧 Picker (I)",
            Tool::SelectRect => "▭ Rect Sel (M)",
            Tool::SelectEllipse => "◯ Ellipse Sel",
            Tool::Zoom => "🔍 Zoom (Z)",
        }
    }
    fn all() -> &'static [Tool] {
        &[Tool::Move, Tool::Brush, Tool::Eraser, Tool::Fill, Tool::Eyedropper, Tool::SelectRect, Tool::SelectEllipse, Tool::Zoom]
    }
}

// ---------- PhotoStep brand identity ----------
pub const BRAND_NAME: &str = "PhotoStep";
pub const BRAND_VERSION: &str = "0.1.0";
pub const BRAND_COPYRIGHT: &str = "© 2026 salim-slimani. All rights reserved.";
pub const BRAND_ORANGE: Color32 = Color32::from_rgb(255, 90, 40);
pub const BRAND_AMBER: Color32 = Color32::from_rgb(255, 176, 58);
pub const BRAND_AQUA: Color32 = Color32::from_rgb(53, 208, 197);

/// Apply the PhotoStep visual identity: dark ink surfaces, step-orange accents.
fn apply_brand_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.selection.bg_fill = BRAND_ORANGE;
    style.visuals.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(255, 110, 62);
    style.visuals.widgets.active.bg_fill = BRAND_ORANGE;
    style.visuals.widgets.open.bg_fill = Color32::from_rgb(60, 60, 86);
    style.visuals.window_rounding = egui::Rounding::same(10.0);
    style.visuals.menu_rounding = egui::Rounding::same(8.0);
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
    painting: bool,
    sel: Option<egui::Rect>, // in image pixels
    sel_start: Option<(f32, f32)>,
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
            painting: false,
            sel: None,
            sel_start: None,
            msg: "Ready — File › Open an image, or paint on the canvas.".into(),
            show_about: false,
            bri: 0, con: 0.0, sat: 1.0, exp: 0.0, blur: 4,
        }
    }

    fn checkpoint(&mut self) {
        self.history.push(&self.doc);
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
            let flat = self.doc.composite();
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [self.doc.width as usize, self.doc.height as usize],
                &flat,
            );
            self.tex = Some(ctx.load_texture("canvas", img, egui::TextureOptions::LINEAR));
            self.tex_size = (self.doc.width, self.doc.height);
        }
    }

    fn apply_brush(&mut self, img_pos: egui::Pos2, erase: bool) {
        let layer = self.doc.active_layer_mut();
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
                    // soft edge
                    let a = if erase { 1.0 } else { (1.0 - d / r * 0.6).clamp(0.0, 1.0) };
                    let o = ((y * layer.width + x) * 4) as usize;
                    if erase {
                        layer.pixels[o + 3] = 0;
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
            .add_filter("images", &["png", "jpg", "jpeg", "tiff", "bmp", "webp", "gif", "qoi", "pstep", "json"])
            .pick_file()
        {
            let s = p.to_string_lossy().to_string();
            self.open_path(&s);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn open_dialog(&mut self) {
        self.msg = "File dialogs need the desktop build — the web demo starts from a blank canvas.".into();
    }

    fn open_path(&mut self, s: &str) {
        let r = if s.ends_with(".pstep") || s.ends_with(".json") {
            io::load_project(s)
        } else {
            io::load_image(s)
        };
        match r {
            Ok(d) => {
                self.checkpoint();
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
        self.msg = "Saving files needs the desktop build — the web demo is for trying the tools.".into();
    }

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("🆕 New  (white 1280×800)").clicked() {
                        self.checkpoint();
                        self.doc = Document::new(1280, 800, [255, 255, 255, 255]);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("📂 Open…").clicked() {
                        self.open_dialog();
                        ui.close();
                    }
                    if ui.button("💾 Save / Export…").clicked() {
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
                        self.checkpoint();
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
                            self.checkpoint();
                            f(&mut self.doc);
                            self.tex = None;
                            ui.close();
                        }
                    }
                    if ui.button("Rotate 90° CW").clicked() {
                        self.checkpoint();
                        ops::rotate90_cw(&mut self.doc);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Flip Horizontal").clicked() {
                        self.checkpoint();
                        ops::flip_horizontal(&mut self.doc);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Flip Vertical").clicked() {
                        self.checkpoint();
                        ops::flip_vertical(&mut self.doc);
                        self.tex = None;
                        ui.close();
                    }
                });
                ui.menu_button("Filter", |ui| {
                    if ui.button("Blur more").clicked() {
                        self.checkpoint();
                        ops::gaussian_blur(&mut self.doc, 6);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Sharpen").clicked() {
                        self.checkpoint();
                        ops::sharpen(&mut self.doc, 1.2);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Find Edges").clicked() {
                        self.checkpoint();
                        ops::edge_detect(&mut self.doc);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Emboss").clicked() {
                        self.checkpoint();
                        ops::emboss(&mut self.doc);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Pixelate ×8").clicked() {
                        self.checkpoint();
                        ops::pixelate(&mut self.doc, 8);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("Vignette").clicked() {
                        self.checkpoint();
                        ops::vignette(&mut self.doc, 0.6);
                        self.tex = None;
                        ui.close();
                    }
                });
                ui.menu_button("Layer", |ui| {
                    if ui.button("＋ New layer").clicked() {
                        self.checkpoint();
                        let n = self.doc.layers.len() + 1;
                        self.doc.add_solid_layer(&format!("Layer {n}"), [0, 0, 0, 0]);
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("⧉ Merge down  (Ctrl+E)").clicked() {
                        self.checkpoint();
                        self.doc.merge_down();
                        self.tex = None;
                        ui.close();
                    }
                    if ui.button("⛶ Flatten image").clicked() {
                        self.checkpoint();
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
                ui.label(egui::RichText::new("◧ PHOTOSTEP").size(17.0).strong().color(BRAND_ORANGE));
                ui.label(egui::RichText::new(format!("v{}", BRAND_VERSION)).small().weak());
            });
            ui.separator();
            ui.heading("Tools");
            for t in Tool::all() {
                ui.selectable_value(&mut self.tool, *t, t.name());
            }
            ui.separator();
            ui.label("Brush size");
            ui.add(egui::Slider::new(&mut self.brush_size, 1.0..=200.0));
            ui.label("Color");
            ui.color_edit_button_srgba(&mut self.color);
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
            let infos: Vec<(String, bool)> =
                self.doc.layers.iter().map(|l| (l.name.clone(), l.visible)).collect();
            for idx in (0..infos.len()).rev() {
                let (nm, vis) = &infos[idx];
                ui.horizontal(|ui| {
                    let mut v = *vis;
                    if ui.checkbox(&mut v, "").changed() {
                        vis_change = Some((idx, v));
                    }
                    let active = self.doc.active == idx;
                    let label = format!("{} {}", if active { "▶" } else { "·" }, nm);
                    if ui.selectable_label(active, label).clicked() {
                        select = Some(idx);
                    }
                    if ui.small_button("✕").clicked() {
                        del = Some(idx);
                    }
                });
            }
            if let Some((i, v)) = vis_change {
                self.checkpoint();
                self.doc.layers[i].visible = v;
                self.tex = None;
            }
            if let Some(i) = select {
                self.doc.active = i;
            }
            if let Some(i) = del {
                self.checkpoint();
                self.doc.active = i;
                self.doc.remove_active();
                self.tex = None;
            }
            ui.horizontal(|ui| {
                if ui.button("＋ Add").clicked() {
                    self.checkpoint();
                    let k = self.doc.layers.len() + 1;
                    self.doc.add_solid_layer(&format!("Layer {k}"), [0, 0, 0, 0]);
                    self.tex = None;
                }
                if ui.button("⧉ Dup").clicked() {
                    self.checkpoint();
                    self.doc.duplicate_active();
                    self.tex = None;
                }
                if ui.button("▲").clicked() {
                    self.checkpoint();
                    self.doc.move_active(true);
                    self.tex = None;
                }
                if ui.button("▼").clicked() {
                    self.checkpoint();
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
                        self.checkpoint();
                        self.doc.layers[a].opacity = op;
                        self.tex = None;
                    }
                });
                egui::ComboBox::from_label("Blend")
                    .selected_text(blend.name())
                    .show_ui(ui, |ui| {
                        for m in BlendMode::all() {
                            if ui.selectable_value(&mut blend, *m, m.name()).changed() {
                                self.checkpoint();
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
            ui.heading("Adjust (active layer)");
            let mut dirty = false;
            ui.add(egui::Slider::new(&mut self.bri, -100..=100).text("Brightness"));
            ui.add(egui::Slider::new(&mut self.con, -100.0..=100.0).text("Contrast"));
            ui.add(egui::Slider::new(&mut self.sat, 0.0..=3.0).text("Saturation"));
            ui.add(egui::Slider::new(&mut self.exp, -3.0..=3.0).text("Exposure"));
            ui.add(egui::Slider::new(&mut self.blur, 1..=24).text("Blur radius"));
            ui.horizontal(|ui| {
                if ui.button("Apply B/C/S").clicked() {
                    self.checkpoint();
                    if self.bri != 0 { ops::brightness(&mut self.doc, self.bri); }
                    if self.con.abs() > 0.01 { ops::contrast(&mut self.doc, self.con); }
                    if (self.sat - 1.0).abs() > 0.01 { ops::hue_saturation(&mut self.doc, 0.0, self.sat); }
                    if self.exp.abs() > 0.01 { ops::exposure(&mut self.doc, self.exp); }
                    self.tex = None;
                    dirty = true;
                }
                if ui.button("Blur").clicked() {
                    self.checkpoint();
                    ops::gaussian_blur(&mut self.doc, self.blur);
                    self.tex = None;
                    dirty = true;
                }
            });
            if dirty {
                self.bri = 0;
                self.con = 0.0;
                self.sat = 1.0;
                self.exp = 0.0;
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
                // checkerboard for transparency
                ui.painter().rect_filled(rect, 0.0, Color32::from_gray(32));
                ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                // selection overlay
                if let Some(s) = self.sel {
                    let p0 = rect.min + Vec2::new(s.min.x * self.zoom, s.min.y * self.zoom);
                    let p1 = rect.min + Vec2::new(s.max.x * self.zoom, s.max.y * self.zoom);
                    ui.painter().rect_stroke(egui::Rect::from_min_max(p0.into(), p1.into()), 0.0, (1.5, Color32::YELLOW));
                }
                // interactions
                let to_img = |p: egui::Pos2| {
                    egui::pos2((p.x - rect.min.x) / self.zoom, (p.y - rect.min.y) / self.zoom)
                };
                if resp.drag_started() {
                    self.painting = true;
                    if matches!(self.tool, Tool::Brush | Tool::Eraser) {
                        self.checkpoint();
                    }
                    if let Some(p) = resp.interact_pointer_pos() {
                        let ip = to_img(p);
                        self.sel_start = Some((ip.x, ip.y));
                        if matches!(self.tool, Tool::Brush) {
                            self.apply_brush(ip, false);
                        } else if matches!(self.tool, Tool::Eraser) {
                            self.apply_brush(ip, true);
                        }
                    }
                } else if resp.dragged() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        let ip = to_img(p);
                        match self.tool {
                            Tool::Brush => self.apply_brush(ip, false),
                            Tool::Eraser => self.apply_brush(ip, true),
                            Tool::SelectRect | Tool::SelectEllipse => {
                                if let Some((x0, y0)) = self.sel_start {
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
                    // re-upload once
                    ctx.request_repaint();
                }
                if resp.clicked() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        let ip = to_img(p);
                        let (ix, iy) = (ip.x as i32, ip.y as i32);
                        if ix >= 0 && iy >= 0 && ix < self.doc.width as i32 && iy < self.doc.height as i32 {
                            match self.tool {
                                Tool::Eyedropper => {
                                    let o = ((iy as u32 * self.doc.width + ix as u32) * 4) as usize;
                                    let px = &self.doc.active_layer().pixels[o..o + 4];
                                    self.color = Color32::from_rgb(px[0], px[1], px[2]);
                                    self.msg = format!("Picked rgb({},{},{})", px[0], px[1], px[2]);
                                }
                                Tool::Fill => {
                                    self.checkpoint();
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
                                                let inside = match self.tool {
                                                    _ => true,
                                                };
                                                let _ = inside;
                                                let o = ((y * layer.width + x) * 4) as usize;
                                                layer.pixels[o..o + 4].copy_from_slice(&c);
                                            }
                                        }
                                    } else {
                                        self.doc.active_layer_mut().fill(c);
                                    }
                                    self.tex = None;
                                }
                                _ => {}
                            }
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
                let _ = self.painting;
            });
        });
    }
}

impl eframe::App for PhotoStepApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                ui.label(format!("📐 {}×{}  •  {} layers  •  🔍 {:.0}%  •  🖌 {:?}",
                    self.doc.width, self.doc.height, self.doc.layers.len(), self.zoom * 100.0, self.tool));
                ui.separator();
                ui.label(&self.msg);
            });
        });
        self.canvas(ctx);
    }
}
