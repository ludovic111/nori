//! Adjustments as fast per-pixel functions: an [`Adjustment`] is compiled once into tables or a
//! small closure-free form, then applied to straight colours in 0..1.

use nori_core::Adjustment;

const TABLE: usize = 4096;

/// An adjustment ready to apply to pixels.
#[derive(Clone, Debug)]
pub enum Compiled {
    /// One curve per channel (levels, curves, exposure, brightness/contrast, invert, posterize).
    Tables(Box<[[f32; TABLE + 1]; 3]>),
    HueSaturation { hue: f32, saturation: f32, lightness: f32, colorize: bool },
    Vibrance { vibrance: f32, saturation: f32 },
    BlackWhite { weights: [f32; 3] },
    Threshold { level: f32 },
    Lut { size: usize, amount: f32, table: std::sync::Arc<Vec<[f32; 3]>> },
}

fn tables(f: impl Fn(usize, f32) -> f32) -> Compiled {
    let mut t = Box::new([[0f32; TABLE + 1]; 3]);
    for (ch, row) in t.iter_mut().enumerate() {
        for (i, v) in row.iter_mut().enumerate() {
            *v = f(ch, i as f32 / TABLE as f32).clamp(0.0, 1.0);
        }
    }
    Compiled::Tables(t)
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055 }
}

/// A smooth curve through `points` (0–255 in and out), monotone between points so it never
/// overshoots (Fritsch–Carlson), flat before the first point and after the last.
pub fn curve_fn(points: &[[f32; 2]]) -> impl Fn(f32) -> f32 + '_ {
    let n = points.len();
    let xs: Vec<f32> = points.iter().map(|p| p[0] / 255.0).collect();
    let ys: Vec<f32> = points.iter().map(|p| p[1] / 255.0).collect();
    let mut m = vec![0f32; n];
    if n >= 2 {
        let d: Vec<f32> = (0..n - 1).map(|i| (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]).max(1e-6)).collect();
        m[0] = d[0];
        m[n - 1] = d[n - 2];
        for i in 1..n - 1 {
            m[i] = if d[i - 1] * d[i] <= 0.0 { 0.0 } else { (d[i - 1] + d[i]) / 2.0 };
        }
        for i in 0..n - 1 {
            if d[i] == 0.0 {
                m[i] = 0.0;
                m[i + 1] = 0.0;
            } else {
                let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
                let s = a * a + b * b;
                if s > 9.0 {
                    let t = 3.0 / s.sqrt();
                    m[i] = t * a * d[i];
                    m[i + 1] = t * b * d[i];
                }
            }
        }
    }
    move |x: f32| {
        if n == 0 {
            return x;
        }
        if x <= xs[0] {
            return ys[0];
        }
        if x >= xs[n - 1] {
            return ys[n - 1];
        }
        let i = xs.partition_point(|v| *v <= x).saturating_sub(1).min(n - 2);
        let h = (xs[i + 1] - xs[i]).max(1e-6);
        let t = (x - xs[i]) / h;
        let (t2, t3) = (t * t, t * t * t);
        (2.0 * t3 - 3.0 * t2 + 1.0) * ys[i] + (t3 - 2.0 * t2 + t) * h * m[i] + (-2.0 * t3 + 3.0 * t2) * ys[i + 1] + (t3 - t2) * h * m[i + 1]
    }
}

