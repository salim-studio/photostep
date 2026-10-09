//! PhotoStep ops: fast parallel adjustments, filters, transforms.
//! All pixel loops use rayon for multi-core speed.
//!
//! Convention: every pixel op has a `_buf` variant working on a raw RGBA8
//! slice (used by adjustment layers), plus a `Document` wrapper that targets
//! the active layer.

use rayon::prelude::*;
use crate::core::{Adjustment, Document};

#[inline]
fn clamp_u8(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

// ---------------------------------------------------------------------------
// Adjustments (buffer level)
// ---------------------------------------------------------------------------

pub fn brightness_buf(buf: &mut [u8], delta: i16) {
    buf.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            p[i] = (p[i] as i16 + delta).clamp(0, 255) as u8;
        }
    });
}

pub fn contrast_buf(buf: &mut [u8], amount: f32) {
    // amount: -100..100
    let f = (259.0 * (amount + 255.0)) / (255.0 * (259.0 - amount));
    buf.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            p[i] = clamp_u8(f * (p[i] as f32 - 128.0) + 128.0);
        }
    });
}

pub fn invert_buf(buf: &mut [u8]) {
    buf.par_chunks_exact_mut(4).for_each(|p| {
        p[0] = 255 - p[0];
        p[1] = 255 - p[1];
        p[2] = 255 - p[2];
    });
}

pub fn grayscale_buf(buf: &mut [u8]) {
    buf.par_chunks_exact_mut(4).for_each(|p| {
        let l = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32).round() as u8;
        p[0] = l;
        p[1] = l;
        p[2] = l;
    });
}

pub fn threshold_buf(buf: &mut [u8], t: u8) {
    buf.par_chunks_exact_mut(4).for_each(|p| {
        let l = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        let v = if l >= t as f32 { 255 } else { 0 };
        p[0] = v;
        p[1] = v;
        p[2] = v;
    });
}

pub fn posterize_buf(buf: &mut [u8], levels: u8) {
    let lv = levels.max(2) as f32;
    buf.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            let v = p[i] as f32 / 255.0;
            p[i] = ((v * (lv - 1.0)).round() / (lv - 1.0) * 255.0).round() as u8;
        }
    });
}

pub fn exposure_buf(buf: &mut [u8], ev: f32) {
    let m = 2f32.powf(ev);
    buf.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            p[i] = clamp_u8(p[i] as f32 * m);
        }
    });
}

pub fn vibrance_buf(buf: &mut [u8], amount: f32) {
    // amount -100..100
    let k = amount / 100.0;
    buf.par_chunks_exact_mut(4).for_each(|p| {
        let mx = p[0].max(p[1]).max(p[2]) as f32;
        let mn = p[0].min(p[1]).min(p[2]) as f32;
        let sat = if mx == 0.0 { 0.0 } else { (mx - mn) / mx };
        let boost = k * (1.0 - sat);
        let avg = (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0;
        for i in 0..3 {
            p[i] = clamp_u8(p[i] as f32 + (p[i] as f32 - avg) * boost);
        }
    });
}

pub fn hue_saturation_buf(buf: &mut [u8], hue_deg: f32, sat_mult: f32, lightness: f32) {
    // lightness -100..100
    let l_shift = lightness / 100.0 * 0.5;
    let h_shift = hue_deg / 360.0;
    buf.par_chunks_exact_mut(4).for_each(|p| {
        let (mut h, mut s, l) = rgb_to_hsl(p[0], p[1], p[2]);
        h = (h + h_shift).rem_euclid(1.0);
        s = (s * sat_mult).clamp(0.0, 1.0);
        let l = (l + l_shift).clamp(0.0, 1.0);
        let (r, g, b) = hsl_to_rgb(h, s, l);
        p[0] = r;
        p[1] = g;
        p[2] = b;
    });
}

pub fn levels_buf(buf: &mut [u8], in_lo: u8, in_hi: u8, gamma: f32) {
    let lo = in_lo as f32;
    let hi = in_hi.max(in_lo + 1) as f32;
    let g = gamma.clamp(0.1, 5.0);
    buf.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            let mut v = ((p[i] as f32 - lo) / (hi - lo)).clamp(0.0, 1.0);
            v = v.powf(1.0 / g);
            p[i] = (v * 255.0).round() as u8;
        }
    });
}

