//! Resampling and geometry: image size, scaling and rotating layers, flips, quarter turns.
//! Resampling works on premultiplied colour (no dark fringes at transparent edges), with a
//! kernel widened when shrinking so nothing aliases.

use nori_core::Raster;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Filter {
    Nearest,
    Bilinear,
    #[default]
    Bicubic,
    Lanczos,
}

impl Filter {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "nearest" | "nearestneighbor" | "nearest-neighbor" => Ok(Filter::Nearest),
            "bilinear" | "linear" => Ok(Filter::Bilinear),
            "bicubic" | "cubic" => Ok(Filter::Bicubic),
            "lanczos" | "lanczos3" => Ok(Filter::Lanczos),
            _ => Err(format!("`{s}` isn't a resampling filter: nearest, bilinear, bicubic or lanczos")),
        }
    }

    fn support(self) -> f32 {
        match self {
            Filter::Nearest => 0.5,
            Filter::Bilinear => 1.0,
            Filter::Bicubic => 2.0,
            Filter::Lanczos => 3.0,
        }
    }

    fn weight(self, x: f32) -> f32 {
        let x = x.abs();
        match self {
            Filter::Nearest => {
                if x < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Filter::Bilinear => (1.0 - x).max(0.0),
            Filter::Bicubic => {
                // Catmull-Rom (a = -0.5): sharp without ringing much.
                let a = -0.5;
                if x < 1.0 {
                    (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0
                } else if x < 2.0 {
                    a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a
                } else {
                    0.0
                }
            }
            Filter::Lanczos => {
                if x < 1e-6 {
                    1.0
                } else if x < 3.0 {
                    let px = std::f32::consts::PI * x;
                    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
                } else {
                    0.0
                }
            }
        }
    }
}

/// Straight RGBA8 → premultiplied f32.
pub fn premultiply(rgba: &[u8]) -> Vec<[f32; 4]> {
    rgba.chunks_exact(4)
        .map(|p| {
            let a = p[3] as f32 / 255.0;
            [p[0] as f32 / 255.0 * a, p[1] as f32 / 255.0 * a, p[2] as f32 / 255.0 * a, a]
        })
        .collect()
}

/// Premultiplied f32 → straight RGBA8.
pub fn unpremultiply(px: &[[f32; 4]]) -> Vec<u8> {
    crate::composite::to_rgba8(px)
}

/// One pass of separable resampling along rows: `src` is `w`×`h`, the result `nw`×`h`.
fn resample_rows(src: &[[f32; 4]], w: usize, h: usize, nw: usize, f: Filter) -> Vec<[f32; 4]> {
    let scale = nw as f32 / w as f32;
    let widen = if scale < 1.0 { 1.0 / scale } else { 1.0 };
    let support = f.support() * widen;
    // Weights per output column, computed once.
    let taps: Vec<(usize, Vec<f32>)> = (0..nw)
        .map(|x| {
            let center = (x as f32 + 0.5) / scale;
            let lo = ((center - support).floor() as i64).max(0) as usize;
            let hi = ((center + support).ceil() as i64).min(w as i64) as usize;
            let mut ws: Vec<f32> = (lo..hi).map(|i| f.weight((i as f32 + 0.5 - center) / widen)).collect();
            let sum: f32 = ws.iter().sum();
            if sum.abs() > 1e-6 {
                ws.iter_mut().for_each(|v| *v /= sum);
            } else if !ws.is_empty() {
                let n = ws.len() as f32;
                ws.iter_mut().for_each(|v| *v = 1.0 / n);
            }
            (lo, ws)
        })
        .collect();
    let mut out = vec![[0f32; 4]; nw * h];
    out.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
        let s = &src[y * w..(y + 1) * w];
        for (x, o) in row.iter_mut().enumerate() {
            let (lo, ws) = &taps[x];
            let mut acc = [0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let p = s[lo + k];
                acc[0] += p[0] * wt;
                acc[1] += p[1] * wt;
                acc[2] += p[2] * wt;
                acc[3] += p[3] * wt;
            }
            let a = acc[3].clamp(0.0, 1.0);
            *o = [acc[0].clamp(0.0, a), acc[1].clamp(0.0, a), acc[2].clamp(0.0, a), a];
        }
    });
    out
}