/// Compiles an adjustment.
pub fn compile(a: &Adjustment) -> Compiled {
    match a {
        Adjustment::Levels { input_black, input_white, gamma, output_black, output_white } => {
            let (ib, iw, g, ob, ow) = (input_black / 255.0, input_white / 255.0, gamma.max(0.01), output_black / 255.0, output_white / 255.0);
            tables(move |_, x| {
                let t = ((x - ib) / (iw - ib).max(1e-4)).clamp(0.0, 1.0).powf(1.0 / g);
                ob + t * (ow - ob)
            })
        }
        Adjustment::Curves { rgb, red, green, blue } => {
            let master = curve_fn(rgb);
            let per = [curve_fn(red), curve_fn(green), curve_fn(blue)];
            tables(|ch, x| per[ch](master(x)))
        }
        Adjustment::Exposure { exposure, offset, gamma } => {
            let (k, o, g) = (2f32.powf(*exposure), *offset, gamma.max(0.01));
            tables(move |_, x| {
                let lin = (srgb_to_linear(x) * k + o).max(0.0);
                linear_to_srgb(lin.powf(1.0 / g))
            })
        }
        Adjustment::BrightnessContrast { brightness, contrast } => {
            let b = brightness / 150.0 * 0.5;
            // A contrast of 100 is a steep S; −50 halves the range, as other editors do.
            let c = if *contrast >= 0.0 { 1.0 + contrast / 100.0 * 2.0 } else { 1.0 + contrast / 100.0 };
            tables(move |_, x| {
                // Brightness bends the curve (keeping black and white), contrast pivots at the middle.
                let y = if b >= 0.0 { x + b * (1.0 - x) * x * 4.0 * 0.5 } else { x + b * x * (1.0 - x) * 4.0 * 0.5 };
                (y - 0.5) * c + 0.5
            })
        }
        Adjustment::Invert => tables(|_, x| 1.0 - x),
        Adjustment::Posterize { levels } => {
            let n = levels.round().clamp(2.0, 255.0);
            tables(move |_, x| ((x * (n - 1.0)).round() / (n - 1.0)).clamp(0.0, 1.0))
        }
        Adjustment::HueSaturation { hue, saturation, lightness, colorize } => {
            Compiled::HueSaturation { hue: *hue, saturation: saturation / 100.0, lightness: lightness / 100.0, colorize: *colorize }
        }
        Adjustment::Vibrance { vibrance, saturation } => Compiled::Vibrance { vibrance: vibrance / 100.0, saturation: saturation / 100.0 },
        Adjustment::BlackWhite { red, green, blue } => Compiled::BlackWhite { weights: [red / 100.0, green / 100.0, blue / 100.0] },
        Adjustment::Threshold { level } => Compiled::Threshold { level: level / 255.0 },
        Adjustment::Lut { size, amount, table, .. } => Compiled::Lut { size: *size as usize, amount: *amount, table: table.clone() },
    }
}

#[inline]
fn lookup(t: &[f32; TABLE + 1], x: f32) -> f32 {
    let f = x.clamp(0.0, 1.0) * TABLE as f32;
    let i = f as usize;
    if i >= TABLE {
        return t[TABLE];
    }
    let k = f - i as f32;
    t[i] + (t[i + 1] - t[i]) * k
}