pub fn color_balance_buf(buf: &mut [u8], dr: i16, dg: i16, db: i16) {
    buf.par_chunks_exact_mut(4).for_each(|p| {
        p[0] = (p[0] as i16 + dr).clamp(0, 255) as u8;
        p[1] = (p[1] as i16 + dg).clamp(0, 255) as u8;
        p[2] = (p[2] as i16 + db).clamp(0, 255) as u8;
    });
}

pub fn auto_contrast_buf(buf: &[u8]) -> Option<(u8, u8)> {
    let (mut mn, mut mx) = (255u8, 0u8);
    for p in buf.chunks_exact(4) {
        let l = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8;
        mn = mn.min(l);
        mx = mx.max(l);
    }
    if mx > mn { Some((mn, mx)) } else { None }
}

/// Build a 256-entry LUT from control points (linear interpolation).
pub fn curves_lut(points: &[(u8, u8)]) -> [u8; 256] {
    let mut pts: Vec<(u8, u8)> = points.to_vec();
    pts.sort_by_key(|p| p.0);
    pts.dedup_by_key(|p| p.0);
    if pts.is_empty() {
        pts.push((0, 0));
        pts.push((255, 255));
    }
    let mut lut = [0u8; 256];
    let mut seg = 0;
    for (i, slot) in lut.iter_mut().enumerate() {
        let x = i as u8;
        while seg + 1 < pts.len() && x > pts[seg + 1].0 {
            seg += 1;
        }
        let (x0, y0) = pts[seg];
        if x <= x0 || seg + 1 >= pts.len() {
            *slot = y0;
        } else {
            let (x1, y1) = pts[seg + 1];
            if x1 <= x0 {
                *slot = y1;
            } else {
                let t = (x - x0) as f32 / (x1 - x0) as f32;
                *slot = (y0 as f32 + t * (y1 as f32 - y0 as f32)).round() as u8;
            }
        }
    }
    lut
}

pub fn curves_buf(buf: &mut [u8], lut: &[u8; 256]) {
    buf.par_chunks_exact_mut(4).for_each(|p| {
        p[0] = lut[p[0] as usize];
        p[1] = lut[p[1] as usize];
        p[2] = lut[p[2] as usize];
    });
}

pub fn black_white_buf(buf: &mut [u8], r: f32, g: f32, b: f32) {
    buf.par_chunks_exact_mut(4).for_each(|p| {
        let l = clamp_u8(r * p[0] as f32 + g * p[1] as f32 + b * p[2] as f32);
        p[0] = l;
        p[1] = l;
        p[2] = l;
    });
}

pub fn photo_filter_buf(buf: &mut [u8], color: [u8; 3], density: f32) {
    let d = density.clamp(0.0, 1.0);
    buf.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            p[i] = clamp_u8(p[i] as f32 * (1.0 - d) + color[i] as f32 * d);
        }
    });
}

pub fn channel_mixer_buf(buf: &mut [u8], r: [f32; 3], g: [f32; 3], b: [f32; 3]) {
    buf.par_chunks_exact_mut(4).for_each(|p| {
        let (pr, pg, pb) = (p[0] as f32, p[1] as f32, p[2] as f32);
        p[0] = clamp_u8(r[0] * pr + r[1] * pg + r[2] * pb);
        p[1] = clamp_u8(g[0] * pr + g[1] * pg + g[2] * pb);
        p[2] = clamp_u8(b[0] * pr + b[1] * pg + b[2] * pb);
    });
}

pub fn gradient_map_buf(buf: &mut [u8], dark: [u8; 3], light: [u8; 3]) {
    buf.par_chunks_exact_mut(4).for_each(|p| {
        let l = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0;
        for i in 0..3 {
            p[i] = clamp_u8(dark[i] as f32 * (1.0 - l) + light[i] as f32 * l);
        }
    });
}

pub fn shadows_highlights_buf(buf: &mut [u8], shadows: f32, highlights: f32) {
    // amounts 0..100
    let s = (shadows / 100.0).clamp(0.0, 1.0) * 0.6;
    let h = (highlights / 100.0).clamp(0.0, 1.0) * 0.6;
    buf.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            let v = p[i] as f32 / 255.0;
            let v = v + s * (1.0 - v) * (1.0 - v) - h * v * v;
            p[i] = clamp_u8(v * 255.0);
        }
    });
}