fn transpose(src: &[[f32; 4]], w: usize, h: usize) -> Vec<[f32; 4]> {
    let mut out = vec![[0f32; 4]; w * h];
    out.par_chunks_mut(h).enumerate().for_each(|(x, col)| {
        for (y, o) in col.iter_mut().enumerate() {
            *o = src[y * w + x];
        }
    });
    out
}

/// Resamples premultiplied pixels `w`×`h` to `nw`×`nh`.
pub fn resample_premul(src: &[[f32; 4]], w: u32, h: u32, nw: u32, nh: u32, f: Filter) -> Vec<[f32; 4]> {
    let (w, h, nw, nh) = (w as usize, h as usize, nw.max(1) as usize, nh.max(1) as usize);
    let rows = if nw == w { src.to_vec() } else { resample_rows(src, w, h, nw, f) };
    if nh == h {
        return rows;
    }
    let t = transpose(&rows, nw, h);
    let cols = resample_rows(&t, h, nw, nh, f);
    transpose(&cols, nh, nw)
}

/// Resamples straight RGBA8 rows.
pub fn resample_rgba(rgba: &[u8], w: u32, h: u32, nw: u32, nh: u32, f: Filter) -> Vec<u8> {
    unpremultiply(&resample_premul(&premultiply(rgba), w, h, nw, nh, f))
}

/// A raster at a new size.
pub fn resize_raster(r: &Raster, nw: u32, nh: u32, f: Filter) -> Raster {
    Raster::from_rgba(nw, nh, &resample_rgba(&r.to_vec(), r.width(), r.height(), nw, nh, f))
}

/// Shrinks a picture to fit in `max`×`max` (never enlarges), averaging what it drops.
pub fn fit_rgba(rgba: &[u8], w: u32, h: u32, max: u32) -> (u32, u32, Vec<u8>) {
    let s = (max as f32 / w.max(h) as f32).min(1.0);
    if s >= 1.0 {
        return (w, h, rgba.to_vec());
    }
    let (nw, nh) = (((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1));
    (nw, nh, resample_rgba(rgba, w, h, nw, nh, Filter::Bilinear))
}

/// Mirrored left to right (`horizontal`) or top to bottom.
pub fn flip(r: &Raster, horizontal: bool) -> Raster {
    let (w, h) = (r.width() as usize, r.height() as usize);
    let src = r.to_vec();
    let mut out = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = if horizontal { (w - 1 - x, y) } else { (x, h - 1 - y) };
            out[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&src[(sy * w + sx) * 4..(sy * w + sx) * 4 + 4]);
        }
    }
    Raster::from_rgba(w as u32, h as u32, &out)
}

/// Turned clockwise by `quarters` × 90°.
pub fn rotate90(r: &Raster, quarters: i32) -> Raster {
    let q = quarters.rem_euclid(4);
    if q == 0 {
        return r.clone();
    }
    let (w, h) = (r.width() as usize, r.height() as usize);
    let src = r.to_vec();
    let (nw, nh) = if q % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match q {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            out[(ny * nw + nx) * 4..(ny * nw + nx) * 4 + 4].copy_from_slice(&src[(y * w + x) * 4..(y * w + x) * 4 + 4]);
        }
    }
    Raster::from_rgba(nw as u32, nh as u32, &out)
}

