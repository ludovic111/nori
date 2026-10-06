//! The paint bucket, the magic wand and the gradient tool on pixels.

use nori_core::{Color, Mask, Raster, Rect};
use serde::{Deserialize, Serialize};

/// The pixels like the one at (x, y): within `tolerance` (0–255, the largest channel
/// difference, alpha included), touching it (`contiguous`) or anywhere. `rgba` is the picture
/// sampled (a layer's pixels or the whole page), `w`×`h`. Returns a mask over that picture.
pub fn region(rgba: &[u8], w: u32, h: u32, x: i32, y: i32, tolerance: u8, contiguous: bool) -> Mask {
    let mut m = Mask::filled(w, h, 0);
    if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
        return m;
    }
    let at = |px: u32, py: u32| -> &[u8] { &rgba[((py * w + px) * 4) as usize..((py * w + px) * 4 + 4) as usize] };
    let seed: [u8; 4] = at(x as u32, y as u32).try_into().unwrap_or([0; 4]);
    let like = |p: &[u8]| (0..4).all(|c| (p[c] as i16 - seed[c] as i16).unsigned_abs() <= tolerance as u16);
    let mut out = vec![0u8; (w * h) as usize];
    if contiguous {
        // Scanline flood fill.
        let mut stack = vec![(x as u32, y as u32)];
        while let Some((sx, sy)) = stack.pop() {
            if out[(sy * w + sx) as usize] != 0 || !like(at(sx, sy)) {
                continue;
            }
            let mut l = sx;
            while l > 0 && out[(sy * w + l - 1) as usize] == 0 && like(at(l - 1, sy)) {
                l -= 1;
            }
            let mut r = sx;
            while r + 1 < w && out[(sy * w + r + 1) as usize] == 0 && like(at(r + 1, sy)) {
                r += 1;
            }
            for px in l..=r {
                out[(sy * w + px) as usize] = 255;
                if sy > 0 && out[((sy - 1) * w + px) as usize] == 0 && like(at(px, sy - 1)) {
                    stack.push((px, sy - 1));
                }
                if sy + 1 < h && out[((sy + 1) * w + px) as usize] == 0 && like(at(px, sy + 1)) {
                    stack.push((px, sy + 1));
                }
            }
        }
    } else {
        for py in 0..h {
            for px in 0..w {
                if like(at(px, py)) {
                    out[(py * w + px) as usize] = 255;
                }
            }
        }
    }
    m.write_rect(Rect::new(0, 0, w, h), &out);
    m.compact();
    m
}

/// Lays `color` over a raster where `mask` (in the raster's own pixels) says, at `opacity`.
pub fn fill_mask(raster: &mut Raster, mask: &Mask, color: Color, opacity: f32) -> Rect {
    let Some(b) = mask.content_bounds().or_else(|| (mask.fill()[0] > 0).then(|| mask.bounds())) else { return Rect::default() };
    let b = b.intersect(&raster.bounds());
    if b.is_empty() {
        return b;
    }
    let mut px = raster.read_rect(b);
    let m = mask.read_rect(b);
    for (i, p) in px.chunks_exact_mut(4).enumerate() {
        let k = m[i] as f32 / 255.0 * opacity.clamp(0.0, 1.0) * color.a;
        if k <= 0.0 {
            continue;
        }
        over_straight(p, [color.r, color.g, color.b], k);
    }
    raster.write_rect(b, &px);
    b
}

/// Source-over of a straight colour at coverage `k` onto a straight RGBA8 pixel.
pub fn over_straight(p: &mut [u8], c: [f32; 3], k: f32) {
    let da = p[3] as f32 / 255.0;
    let oa = k + da * (1.0 - k);
    if oa <= 0.0 {
        return;
    }
    for ch in 0..3 {
        let d = p[ch] as f32 / 255.0;
        p[ch] = (((c[ch] * k + d * da * (1.0 - k)) / oa).clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    p[3] = (oa * 255.0).round() as u8;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
    Reflected,
}

impl GradientKind {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "linear" => Ok(Self::Linear),
            "radial" => Ok(Self::Radial),
            "reflected" => Ok(Self::Reflected),
            _ => Err(format!("`{s}` isn't a gradient kind: linear, radial or reflected")),
        }
    }
}

/// Draws a gradient from `a` (colour `from`) to `b` (colour `to`) onto a raster placed at
/// `origin`, inside `selection` (page pixels), at `opacity`.
#[allow(clippy::too_many_arguments)]
pub fn gradient(raster: &mut Raster, origin: (i32, i32), kind: GradientKind, a: [f32; 2], b: [f32; 2], from: Color, to: Color, opacity: f32, selection: Option<&Mask>) -> Rect {
    let area = raster.bounds();
    let mut px = raster.read_rect(area);
    let sel = selection.map(|m| m.read_rect(area.translate(origin.0, origin.1)));
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    let len = len2.sqrt();
    for (i, p) in px.chunks_exact_mut(4).enumerate() {
        let k = opacity * sel.as_ref().map_or(1.0, |s| s[i] as f32 / 255.0);
        if k <= 0.0 {
            continue;
        }
        let x = (i as u32 % area.w) as f32 + origin.0 as f32 + 0.5;
        let y = (i as u32 / area.w) as f32 + origin.1 as f32 + 0.5;
        let t = match kind {
            GradientKind::Linear => ((x - a[0]) * dx + (y - a[1]) * dy) / len2,
            GradientKind::Reflected => (((x - a[0]) * dx + (y - a[1]) * dy) / len2).abs(),
            GradientKind::Radial => ((x - a[0]).powi(2) + (y - a[1]).powi(2)).sqrt() / len,
        }
        .clamp(0.0, 1.0);
        let c = [from.r + (to.r - from.r) * t, from.g + (to.g - from.g) * t, from.b + (to.b - from.b) * t];
        let alpha = from.a + (to.a - from.a) * t;
        over_straight(p, c, k * alpha);
    }
    raster.write_rect(area, &px);
    area.translate(origin.0, origin.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_fill_stops_at_edges() {
        // A white picture with a black wall at x = 5.
        let (w, h) = (10u32, 4u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        for y in 0..h {
            let o = ((y * w + 5) * 4) as usize;
            rgba[o..o + 3].copy_from_slice(&[0, 0, 0]);
        }
        let m = region(&rgba, w, h, 1, 1, 10, true);
        assert_eq!(m.get(4, 3), [255]);
        assert_eq!(m.get(5, 0), [0]);
        assert_eq!(m.get(7, 0), [0]);
        let all = region(&rgba, w, h, 1, 1, 10, false);
        assert_eq!(all.get(7, 0), [255]);
        let mut r = Raster::from_rgba(w, h, &rgba);
        fill_mask(&mut r, &m, Color::rgb(1.0, 0.0, 0.0), 1.0);
        assert_eq!(r.get(0, 0), [255, 0, 0, 255]);
        assert_eq!(r.get(9, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn gradients_run_from_one_colour_to_the_other() {
        let mut r = Raster::transparent(100, 10);
        gradient(&mut r, (0, 0), GradientKind::Linear, [0.0, 5.0], [100.0, 5.0], Color::BLACK, Color::WHITE, 1.0, None);
        assert!(r.get(1, 5)[0] < 10);
        assert!(r.get(98, 5)[0] > 245);
        assert!((r.get(50, 5)[0] as i32 - 128).abs() < 4);
        assert_eq!(r.get(50, 5)[3], 255);
    }
}