/// Evaluate any adjustment on a raw RGBA8 buffer (alpha untouched).
pub fn apply_adjustment(adj: &Adjustment, buf: &mut [u8]) {
    match adj {
        Adjustment::BrightnessContrast { brightness, contrast } => {
            if *brightness != 0 {
                brightness_buf(buf, *brightness);
            }
            if contrast.abs() > 0.001 {
                contrast_buf(buf, *contrast);
            }
        }
        Adjustment::Levels { in_lo, in_hi, gamma } => levels_buf(buf, *in_lo, *in_hi, *gamma),
        Adjustment::Curves { points } => {
            let lut = curves_lut(points);
            curves_buf(buf, &lut);
        }
        Adjustment::HueSaturation { hue_deg, sat_mult, lightness } => {
            hue_saturation_buf(buf, *hue_deg, *sat_mult, *lightness)
        }
        Adjustment::Vibrance { amount } => vibrance_buf(buf, *amount),
        Adjustment::Exposure { ev } => exposure_buf(buf, *ev),
        Adjustment::ColorBalance { dr, dg, db } => color_balance_buf(buf, *dr, *dg, *db),
        Adjustment::BlackWhite { r, g, b } => black_white_buf(buf, *r, *g, *b),
        Adjustment::PhotoFilter { color, density } => photo_filter_buf(buf, *color, *density),
        Adjustment::ChannelMixer { r, g, b } => channel_mixer_buf(buf, *r, *g, *b),
        Adjustment::GradientMap { dark, light } => gradient_map_buf(buf, *dark, *light),
        Adjustment::ShadowsHighlights { shadows, highlights } => {
            shadows_highlights_buf(buf, *shadows, *highlights)
        }
        Adjustment::Threshold { t } => threshold_buf(buf, *t),
        Adjustment::Posterize { levels } => posterize_buf(buf, *levels),
        Adjustment::Invert => invert_buf(buf),
        Adjustment::Grayscale => grayscale_buf(buf),
        Adjustment::AutoContrast => {
            if let Some((mn, mx)) = auto_contrast_buf(buf) {
                levels_buf(buf, mn, mx, 1.0);
            }
        }
    }
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) / 2.0;
    if (mx - mn).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return (v, v, v);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let conv = |t: f32| {
        let t = t.rem_euclid(1.0);
        let c = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (c * 255.0).round().clamp(0.0, 255.0) as u8
    };
    (conv(h + 1.0 / 3.0), conv(h), conv(h - 1.0 / 3.0))
}

// ---------------------------------------------------------------------------
// Document-level adjustment wrappers (active pixel layer)
// ---------------------------------------------------------------------------

fn active_pixels(doc: &mut Document) -> &mut Vec<u8> {
    &mut doc.active_layer_mut().pixels
}

