//! Blend modes, per the W3C Compositing and Blending spec (which is what other editors do),
//! on straight colours in 0..1.

use nori_core::BlendMode;

#[inline]
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

#[inline]
fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut o = c;
    if n < 0.0 {
        let d = (l - n).max(1e-6);
        o = [l + (o[0] - l) * l / d, l + (o[1] - l) * l / d, l + (o[2] - l) * l / d];
    }
    if x > 1.0 {
        let d = (x - l).max(1e-6);
        o = [l + (o[0] - l) * (1.0 - l) / d, l + (o[1] - l) * (1.0 - l) / d, l + (o[2] - l) * (1.0 - l) / d];
    }
    o
}

#[inline]
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

#[inline]
fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

#[inline]
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    if mx - mn <= 1e-6 {
        return [0.0; 3];
    }
    let f = |v: f32| (v - mn) * s / (mx - mn);
    [f(c[0]), f(c[1]), f(c[2])]
}

#[inline]
fn separable(mode: BlendMode, b: f32, s: f32) -> f32 {
    match mode {
        BlendMode::Multiply => b * s,
        BlendMode::Screen => b + s - b * s,
        BlendMode::Overlay => hard_light(s, b),
        BlendMode::Darken => b.min(s),
        BlendMode::Lighten => b.max(s),
        BlendMode::ColorDodge => {
            if b <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }
        BlendMode::HardLight => hard_light(b, s),
        BlendMode::SoftLight => {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }
        BlendMode::Difference => (b - s).abs(),
        BlendMode::Exclusion => b + s - 2.0 * b * s,
        BlendMode::LinearBurn => (b + s - 1.0).max(0.0),
        BlendMode::LinearDodge => (b + s).min(1.0),
        BlendMode::LinearLight => (b + 2.0 * s - 1.0).clamp(0.0, 1.0),
        BlendMode::VividLight => {
            if s <= 0.5 {
                let s2 = 2.0 * s;
                if s2 <= 0.0 { if b >= 1.0 { 1.0 } else { 0.0 } } else { 1.0 - ((1.0 - b) / s2).min(1.0) }
            } else {
                let s2 = 2.0 * (s - 0.5);
                if s2 >= 1.0 { if b <= 0.0 { 0.0 } else { 1.0 } } else { (b / (1.0 - s2)).min(1.0) }
            }
        }
        BlendMode::PinLight => {
            if s <= 0.5 {
                b.min(2.0 * s)
            } else {
                b.max(2.0 * s - 1.0)
            }
        }
        BlendMode::Subtract => (b - s).max(0.0),
        BlendMode::Divide => {
            if s <= 0.0 {
                if b <= 0.0 { 0.0 } else { 1.0 }
            } else {
                (b / s).min(1.0)
            }
        }
        _ => s,
    }
}

#[inline]
fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 { b * 2.0 * s } else { let s2 = 2.0 * s - 1.0; b + s2 - b * s2 }
}

/// `B(Cb, Cs)`: the blended colour of a backdrop `b` and a source `s` (straight, 0..1).
#[inline]
pub fn blend(mode: BlendMode, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => s,
        BlendMode::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
        BlendMode::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
        BlendMode::Color => set_lum(s, lum(b)),
        BlendMode::Luminosity => set_lum(b, lum(s)),
        m => [separable(m, b[0], s[0]), separable(m, b[1], s[1]), separable(m, b[2], s[2])],
    }
}

/// Composites a source pixel (straight colour `s`, coverage `sa` already including opacity and
/// masks) over a premultiplied backdrop `dst` (r, g, b, a), with a blend mode.
#[inline]
pub fn over(dst: &mut [f32; 4], s: [f32; 3], sa: f32, mode: BlendMode) {
    if sa <= 0.0 {
        return;
    }
    let ba = dst[3];
    let mixed = if matches!(mode, BlendMode::Normal | BlendMode::PassThrough) || ba <= 0.0 {
        s
    } else {
        let inv = 1.0 / ba;
        let b = [dst[0] * inv, dst[1] * inv, dst[2] * inv];
        let bl = blend(mode, b, s);
        [(1.0 - ba) * s[0] + ba * bl[0], (1.0 - ba) * s[1] + ba * bl[1], (1.0 - ba) * s[2] + ba * bl[2]]
    };
    let k = 1.0 - sa;
    dst[0] = sa * mixed[0] + k * dst[0];
    dst[1] = sa * mixed[1] + k * dst[1];
    dst[2] = sa * mixed[2] + k * dst[2];
    dst[3] = sa + k * ba;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        let b = [0.5, 0.25, 1.0];
        let s = [0.5, 0.5, 0.0];
        assert_eq!(blend(BlendMode::Multiply, b, s), [0.25, 0.125, 0.0]);
        assert_eq!(blend(BlendMode::Screen, b, s), [0.75, 0.625, 1.0]);
        assert_eq!(blend(BlendMode::Difference, b, s), [0.0, 0.25, 1.0]);
        assert_eq!(blend(BlendMode::Darken, b, s), [0.5, 0.25, 0.0]);
        // Luminosity keeps the backdrop's hue and saturation.
        let grey = blend(BlendMode::Luminosity, [0.5, 0.5, 0.5], [1.0, 0.0, 0.0]);
        assert!((grey[0] - 0.3).abs() < 1e-5 && (grey[1] - 0.3).abs() < 1e-5);
        for m in BlendMode::ALL {
            let o = blend(m, b, s);
            assert!(o.iter().all(|v| (-1e-5..=1.00001).contains(v)), "{m:?} → {o:?}");
        }
    }

    #[test]
    fn source_over_is_porter_duff() {
        let mut d = [0.0, 0.0, 0.5, 0.5]; // 50 % blue, premultiplied
        over(&mut d, [1.0, 0.0, 0.0], 0.5, BlendMode::Normal);
        assert!((d[3] - 0.75).abs() < 1e-6);
        assert!((d[0] - 0.5).abs() < 1e-6 && (d[2] - 0.25).abs() < 1e-6);
        // Over nothing, every mode is the source.
        let mut e = [0.0; 4];
        over(&mut e, [0.2, 0.4, 0.6], 1.0, BlendMode::Multiply);
        assert_eq!(e, [0.2, 0.4, 0.6, 1.0]);
    }
}