fn rgb_to_hsl(c: [f32; 3]) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    let l = (mx + mn) / 2.0;
    if mx - mn < 1e-6 {
        return [0.0, 0.0, l];
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == c[0] {
        (c[1] - c[2]) / d + if c[1] < c[2] { 6.0 } else { 0.0 }
    } else if mx == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    [h / 6.0, s, l]
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= 0.0 {
        return [l; 3];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

impl Compiled {
    /// The adjusted colour of a straight pixel.
    #[inline]
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        match self {
            Compiled::Tables(t) => [lookup(&t[0], c[0]), lookup(&t[1], c[1]), lookup(&t[2], c[2])],
            Compiled::HueSaturation { hue, saturation, lightness, colorize } => {
                let [h, s, l] = rgb_to_hsl(c);
                let (h, s) = if *colorize { ((hue / 360.0).rem_euclid(1.0), (0.25 + saturation * 0.75).clamp(0.0, 1.0)) } else { ((h + hue / 360.0).rem_euclid(1.0), (s * (1.0 + saturation)).clamp(0.0, 1.0)) };
                let mut out = hsl_to_rgb(h, s, l);
                if *lightness > 0.0 {
                    out = out.map(|v| v + (1.0 - v) * lightness);
                } else if *lightness < 0.0 {
                    out = out.map(|v| v * (1.0 + lightness));
                }
                out
            }
            Compiled::Vibrance { vibrance, saturation } => {
                let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                let mx = c[0].max(c[1]).max(c[2]);
                let mn = c[0].min(c[1]).min(c[2]);
                let sat = mx - mn;
                // Vibrance pushes weak colours more than strong ones.
                let k = 1.0 + saturation + vibrance * (1.0 - sat);
                c.map(|v| (l + (v - l) * k).clamp(0.0, 1.0))
            }
            Compiled::BlackWhite { weights } => {
                let total = (weights[0] + weights[1] + weights[2]).abs().max(1e-3);
                let g = ((c[0] * weights[0] + c[1] * weights[1] + c[2] * weights[2]) / total.max(1.0)).clamp(0.0, 1.0);
                [g; 3]
            }
            Compiled::Threshold { level } => {
                let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                if l >= *level { [1.0; 3] } else { [0.0; 3] }
            }
            Compiled::Lut { size, amount, table } => {
                if *size < 2 || table.len() != size * size * size {
                    return c;
                }
                let out = trilinear(table, *size, c);
                [c[0] + (out[0] - c[0]) * amount, c[1] + (out[1] - c[1]) * amount, c[2] + (out[2] - c[2]) * amount]
            }
        }
    }
}

fn trilinear(t: &[[f32; 3]], n: usize, c: [f32; 3]) -> [f32; 3] {
    let f = |v: f32| {
        let x = v.clamp(0.0, 1.0) * (n - 1) as f32;
        let i = (x as usize).min(n - 2);
        (i, x - i as f32)
    };
    let (r, fr) = f(c[0]);
    let (g, fg) = f(c[1]);
    let (b, fb) = f(c[2]);
    let at = |r: usize, g: usize, b: usize| t[r + g * n + b * n * n];
    let mut out = [0f32; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let c00 = at(r, g, b)[k] * (1.0 - fr) + at(r + 1, g, b)[k] * fr;
        let c10 = at(r, g + 1, b)[k] * (1.0 - fr) + at(r + 1, g + 1, b)[k] * fr;
        let c01 = at(r, g, b + 1)[k] * (1.0 - fr) + at(r + 1, g, b + 1)[k] * fr;
        let c11 = at(r, g + 1, b + 1)[k] * (1.0 - fr) + at(r + 1, g + 1, b + 1)[k] * fr;
        let c0 = c00 * (1.0 - fg) + c10 * fg;
        let c1 = c01 * (1.0 - fg) + c11 * fg;
        *o = c0 * (1.0 - fb) + c1 * fb;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 2e-3)
    }

    #[test]
    fn neutral_settings_change_nothing() {
        for k in Adjustment::KINDS {
            if matches!(k, "invert" | "blackWhite" | "threshold" | "posterize") {
                continue;
            }
            let c = compile(&Adjustment::default_of(k).unwrap());
            for px in [[0.1, 0.5, 0.9], [1.0, 0.0, 0.3], [0.5, 0.5, 0.5]] {
                assert!(close(c.apply(px), px), "{k}: {px:?} → {:?}", c.apply(px));
            }
        }
    }

    #[test]
    fn levels_curves_invert() {
        let l = compile(&Adjustment::Levels { input_black: 0.0, input_white: 127.5, gamma: 1.0, output_black: 0.0, output_white: 255.0 });
        assert!(close(l.apply([0.25, 0.5, 1.0]), [0.5, 1.0, 1.0]));
        let inv = compile(&Adjustment::Invert);
        assert!(close(inv.apply([0.2, 0.4, 1.0]), [0.8, 0.6, 0.0]));
        let curve = Adjustment::Curves { rgb: vec![[0.0, 0.0], [128.0, 160.0], [255.0, 255.0]], red: nori_core::layer::identity_curve(), green: nori_core::layer::identity_curve(), blue: nori_core::layer::identity_curve() };
        let c = compile(&curve);
        let mid = c.apply([128.0 / 255.0; 3])[0];
        assert!((mid - 160.0 / 255.0).abs() < 3e-3, "{mid}");
        // Monotone: never goes down.
        let mut last = -1.0;
        for i in 0..=255 {
            let v = c.apply([i as f32 / 255.0; 3])[0];
            assert!(v >= last - 1e-6);
            last = v;
        }
    }

    #[test]
    fn hue_rotates_and_desaturates() {
        let h = compile(&Adjustment::HueSaturation { hue: 120.0, saturation: 0.0, lightness: 0.0, colorize: false });
        assert!(close(h.apply([1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        let s = compile(&Adjustment::HueSaturation { hue: 0.0, saturation: -100.0, lightness: 0.0, colorize: false });
        let g = s.apply([1.0, 0.0, 0.0]);
        assert!((g[0] - g[1]).abs() < 1e-4 && (g[1] - g[2]).abs() < 1e-4);
    }

    #[test]
    fn identity_lut_is_identity() {
        let n = 5;
        let mut t = vec![];
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    t.push([r as f32 / 4.0, g as f32 / 4.0, b as f32 / 4.0]);
                }
            }
        }
        let c = compile(&Adjustment::Lut { name: "id".into(), size: 5, amount: 1.0, table: std::sync::Arc::new(t) });
        assert!(close(c.apply([0.3, 0.6, 0.9]), [0.3, 0.6, 0.9]));
    }
}