pub fn brightness(doc: &mut Document, delta: i16) {
    brightness_buf(active_pixels(doc), delta);
}
pub fn contrast(doc: &mut Document, amount: f32) {
    contrast_buf(active_pixels(doc), amount);
}
pub fn invert(doc: &mut Document) {
    invert_buf(active_pixels(doc));
}
pub fn grayscale(doc: &mut Document) {
    grayscale_buf(active_pixels(doc));
}
pub fn threshold(doc: &mut Document, t: u8) {
    threshold_buf(active_pixels(doc), t);
}
pub fn posterize(doc: &mut Document, levels: u8) {
    posterize_buf(active_pixels(doc), levels);
}
pub fn exposure(doc: &mut Document, ev: f32) {
    exposure_buf(active_pixels(doc), ev);
}
pub fn vibrance(doc: &mut Document, amount: f32) {
    vibrance_buf(active_pixels(doc), amount);
}
pub fn hue_saturation(doc: &mut Document, hue_deg: f32, sat_mult: f32) {
    hue_saturation_buf(active_pixels(doc), hue_deg, sat_mult, 0.0);
}
pub fn levels(doc: &mut Document, in_lo: u8, in_hi: u8, gamma: f32) {
    levels_buf(active_pixels(doc), in_lo, in_hi, gamma);
}
pub fn color_balance(doc: &mut Document, dr: i16, dg: i16, db: i16) {
    color_balance_buf(active_pixels(doc), dr, dg, db);
}
pub fn auto_contrast(doc: &mut Document) {
    if let Some((mn, mx)) = auto_contrast_buf(&doc.active_layer().pixels.clone()) {
        levels(doc, mn, mx, 1.0);
    }
}
pub fn curves(doc: &mut Document, points: &[(u8, u8)]) {
    let lut = curves_lut(points);
    curves_buf(active_pixels(doc), &lut);
}
pub fn black_white(doc: &mut Document, r: f32, g: f32, b: f32) {
    black_white_buf(active_pixels(doc), r, g, b);
}
pub fn photo_filter(doc: &mut Document, color: [u8; 3], density: f32) {
    photo_filter_buf(active_pixels(doc), color, density);
}
pub fn channel_mixer(doc: &mut Document, r: [f32; 3], g: [f32; 3], b: [f32; 3]) {
    channel_mixer_buf(active_pixels(doc), r, g, b);
}
pub fn gradient_map(doc: &mut Document, dark: [u8; 3], light: [u8; 3]) {
    gradient_map_buf(active_pixels(doc), dark, light);
}
pub fn shadows_highlights(doc: &mut Document, shadows: f32, highlights: f32) {
    shadows_highlights_buf(active_pixels(doc), shadows, highlights);
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

fn box_blur_pass(src: &[u8], dst: &mut [u8], w: usize, h: usize, radius: usize, horizontal: bool) {
    let r = radius as isize;
    let div = (2 * radius + 1) as u32;
    if horizontal {
        dst.par_chunks_exact_mut(w * 4).enumerate().for_each(|(y, row)| {
            let base = y * w * 4;
            for x in 0..w {
                let mut acc = [0u32; 4];
                for k in -r..=r {
                    let xx = (x as isize + k).clamp(0, w as isize - 1) as usize;
                    let o = base + xx * 4;
                    for c in 0..4 {
                        acc[c] += src[o + c] as u32;
                    }
                }
                for c in 0..4 {
                    row[x * 4 + c] = (acc[c] / div) as u8;
                }
            }
        });
    } else {
        let mut tmp = vec![0u8; src.len()];
        tmp.par_chunks_exact_mut(w * 4).enumerate().for_each(|(y, row)| {
            for x in 0..w {
                let mut acc = [0u32; 4];
                for k in -r..=r {
                    let yy = (y as isize + k).clamp(0, h as isize - 1) as usize;
                    let o = yy * w * 4 + x * 4;
                    for c in 0..4 {
                        acc[c] += src[o + c] as u32;
                    }
                }
                for c in 0..4 {
                    row[x * 4 + c] = (acc[c] / div) as u8;
                }
            }
        });
        dst.copy_from_slice(&tmp);
    }
}

/// Separable box blur of a single channel (masks, effect alphas).
pub fn blur_channel(src: &[u8], w: usize, h: usize, radius: usize) -> Vec<u8> {
    if radius == 0 || src.len() != w * h {
        return src.to_vec();
    }
    let r = radius as isize;
    let div = (2 * radius + 1) as u32;
    let mut a = src.to_vec();
    let mut b = vec![0u8; a.len()];
    for _ in 0..2 {
        // horizontal
        b.par_chunks_exact_mut(w).enumerate().for_each(|(y, row)| {
            for x in 0..w {
                let mut acc = 0u32;
                for k in -r..=r {
                    let xx = (x as isize + k).clamp(0, w as isize - 1) as usize;
                    acc += a[y * w + xx] as u32;
                }
                row[x] = (acc / div) as u8;
            }
        });
        // vertical
        a.par_chunks_exact_mut(w).enumerate().for_each(|(y, row)| {
            for x in 0..w {
                let mut acc = 0u32;
                for k in -r..=r {
                    let yy = (y as isize + k).clamp(0, h as isize - 1) as usize;
                    acc += b[yy * w + x] as u32;
                }
                row[x] = (acc / div) as u8;
            }
        });
    }
    a
}

/// Fast gaussian approximation: 3x separable box blur. Radius clamped for speed.
pub fn gaussian_blur(doc: &mut Document, radius: u32) {
    let r = (radius as usize).clamp(1, 64);
    let (w, h) = (doc.width as usize, doc.height as usize);
    gaussian_blur_buf(active_pixels(doc), w, h, r);
}

pub fn gaussian_blur_buf(buf: &mut [u8], w: usize, h: usize, r: usize) {
    let mut a = buf.to_vec();
    let mut b = vec![0u8; a.len()];
    for _ in 0..3 {
        box_blur_pass(&a, &mut b, w, h, r, true);
        box_blur_pass(&b, &mut a, w, h, r, false);
    }
    buf.copy_from_slice(&a);
}

pub fn sharpen(doc: &mut Document, amount: f32) {
    // unsharp mask: out = orig + amount*(orig - blurred)
    let orig = doc.active_layer().pixels.clone();
    let (w, h) = (doc.width as usize, doc.height as usize);
    let mut blurred = orig.clone();
    let mut tmp = vec![0u8; orig.len()];
    box_blur_pass(&orig, &mut tmp, w, h, 1, true);
    box_blur_pass(&tmp, &mut blurred, w, h, 1, false);
    let k = amount.clamp(0.0, 5.0);
    let px = active_pixels(doc);
    px.par_chunks_exact_mut(4)
        .enumerate()
        .for_each(|(i, p)| {
            let o = i * 4;
            for c in 0..3 {
                let v = orig[o + c] as f32 + k * (orig[o + c] as f32 - blurred[o + c] as f32);
                p[c] = clamp_u8(v);
            }
        });
}

pub fn edge_detect(doc: &mut Document) {
    let src = doc.active_layer().pixels.clone();
    let (w, h) = (doc.width as usize, doc.height as usize);
    let out = active_pixels(doc);
    out.par_chunks_exact_mut(4).enumerate().for_each(|(i, p)| {
        let x = i % w;
        let y = i / w;
        let at = |xx: isize, yy: isize| -> f32 {
            let xx = xx.clamp(0, w as isize - 1) as usize;
            let yy = yy.clamp(0, h as isize - 1) as usize;
            let o = (yy * w + xx) * 4;
            0.299 * src[o] as f32 + 0.587 * src[o + 1] as f32 + 0.114 * src[o + 2] as f32
        };
        let gx = -at(x as isize - 1, y as isize - 1) - 2.0 * at(x as isize - 1, y as isize)
            + at(x as isize + 1, y as isize - 1)
            + 2.0 * at(x as isize + 1, y as isize)
            + at(x as isize + 1, y as isize + 1);
        let gy = -at(x as isize - 1, y as isize - 1) - 2.0 * at(x as isize, y as isize - 1)
            - at(x as isize + 1, y as isize - 1)
            + at(x as isize - 1, y as isize + 1)
            + 2.0 * at(x as isize, y as isize + 1)
            + at(x as isize + 1, y as isize + 1);
        let v = (gx * gx + gy * gy).sqrt().clamp(0.0, 255.0) as u8;
        p[0] = v;
        p[1] = v;
        p[2] = v;
    });
}

pub fn emboss(doc: &mut Document) {
    let src = doc.active_layer().pixels.clone();
    let (w, h) = (doc.width as usize, doc.height as usize);
    let out = active_pixels(doc);
    out.par_chunks_exact_mut(4).enumerate().for_each(|(i, p)| {
        let x = i % w;
        let y = i / w;
        let at = |xx: isize, yy: isize| -> f32 {
            let xx = xx.clamp(0, w as isize - 1) as usize;
            let yy = yy.clamp(0, h as isize - 1) as usize;
            let o = (yy * w + xx) * 4;
            (src[o] as f32 + src[o + 1] as f32 + src[o + 2] as f32) / 3.0
        };
        let v = (128.0 + at(x as isize, y as isize) - at(x as isize - 1, y as isize - 1)).clamp(0.0, 255.0) as u8;
        p[0] = v;
        p[1] = v;
        p[2] = v;
    });
}

pub fn pixelate(doc: &mut Document, block: u32) {
    let b = block.max(2) as usize;
    let (w, h) = (doc.width as usize, doc.height as usize);
    let mut src = doc.active_layer().pixels.clone();
    for by in (0..h).step_by(b) {
        for bx in (0..w).step_by(b) {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for y in by..(by + b).min(h) {
                for x in bx..(bx + b).min(w) {
                    let o = (y * w + x) * 4;
                    for c in 0..4 {
                        acc[c] += src[o + c] as u32;
                    }
                    n += 1;
                }
            }
            let avg = [acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n];
            for y in by..(by + b).min(h) {
                for x in bx..(bx + b).min(w) {
                    let o = (y * w + x) * 4;
                    for c in 0..4 {
                        src[o + c] = avg[c] as u8;
                    }
                }
            }
        }
    }
    doc.active_layer_mut().pixels = src;
}

pub fn add_noise(doc: &mut Document, amount: u8) {
    // fast deterministic xorshift noise (no rand dep)
    let px = active_pixels(doc);
    let mut seed: u64 = 0x9E3779B97F4A7C15;
    for p in px.chunks_exact_mut(4) {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let n = ((seed >> 33) as i16 % (amount as i16 + 1)) - amount as i16 / 2;
        for i in 0..3 {
            p[i] = (p[i] as i16 + n).clamp(0, 255) as u8;
        }
    }
}

pub fn vignette(doc: &mut Document, strength: f32) {
    let (w, h) = (doc.width as f32, doc.height as f32);
    let s = strength.clamp(0.0, 1.0);
    let dw = doc.width;
    let px = active_pixels(doc);
    let cx = w / 2.0;
    let cy = h / 2.0;
    let maxd = (cx * cx + cy * cy).sqrt();
    px.par_chunks_exact_mut(4).enumerate().for_each(|(i, p)| {
        let x = (i as u32 % dw) as f32;
        let y = (i as u32 / dw) as f32;
        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / maxd;
        let m = 1.0 - s * d * d;
        for c in 0..3 {
            p[c] = clamp_u8(p[c] as f32 * m);
        }
    });
}

/// Motion blur along an angle (degrees) with a given streak length.
pub fn motion_blur(doc: &mut Document, angle_deg: f32, length: u32) {
    let (w, h) = (doc.width as usize, doc.height as usize);
    let src = doc.active_layer().pixels.clone();
    let rad = angle_deg.to_radians();
    let (dx, dy) = (rad.cos(), rad.sin());
    let len = (length as usize).clamp(2, 64);
    let half = len as f32 / 2.0;
    let out = active_pixels(doc);
    out.par_chunks_exact_mut(4).enumerate().for_each(|(i, p)| {
        let x = i % w;
        let y = i / w;
        let mut acc = [0u32; 4];
        for k in 0..len {
            let t = k as f32 - half;
            let xx = (x as f32 + dx * t).round().clamp(0.0, w as f32 - 1.0) as usize;
            let yy = (y as f32 + dy * t).round().clamp(0.0, h as f32 - 1.0) as usize;
            let o = (yy * w + xx) * 4;
            for c in 0..4 {
                acc[c] += src[o + c] as u32;
            }
        }
        for c in 0..4 {
            p[c] = (acc[c] / len as u32) as u8;
        }
    });
}

/// Zoom (radial) blur. Amount 0..100.
pub fn radial_blur(doc: &mut Document, amount: f32) {
    let (w, h) = (doc.width as usize, doc.height as usize);
    let src = doc.active_layer().pixels.clone();
    let k = (amount / 100.0).clamp(0.0, 1.0) * 0.25;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let steps = 12;
    let out = active_pixels(doc);
    out.par_chunks_exact_mut(4).enumerate().for_each(|(i, p)| {
        let x = i % w;
        let y = i / w;
        let mut acc = [0u32; 4];
        for s in 0..steps {
            let t = 1.0 + (s as f32 / steps as f32 - 0.5) * k * 2.0;
            let xx = (cx + (x as f32 - cx) * t).round().clamp(0.0, w as f32 - 1.0) as usize;
            let yy = (cy + (y as f32 - cy) * t).round().clamp(0.0, h as f32 - 1.0) as usize;
            let o = (yy * w + xx) * 4;
            for c in 0..4 {
                acc[c] += src[o + c] as u32;
            }
        }
        for c in 0..4 {
            p[c] = (acc[c] / steps) as u8;
        }
    });
}

/// Median filter (dust removal / stylize). Radius clamped to 1..10.
pub fn median(doc: &mut Document, radius: u32) {
    let r = (radius as usize).clamp(1, 10) as isize;
    let (w, h) = (doc.width as usize, doc.height as usize);
    let src = doc.active_layer().pixels.clone();
    let out = active_pixels(doc);
    out.par_chunks_exact_mut(4).enumerate().for_each(|(i, p)| {
        let x = (i % w) as isize;
        let y = (i / w) as isize;
        let mut rs: Vec<u8> = Vec::with_capacity(((2 * r + 1) * (2 * r + 1)) as usize);
        let mut gs = Vec::with_capacity(rs.capacity());
        let mut bs = Vec::with_capacity(rs.capacity());
        for dy in -r..=r {
            for dx in -r..=r {
                let xx = (x + dx).clamp(0, w as isize - 1) as usize;
                let yy = (y + dy).clamp(0, h as isize - 1) as usize;
                let o = (yy * w + xx) * 4;
                rs.push(src[o]);
                gs.push(src[o + 1]);
                bs.push(src[o + 2]);
            }
        }
        rs.sort_unstable();
        gs.sort_unstable();
        bs.sort_unstable();
        let m = rs.len() / 2;
        p[0] = rs[m];
        p[1] = gs[m];
        p[2] = bs[m];
    });
}

/// High Pass: keeps edges, drops low frequencies (great with Overlay blend).
pub fn high_pass(doc: &mut Document, radius: u32) {
    let r = (radius as usize).clamp(1, 64);
    let (w, h) = (doc.width as usize, doc.height as usize);
    let orig = doc.active_layer().pixels.clone();
    let mut blurred = orig.clone();
    let mut tmp = vec![0u8; orig.len()];
    box_blur_pass(&orig, &mut tmp, w, h, r, true);
    box_blur_pass(&tmp, &mut blurred, w, h, r, false);
    let out = active_pixels(doc);
    out.par_chunks_exact_mut(4)
        .enumerate()
        .for_each(|(i, p)| {
            let o = i * 4;
            for c in 0..3 {
                p[c] = clamp_u8(128.0 + orig[o + c] as f32 - blurred[o + c] as f32);
            }
            p[3] = orig[o + 3];
        });
}

// ---------------------------------------------------------------------------
// Selections / masks
// ---------------------------------------------------------------------------

/// Magic Wand: flood-fill selection from a seed point.
/// `src` is a flattened RGBA8 image; tolerance is 0..255 RGB distance.
pub fn magic_wand(src: &[u8], w: usize, h: usize, sx: usize, sy: usize, tolerance: u8) -> Vec<u8> {
    let mut mask = vec![0u8; w * h];
    if sx >= w || sy >= h {
        return mask;
    }
    let seed = (sy * w + sx) * 4;
    let (sr, sg, sb) = (src[seed] as f32, src[seed + 1] as f32, src[seed + 2] as f32);
    let tol = tolerance as f32;
    let mut stack = vec![(sx, sy)];
    mask[sy * w + sx] = 255;
    while let Some((x, y)) = stack.pop() {
        for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
            if nx >= w || ny >= h || mask[ny * w + nx] != 0 {
                continue;
            }
            let o = (ny * w + nx) * 4;
            let d = ((src[o] as f32 - sr).powi(2)
                + (src[o + 1] as f32 - sg).powi(2)
                + (src[o + 2] as f32 - sb).powi(2))
            .sqrt();
            if d <= tol {
                mask[ny * w + nx] = 255;
                stack.push((nx, ny));
            }
        }
    }
    mask
}