/// A raster placed at (x, y) on the canvas, scaled by (sx, sy) and turned by `degrees`
/// (clockwise) about the canvas point (cx, cy), then moved by (dx, dy). Returns the new pixels
/// and where their top-left corner is.
#[allow(clippy::too_many_arguments)]
pub fn affine(r: &Raster, x: i32, y: i32, sx: f32, sy: f32, degrees: f32, cx: f32, cy: f32, dx: f32, dy: f32, f: Filter) -> (Raster, i32, i32) {
    let (w, h) = (r.width() as f32, r.height() as f32);
    let (sin, cos) = degrees.to_radians().sin_cos();
    // Canvas → canvas: about (cx, cy): scale, rotate, then move.
    let fwd = |px: f32, py: f32| {
        let (ux, uy) = ((px - cx) * sx, (py - cy) * sy);
        (cx + ux * cos - uy * sin + dx, cy + ux * sin + uy * cos + dy)
    };
    let corners = [fwd(x as f32, y as f32), fwd(x as f32 + w, y as f32), fwd(x as f32, y as f32 + h), fwd(x as f32 + w, y as f32 + h)];
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (a, b) in corners {
        x0 = x0.min(a);
        y0 = y0.min(b);
        x1 = x1.max(a);
        y1 = y1.max(b);
    }
    let (ox, oy) = (x0.floor() as i32, y0.floor() as i32);
    let (nw, nh) = (((x1.ceil() as i32 - ox).max(1)) as u32, ((y1.ceil() as i32 - oy).max(1)) as u32);
    let (nw, nh) = (nw.min(nori_core::MAX_SIDE), nh.min(nori_core::MAX_SIDE));
    // Shrinking a lot: resample to size first, so the rotation samples a picture of the right
    // scale (no aliasing).
    let (src_raster, ssx, ssy) = if sx.abs() < 0.75 || sy.abs() < 0.75 {
        let tw = ((w * sx.abs()).round() as u32).max(1);
        let th = ((h * sy.abs()).round() as u32).max(1);
        (resize_raster(r, tw, th, f), sx.signum() * w / tw as f32 * sx.abs(), sy.signum() * h / th as f32 * sy.abs())
    } else {
        (r.clone(), sx, sy)
    };
    let (sw, sh) = (src_raster.width() as usize, src_raster.height() as usize);
    let src = premultiply(&src_raster.to_vec());
    // Scale factors of the (possibly pre-shrunk) source.
    let (kx, ky) = (w / sw as f32, h / sh as f32);
    let _ = (ssx, ssy);
    let inv = |qx: f32, qy: f32| {
        let (ux, uy) = (qx - dx - cx, qy - dy - cy);
        let (rx, ry) = (ux * cos + uy * sin, -ux * sin + uy * cos);
        (cx + rx / sx, cy + ry / sy)
    };
    let sample = |fx: f32, fy: f32| -> [f32; 4] {
        // In source pixels.
        let (u, v) = ((fx - x as f32) / kx - 0.5, (fy - y as f32) / ky - 0.5);
        let get = |ix: i64, iy: i64| -> [f32; 4] {
            if ix < 0 || iy < 0 || ix >= sw as i64 || iy >= sh as i64 { [0.0; 4] } else { src[iy as usize * sw + ix as usize] }
        };
        match f {
            Filter::Nearest => get(u.round() as i64, v.round() as i64),
            Filter::Bilinear | Filter::Bicubic | Filter::Lanczos => {
                let (x0, y0) = (u.floor(), v.floor());
                let (tx, ty) = (u - x0, v - y0);
                let (x0, y0) = (x0 as i64, y0 as i64);
                if f == Filter::Bilinear {
                    let mut o = [0f32; 4];
                    for (ix, iy, wt) in [(x0, y0, (1.0 - tx) * (1.0 - ty)), (x0 + 1, y0, tx * (1.0 - ty)), (x0, y0 + 1, (1.0 - tx) * ty), (x0 + 1, y0 + 1, tx * ty)] {
                        let p = get(ix, iy);
                        for c in 0..4 {
                            o[c] += p[c] * wt;
                        }
                    }
                    o
                } else {
                    let mut o = [0f32; 4];
                    let mut sum = 0.0;
                    for j in -1..=2 {
                        let wy = Filter::Bicubic.weight(j as f32 - ty);
                        for i in -1..=2 {
                            let wt = Filter::Bicubic.weight(i as f32 - tx) * wy;
                            let p = get(x0 + i, y0 + j);
                            for c in 0..4 {
                                o[c] += p[c] * wt;
                            }
                            sum += wt;
                        }
                    }
                    let a = (o[3] / sum).clamp(0.0, 1.0);
                    [(o[0] / sum).clamp(0.0, a), (o[1] / sum).clamp(0.0, a), (o[2] / sum).clamp(0.0, a), a]
                }
            }
        }
    };
    let mut out = vec![[0f32; 4]; (nw * nh) as usize];
    out.par_chunks_mut(nw as usize).enumerate().for_each(|(row, line)| {
        for (col, o) in line.iter_mut().enumerate() {
            let (fx, fy) = inv(ox as f32 + col as f32 + 0.5, oy as f32 + row as f32 + 0.5);
            *o = sample(fx, fy);
        }
    });
    (Raster::from_rgba(nw, nh, &unpremultiply(&out)), ox, oy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_flat_colour_and_halves() {
        let rgba: Vec<u8> = std::iter::repeat_n([200u8, 100, 50, 255], 40 * 30).flatten().collect();
        for f in [Filter::Nearest, Filter::Bilinear, Filter::Bicubic, Filter::Lanczos] {
            let out = resample_rgba(&rgba, 40, 30, 20, 15, f);
            assert_eq!(out.len(), 20 * 15 * 4);
            assert!(out.chunks_exact(4).all(|p| p == [200, 100, 50, 255]), "{f:?}");
            let up = resample_rgba(&rgba, 40, 30, 97, 61, f);
            assert!(up.chunks_exact(4).all(|p| (p[0] as i32 - 200).abs() <= 1), "{f:?} up");
        }
    }

    #[test]
    fn transparent_edges_dont_darken() {
        // Half red, half transparent black: the edge stays red, only less opaque.
        let mut rgba = vec![];
        for _y in 0..4 {
            for x in 0..8 {
                rgba.extend_from_slice(if x < 4 { &[255, 0, 0, 255] } else { &[0, 0, 0, 0] });
            }
        }
        let out = resample_rgba(&rgba, 8, 4, 3, 2, Filter::Bilinear);
        for p in out.chunks_exact(4).filter(|p| p[3] > 0) {
            assert!(p[0] >= 250 && p[1] == 0, "{p:?}");
        }
    }

    #[test]
    fn flips_and_turns() {
        let mut r = Raster::transparent(3, 2);
        r.set(0, 0, [1, 1, 1, 255]);
        let f = flip(&r, true);
        assert_eq!(f.get(2, 0), [1, 1, 1, 255]);
        let t = rotate90(&r, 1);
        assert_eq!((t.width(), t.height()), (2, 3));
        assert_eq!(t.get(1, 0), [1, 1, 1, 255]);
        let back = rotate90(&t, 3);
        assert!(back.same_pixels(&r));
    }

    #[test]
    fn affine_identity_and_turn() {
        let mut r = Raster::transparent(10, 10);
        r.set(2, 3, [255, 0, 0, 255]);
        let (same, x, y) = affine(&r, 5, 5, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, Filter::Nearest);
        assert_eq!((x, y), (5, 5));
        assert_eq!(same.get(2, 3), [255, 0, 0, 255]);
        let (turned, x, y) = affine(&r, 0, 0, 1.0, 1.0, 90.0, 5.0, 5.0, 0.0, 0.0, Filter::Nearest);
        assert_eq!((turned.width(), turned.height()), (10, 10));
        assert_eq!((x, y), (0, 0));
        // (2,3) turned 90° clockwise about (5,5): (6,2).
        assert_eq!(turned.get(6, 2), [255, 0, 0, 255]);
    }
}

/// The whole document at `k` times its size: pixels resampled, vectors and text scaled (so
/// they stay sharp), page sizes, margins and guides multiplied. Used to export at 2×, or at a
/// print size.
pub fn scale_document(doc: &nori_core::Document, k: f32, f: Filter) -> nori_core::Document {
    use nori_core::{Content, Layer, Mask};
    let k = k.clamp(0.01, 64.0);
    let mut d = doc.clone();
    let side = |v: u32| ((v as f32 * k).round() as u32).clamp(1, nori_core::MAX_SIDE);
    fn walk(list: &mut [Layer], k: f32, side: &dyn Fn(u32) -> u32, f: Filter) {
        for l in list {
            if let Some(m) = &mut l.mask {
                let (w, h) = (side(m.mask.width()), side(m.mask.height()));
                let grey: Vec<u8> = m.mask.to_vec().iter().flat_map(|v| [*v, *v, *v, 255]).collect();
                let out = resample_rgba(&grey, m.mask.width(), m.mask.height(), w, h, f);
                let fill = m.mask.fill();
                m.mask = Mask::from_vec(w, h, fill, &out.chunks_exact(4).map(|p| p[0]).collect::<Vec<u8>>());
            }
            match &mut l.content {
                Content::Raster { x, y, pixels } => {
                    *x = (*x as f32 * k).round() as i32;
                    *y = (*y as f32 * k).round() as i32;
                    let (w, h) = (side(pixels.width()), side(pixels.height()));
                    *pixels = resize_raster(pixels, w, h, f);
                }
                Content::Text { text } => {
                    text.x *= k;
                    text.y *= k;
                    text.size *= k;
                    text.letter_spacing *= k;
                    text.paragraph_spacing *= k;
                    if let Some(fr) = &mut text.frame {
                        fr[0] *= k;
                        fr[1] *= k;
                    }
                    for r in &mut text.runs {
                        if let Some(s) = &mut r.size {
                            *s *= k;
                        }
                    }
                }
                Content::Vector { shape } => {
                    let t = shape.transform;
                    shape.transform = [t[0] * k, t[1] * k, t[2] * k, t[3] * k, t[4] * k, t[5] * k];
                    if let Some(s) = &mut shape.stroke {
                        s.width *= k;
                        s.dash.iter_mut().for_each(|v| *v *= k);
                    }
                    let scale_paint = |p: &mut nori_core::Paint| match p {
                        nori_core::Paint::Linear { x1, y1, x2, y2, .. } => {
                            *x1 *= k;
                            *y1 *= k;
                            *x2 *= k;
                            *y2 *= k;
                        }
                        nori_core::Paint::Radial { cx, cy, r, .. } => {
                            *cx *= k;
                            *cy *= k;
                            *r *= k;
                        }
                        _ => {}
                    };
                    scale_paint(&mut shape.fill);
                    if let Some(s) = &mut shape.stroke {
                        scale_paint(&mut s.paint);
                    }
                }
                Content::Group { children, .. } => walk(children, k, side, f),
                Content::Fill { .. } | Content::Adjustment { .. } => {}
            }
        }
    }
    for p in d.pages.iter_mut().chain(d.masters.iter_mut()) {
        p.width = side(p.width);
        p.height = side(p.height);
        p.x *= k;
        p.y *= k;
        p.margins.top *= k;
        p.margins.right *= k;
        p.margins.bottom *= k;
        p.margins.left *= k;
        p.gutter *= k;
        p.bleed *= k;
        p.guides.iter_mut().for_each(|g| g.at *= k);
        walk(&mut p.layers, k, &side, f);
    }
    d.dpi *= k;
    d.selection = None;
    for s in &mut d.styles.paragraph {
        s.size *= k;
        s.letter_spacing *= k;
        s.paragraph_spacing *= k;
    }
    for s in &mut d.styles.character {
        if let Some(v) = &mut s.size {
            *v *= k;
        }
    }
    d
}
