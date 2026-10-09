//! PhotoStep ops: fast parallel adjustments, filters, transforms.
//! All pixel loops use rayon for multi-core speed.

use rayon::prelude::*;
use crate::core::Document;

#[inline]
fn clamp_u8(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

// ---------- adjustments (in-place on active layer) ----------

pub fn brightness(doc: &mut Document, delta: i16) {
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            p[i] = (p[i] as i16 + delta).clamp(0, 255) as u8;
        }
    });
}

pub fn contrast(doc: &mut Document, amount: f32) {
    // amount: -100..100
    let f = (259.0 * (amount + 255.0)) / (255.0 * (259.0 - amount));
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            p[i] = clamp_u8(f * (p[i] as f32 - 128.0) + 128.0);
        }
    });
}

pub fn invert(doc: &mut Document) {
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
        p[0] = 255 - p[0];
        p[1] = 255 - p[1];
        p[2] = 255 - p[2];
    });
}

pub fn grayscale(doc: &mut Document) {
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
        let l = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32).round() as u8;
        p[0] = l;
        p[1] = l;
        p[2] = l;
    });
}

pub fn threshold(doc: &mut Document, t: u8) {
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
        let l = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        let v = if l >= t as f32 { 255 } else { 0 };
        p[0] = v;
        p[1] = v;
        p[2] = v;
    });
}

pub fn posterize(doc: &mut Document, levels: u8) {
    let lv = levels.max(2) as f32;
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            let v = p[i] as f32 / 255.0;
            p[i] = ((v * (lv - 1.0)).round() / (lv - 1.0) * 255.0).round() as u8;
        }
    });
}

pub fn exposure(doc: &mut Document, ev: f32) {
    let m = 2f32.powf(ev);
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            p[i] = clamp_u8(p[i] as f32 * m);
        }
    });
}

pub fn vibrance(doc: &mut Document, amount: f32) {
    // amount -100..100
    let k = amount / 100.0;
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
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

pub fn hue_saturation(doc: &mut Document, hue_deg: f32, sat_mult: f32) {
    let px = &mut doc.active_layer_mut().pixels;
    let h_shift = hue_deg / 360.0;
    px.par_chunks_exact_mut(4).for_each(|p| {
        let (mut h, mut s, l) = rgb_to_hsl(p[0], p[1], p[2]);
        h = (h + h_shift).rem_euclid(1.0);
        s = (s * sat_mult).clamp(0.0, 1.0);
        let (r, g, b) = hsl_to_rgb(h, s, l);
        p[0] = r;
        p[1] = g;
        p[2] = b;
    });
}

pub fn levels(doc: &mut Document, in_lo: u8, in_hi: u8, gamma: f32) {
    let lo = in_lo as f32;
    let hi = in_hi.max(in_lo + 1) as f32;
    let g = gamma.clamp(0.1, 5.0);
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
        for i in 0..3 {
            let mut v = ((p[i] as f32 - lo) / (hi - lo)).clamp(0.0, 1.0);
            v = v.powf(1.0 / g);
            p[i] = (v * 255.0).round() as u8;
        }
    });
}

pub fn color_balance(doc: &mut Document, dr: i16, dg: i16, db: i16) {
    let px = &mut doc.active_layer_mut().pixels;
    px.par_chunks_exact_mut(4).for_each(|p| {
        p[0] = (p[0] as i16 + dr).clamp(0, 255) as u8;
        p[1] = (p[1] as i16 + dg).clamp(0, 255) as u8;
        p[2] = (p[2] as i16 + db).clamp(0, 255) as u8;
    });
}

pub fn auto_contrast(doc: &mut Document) {
    let px = doc.active_layer().pixels.clone();
    let (mut mn, mut mx) = (255u8, 0u8);
    for p in px.chunks_exact(4) {
        let l = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8;
        mn = mn.min(l);
        mx = mx.max(l);
    }
    if mx > mn {
        levels(doc, mn, mx, 1.0);
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
        let mut t = t.rem_euclid(1.0);
        let _ = &mut t;
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

// ---------- filters ----------

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

/// Fast gaussian approximation: 3x separable box blur. Radius clamped for speed.
pub fn gaussian_blur(doc: &mut Document, radius: u32) {
    let r = (radius as usize).clamp(1, 64);
    let layer = doc.active_layer_mut();
    let (w, h) = (layer.width as usize, layer.height as usize);
    let mut a = layer.pixels.clone();
    let mut b = vec![0u8; a.len()];
    for _ in 0..3 {
        box_blur_pass(&a, &mut b, w, h, r, true);
        box_blur_pass(&b, &mut a, w, h, r, false);
    }
    layer.pixels = a;
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
    let px = &mut doc.active_layer_mut().pixels;
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
    let out = &mut doc.active_layer_mut().pixels;
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
            + at(x as isize + 1, y as isize - 1) * 1.0
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
    let out = &mut doc.active_layer_mut().pixels;
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
    let px = &mut doc.active_layer_mut().pixels;
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
    let px = &mut doc.active_layer_mut().pixels;
    let cx = w / 2.0;
    let cy = h / 2.0;
    let maxd = (cx * cx + cy * cy).sqrt();
    px.par_chunks_exact_mut(4).enumerate().for_each(|(i, p)| {
        let x = (i as u32 % doc.width) as f32;
        let y = (i as u32 / doc.width) as f32;
        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / maxd;
        let m = 1.0 - s * d * d;
        for c in 0..3 {
            p[c] = clamp_u8(p[c] as f32 * m);
        }
    });
}

// ---------- transforms ----------

pub fn flip_horizontal(doc: &mut Document) {
    let w = doc.width as usize;
    let px = &mut doc.active_layer_mut().pixels;
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
    let px = &mut doc.active_layer_mut().pixels;
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

pub fn resize_document(doc: &mut Document, w: u32, h: u32) {
    let (w, h) = (w.max(1), h.max(1));
    for layer in &mut doc.layers {
        layer.resize_to(w, h);
    }
    doc.width = w;
    doc.height = h;
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
}