pub fn invert_mask(mask: &mut [u8]) {
    for m in mask.iter_mut() {
        *m = 255 - *m;
    }
}

/// Bounding box of the selected area, as (x0, y0, x1, y1) with x1/y1 exclusive.
pub fn mask_bounding_box(mask: &[u8], w: usize, h: usize) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] >= 128 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 == 0 {
        None
    } else {
        Some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
    }
}

// ---------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------

pub fn flip_horizontal(doc: &mut Document) {
    let w = doc.width as usize;
    let px = active_pixels(doc);
    for row in px.chunks_exact_mut(w * 4) {
        for x in 0..w / 2 {
            for c in 0..4 {
                let a = x * 4 + c;
                let b = (w - 1 - x) * 4 + c;
                row.swap(a, b);
            }
        }
    }
}

pub fn flip_vertical(doc: &mut Document) {
    let w = doc.width as usize;
    let h = doc.height as usize;
    let px = active_pixels(doc);
    for y in 0..h / 2 {
        for x in 0..w * 4 {
            let a = y * w * 4 + x;
            let b = (h - 1 - y) * w * 4 + x;
            px.swap(a, b);
        }
    }
}

pub fn rotate90_cw(doc: &mut Document) {
    let (w, h) = (doc.width, doc.height);
    for layer in &mut doc.layers {
        if layer.pixels.is_empty() {
            std::mem::swap(&mut layer.width, &mut layer.height);
            continue;
        }
        let mut out = vec![0u8; layer.pixels.len()];
        let (sw, sh) = (layer.width as usize, layer.height as usize);
        for y in 0..sh {
            for x in 0..sw {
                let s = (y * sw + x) * 4;
                let nx = sh - 1 - y;
                let ny = x;
                let d = (ny * sh + nx) * 4;
                out[d..d + 4].copy_from_slice(&layer.pixels[s..s + 4]);
            }
        }
        layer.pixels = out;
        layer.width = h;
        layer.height = w;
    }
    doc.width = h;
    doc.height = w;
}

fn bilinear(src: &[u8], w: i32, h: i32, x: f32, y: f32) -> [u8; 4] {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |xx: i32, yy: i32| -> [f32; 4] {
        let xx = xx.clamp(0, w - 1) as usize;
        let yy = yy.clamp(0, h - 1) as usize;
        let o = (yy * w as usize + xx) * 4;
        [src[o] as f32, src[o + 1] as f32, src[o + 2] as f32, src[o + 3] as f32]
    };
    if x < 0.0 || y < 0.0 || x > w as f32 - 1.0 || y > h as f32 - 1.0 {
        return [0, 0, 0, 0];
    }
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    let mut out = [0u8; 4];
    for i in 0..4 {
        let v = a[i] * (1.0 - fx) * (1.0 - fy) + b[i] * fx * (1.0 - fy) + c[i] * (1.0 - fx) * fy + d[i] * fx * fy;
        out[i] = v.round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// Rotate the active layer content by any angle (same canvas, bilinear).
pub fn rotate_arbitrary(doc: &mut Document, degrees: f32) {
    let (w, h) = (doc.width as i32, doc.height as i32);
    let src = doc.active_layer().pixels.clone();
    if src.is_empty() {
        return;
    }
    let rad = degrees.to_radians();
    let (s, c) = rad.sin_cos();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let out = active_pixels(doc);
    out.par_chunks_exact_mut(w as usize * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let sx = c * dx + s * dy + cx;
                let sy = -s * dx + c * dy + cy;
                row[x as usize * 4..x as usize * 4 + 4]
                    .copy_from_slice(&bilinear(&src, w, h, sx, sy));
            }
        });
}

/// Scale the active layer content by percent (same canvas, centered, bilinear).
pub fn scale_content(doc: &mut Document, sx_pct: f32, sy_pct: f32) {
    let (w, h) = (doc.width as i32, doc.height as i32);
    let src = doc.active_layer().pixels.clone();
    if src.is_empty() {
        return;
    }
    let (sx, sy) = ((sx_pct / 100.0).max(0.01), (sy_pct / 100.0).max(0.01));
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let out = active_pixels(doc);
    out.par_chunks_exact_mut(w as usize * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let px = cx + (x as f32 - cx) / sx;
                let py = cy + (y as f32 - cy) / sy;
                row[x as usize * 4..x as usize * 4 + 4]
                    .copy_from_slice(&bilinear(&src, w, h, px, py));
            }
        });
}

pub fn resize_document(doc: &mut Document, w: u32, h: u32) {
    let (w, h) = (w.max(1), h.max(1));
    for layer in &mut doc.layers {
        layer.resize_to(w, h);
    }
    doc.width = w;
    doc.height = h;
}

/// Crop the whole document to a rectangle (all layers).
pub fn crop_document(doc: &mut Document, x0: u32, y0: u32, x1: u32, y1: u32) {
    let (x0, y0) = (x0.min(doc.width), y0.min(doc.height));
    let (x1, y1) = (x1.clamp(x0 + 1, doc.width), y1.clamp(y0 + 1, doc.height));
    let (nw, nh) = (x1 - x0, y1 - y0);
    for layer in &mut doc.layers {
        if !layer.pixels.is_empty() {
            let mut out = vec![0u8; nw as usize * nh as usize * 4];
            for y in 0..nh {
                let s = (((y0 + y) * layer.width + x0) * 4) as usize;
                let d = (y * nw * 4) as usize;
                let len = nw as usize * 4;
                if s + len <= layer.pixels.len() {
                    out[d..d + len].copy_from_slice(&layer.pixels[s..s + len]);
                }
            }
            layer.pixels = out;
        }
        if let Some(m) = layer.mask.take() {
            let mut out = vec![0u8; nw as usize * nh as usize];
            for y in 0..nh {
                let s = ((y0 + y) * layer.width + x0) as usize;
                let d = (y * nw) as usize;
                let len = nw as usize;
                if s + len <= m.len() {
                    out[d..d + len].copy_from_slice(&m[s..s + len]);
                }
            }
            layer.mask = Some(out);
        }
        layer.width = nw;
        layer.height = nh;
    }
    doc.width = nw;
    doc.height = nh;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Document;

    #[test]
    fn adjust_roundtrip() {
        let mut d = Document::new(4, 4, [100, 100, 100, 255]);
        brightness(&mut d, 10);
        assert_eq!(d.active_layer().pixels[0], 110);
        invert(&mut d);
        assert_eq!(d.active_layer().pixels[0], 145);
    }

    #[test]
    fn curves_identity() {
        let lut = curves_lut(&[(0, 0), (255, 255)]);
        assert_eq!(lut[0], 0);
        assert_eq!(lut[128], 128);
        assert_eq!(lut[255], 255);
    }

    #[test]
    fn wand_selects_uniform() {
        let src = vec![100u8, 100, 100, 255].repeat(16);
        let m = magic_wand(&src, 4, 4, 0, 0, 10);
        assert!(m.iter().all(|v| *v == 255));
    }
}
