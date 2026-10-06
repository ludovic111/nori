//! Filters that change a layer's pixels (inside the selection): blurs, sharpen, noise,
//! pixelate. Each is described by a [`FilterSpec`] (its parameters with ranges and defaults), so
//! commands, the window's dialogs and the Plugins area list them the same way; plugins written
//! with the SDK are described the same way too.

use nori_core::{Mask, Raster, Rect};
use rayon::prelude::*;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::transform::{premultiply, unpremultiply};

/// A parameter a filter takes.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamSpec {
    pub name: &'static str,
    pub label: &'static str,
    /// `number`, `integer`, `boolean` or `choice`.
    pub kind: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    /// For choices: the options (the value is the option's index or its name).
    pub options: &'static [&'static str],
    pub unit: &'static str,
}

const fn num(name: &'static str, label: &'static str, min: f64, max: f64, default: f64, unit: &'static str) -> ParamSpec {
    ParamSpec { name, label, kind: "number", min, max, default, options: &[], unit }
}

const fn int(name: &'static str, label: &'static str, min: f64, max: f64, default: f64, unit: &'static str) -> ParamSpec {
    ParamSpec { name, label, kind: "integer", min, max, default, options: &[], unit }
}

const fn boolean(name: &'static str, label: &'static str, default: bool) -> ParamSpec {
    ParamSpec { name, label, kind: "boolean", min: 0.0, max: 1.0, default: if default { 1.0 } else { 0.0 }, options: &[], unit: "" }
}

const fn choice(name: &'static str, label: &'static str, options: &'static [&'static str]) -> ParamSpec {
    ParamSpec { name, label, kind: "choice", min: 0.0, max: (options.len() - 1) as f64, default: 0.0, options, unit: "" }
}

/// A stock filter.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// Menu group: blur, sharpen, noise, stylize.
    pub group: &'static str,
    pub description: &'static str,
    pub params: &'static [ParamSpec],
}

pub static STOCK: &[FilterSpec] = &[
    FilterSpec {
        id: "gaussianBlur",
        name: "Gaussian Blur",
        group: "blur",
        description: "Softens the picture evenly; radius is the blur's size in pixels.",
        params: &[num("radius", "Radius", 0.1, 250.0, 4.0, "px")],
    },
    FilterSpec {
        id: "motionBlur",
        name: "Motion Blur",
        group: "blur",
        description: "Streaks the picture along a direction, as if it moved during the exposure.",
        params: &[num("angle", "Angle", -180.0, 180.0, 0.0, "°"), num("distance", "Distance", 1.0, 500.0, 20.0, "px")],
    },
    FilterSpec {
        id: "sharpen",
        name: "Sharpen",
        group: "sharpen",
        description: "Unsharp mask: strengthens edges by `amount`, over `radius` pixels, ignoring differences under `threshold`.",
        params: &[num("amount", "Amount", 1.0, 500.0, 100.0, "%"), num("radius", "Radius", 0.1, 100.0, 1.5, "px"), int("threshold", "Threshold", 0.0, 255.0, 0.0, "levels")],
    },
    FilterSpec {
        id: "noise",
        name: "Add Noise",
        group: "noise",
        description: "Adds grain: `amount` of it, gaussian or uniform, in colour or monochromatic.",
        params: &[num("amount", "Amount", 0.1, 400.0, 10.0, "%"), choice("distribution", "Distribution", &["gaussian", "uniform"]), boolean("monochromatic", "Monochromatic", false), int("seed", "Seed", 0.0, 1_000_000.0, 1.0, "")],
    },
    FilterSpec {
        id: "pixelate",
        name: "Pixelate",
        group: "stylize",
        description: "Turns the picture into square cells of one colour each.",
        params: &[int("cell", "Cell size", 2.0, 400.0, 12.0, "px")],
    },
];

pub fn spec(id: &str) -> Option<&'static FilterSpec> {
    STOCK.iter().find(|s| s.id.eq_ignore_ascii_case(id))
}

/// Parameter values from JSON, checked against the spec: defaults filled in, ranges clamped,
/// unknown names refused.
pub fn values(spec: &FilterSpec, given: &Map<String, Value>) -> Result<Vec<f64>, String> {
    for k in given.keys() {
        if !spec.params.iter().any(|p| p.name == k) {
            let names: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
            let hint = nori_core::closest(k, &names).map(|c| format!(" Did you mean `{c}`?")).unwrap_or_default();
            return Err(format!("{} has no parameter `{k}`.{hint} Parameters: {}.", spec.name, names.join(", ")));
        }
    }
    spec.params
        .iter()
        .map(|p| {
            let v = match given.get(p.name) {
                None | Some(Value::Null) => p.default,
                Some(Value::Bool(b)) => f64::from(u8::from(*b)),
                Some(Value::Number(n)) => n.as_f64().unwrap_or(p.default),
                Some(Value::String(s)) if p.kind == "choice" => {
                    p.options.iter().position(|o| o.eq_ignore_ascii_case(s)).map(|i| i as f64).ok_or_else(|| format!("`{}` is one of {}", p.name, p.options.join(", ")))?
                }
                Some(Value::String(s)) => s.trim().parse::<f64>().map_err(|_| format!("`{}` should be a number", p.name))?,
                Some(other) => return Err(format!("`{}` should be a number, not {other}", p.name)),
            };
            if !v.is_finite() {
                return Err(format!("`{}` should be a finite number", p.name));
            }
            let v = v.clamp(p.min, p.max);
            Ok(if p.kind == "integer" || p.kind == "choice" { v.round() } else { v })
        })
        .collect()
}

/// How far a filter reads beyond the pixels it changes (so the selection's edge sees its
/// neighbours).
pub fn margin(id: &str, v: &[f64]) -> i32 {
    match id {
        "gaussianBlur" => (v[0] * 3.0).ceil() as i32 + 2,
        "motionBlur" => v[1].ceil() as i32 + 2,
        "sharpen" => (v[1] * 3.0).ceil() as i32 + 2,
        "pixelate" => v[0] as i32,
        _ => 0,
    }
}

/// Runs a stock filter on premultiplied pixels `w`×`h`; `origin` is where they sit on the canvas
/// (pixelate lines its cells up with the canvas, noise is seeded by canvas position).
pub fn run(id: &str, v: &[f64], px: &mut [[f32; 4]], w: usize, h: usize, origin: (i32, i32)) -> Result<(), String> {
    match id {
        "gaussianBlur" => gaussian_blur(px, w, h, v[0] as f32),
        "motionBlur" => motion_blur(px, w, h, v[0] as f32, v[1] as f32),
        "sharpen" => {
            let mut blurred = px.to_vec();
            gaussian_blur(&mut blurred, w, h, v[1] as f32);
            let amount = (v[0] / 100.0) as f32;
            let threshold = (v[2] / 255.0) as f32;
            px.par_iter_mut().zip(blurred.par_iter()).for_each(|(p, b)| {
                let a = p[3];
                for c in 0..3 {
                    let d = p[c] - b[c];
                    if d.abs() >= threshold * a.max(1e-6) {
                        p[c] = (p[c] + d * amount).clamp(0.0, a);
                    }
                }
            });
        }
        "noise" => {
            let amount = (v[0] / 100.0) as f32 * 0.5;
            let gaussian = v[1] < 0.5;
            let mono = v[2] >= 0.5;
            let seed = v[3] as u32;
            px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    let a = p[3];
                    if a <= 0.0 {
                        continue;
                    }
                    let (gx, gy) = ((origin.0 + x as i32) as u32, (origin.1 + y as i32) as u32);
                    let n = |ch: u32| {
                        let u = |k: u32| hash(gx, gy, seed.wrapping_mul(7).wrapping_add(ch * 3 + k)) as f32 / u32::MAX as f32;
                        if gaussian { (u(0) + u(1) + u(2) + u(3) - 2.0) * 0.866 } else { u(0) * 2.0 - 1.0 }
                    };
                    let shared = n(0);
                    for c in 0..3 {
                        let d = if mono { shared } else { n(c as u32 + 1) } * amount;
                        p[c] = (p[c] / a + d).clamp(0.0, 1.0) * a;
                    }
                }
            });
        }
        "pixelate" => {
            let cell = (v[0] as i32).max(1);
            let start_x = origin.0.div_euclid(cell) * cell - origin.0;
            let start_y = origin.1.div_euclid(cell) * cell - origin.1;
            let mut cy = start_y;
            while cy < h as i32 {
                let mut cx = start_x;
                while cx < w as i32 {
                    let (x0, y0) = (cx.max(0) as usize, cy.max(0) as usize);
                    let (x1, y1) = (((cx + cell) as usize).min(w), ((cy + cell) as usize).min(h));
                    let mut acc = [0f32; 4];
                    let n = ((x1 - x0) * (y1 - y0)).max(1) as f32;
                    for y in y0..y1 {
                        for x in x0..x1 {
                            for c in 0..4 {
                                acc[c] += px[y * w + x][c];
                            }
                        }
                    }
                    let avg = acc.map(|v| v / n);
                    for y in y0..y1 {
                        for x in x0..x1 {
                            px[y * w + x] = avg;
                        }
                    }
                    cx += cell;
                }
                cy += cell;
            }
        }
        other => return Err(format!("No filter `{other}`.")),
    }
    Ok(())
}

fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

/// Three box blurs approximating a gaussian of standard deviation `sigma`.
fn boxes_for(sigma: f32, n: usize) -> Vec<usize> {
    let w_ideal = ((12.0 * sigma * sigma / n as f32) + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal = (12.0 * sigma * sigma - (n as i32 * wl * wl) as f32 - (4 * n as i32 * wl) as f32 - 3.0 * n as f32) / (-4.0 * wl as f32 - 4.0);
    let m = m_ideal.round() as i32;
    (0..n as i32).map(|i| (if i < m { wl } else { wu }).max(1) as usize).collect()
}

fn box_rows(src: &[[f32; 4]], dst: &mut [[f32; 4]], w: usize, r: usize) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }
    dst.par_chunks_mut(w).zip(src.par_chunks(w)).for_each(|(out, row)| {
        let n = (2 * r + 1) as f32;
        let mut acc = [0f32; 4];
        // Edges extend the edge pixel.
        let at = |i: isize| row[i.clamp(0, w as isize - 1) as usize];
        for i in -(r as isize)..=(r as isize) {
            let p = at(i);
            for c in 0..4 {
                acc[c] += p[c];
            }
        }
        for x in 0..w {
            out[x] = acc.map(|v| v / n);
            let add = at(x as isize + r as isize + 1);
            let sub = at(x as isize - r as isize);
            for c in 0..4 {
                acc[c] += add[c] - sub[c];
            }
        }
    });
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

pub fn gaussian_blur(px: &mut [[f32; 4]], w: usize, h: usize, sigma: f32) {
    if sigma < 0.2 || w == 0 || h == 0 {
        return;
    }
    let boxes = boxes_for(sigma, 3);
    let mut tmp = vec![[0f32; 4]; px.len()];
    for b in &boxes {
        box_rows(px, &mut tmp, w, (b - 1) / 2);
        px.copy_from_slice(&tmp);
    }
    let mut t = transpose(px, w, h);
    let mut tmp = vec![[0f32; 4]; t.len()];
    for b in &boxes {
        box_rows(&t, &mut tmp, h, (b - 1) / 2);
        t.copy_from_slice(&tmp);
    }
    px.copy_from_slice(&transpose(&t, h, w));
}

fn motion_blur(px: &mut [[f32; 4]], w: usize, h: usize, angle: f32, distance: f32) {
    let (s, c) = angle.to_radians().sin_cos();
    let n = (distance.ceil() as i32).max(1);
    let src = px.to_vec();
    px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let mut acc = [0f32; 4];
            for k in -n / 2..=n / 2 {
                let (fx, fy) = (x as f32 + c * k as f32, y as f32 - s * k as f32);
                let (ix, iy) = ((fx.round() as isize).clamp(0, w as isize - 1) as usize, (fy.round() as isize).clamp(0, h as isize - 1) as usize);
                let p = src[iy * w + ix];
                for ch in 0..4 {
                    acc[ch] += p[ch];
                }
            }
            let m = (n / 2 * 2 + 1) as f32;
            *o = acc.map(|v| v / m);
        }
    });
}

/// Runs `f` on a layer's pixels inside the selection (all of them without one): the area the
/// selection covers, `margin` pixels wider so filters see the neighbours, is handed to `f` as
/// premultiplied pixels with its canvas origin; the result is mixed back by the selection.
/// Returns the canvas rectangle that changed.
pub fn apply_in_selection(pixels: &mut Raster, ox: i32, oy: i32, selection: Option<&Mask>, margin: i32, f: impl FnOnce(&mut [[f32; 4]], usize, usize, (i32, i32)) -> Result<(), String>) -> Result<Rect, String> {
    let layer = Rect::new(ox, oy, pixels.width(), pixels.height());
    let target = match selection {
        Some(m) if m.fill()[0] == 0 => match m.content_bounds() {
            Some(b) => b.intersect(&layer),
            None => return Ok(Rect::default()),
        },
        _ => layer,
    };
    if target.is_empty() {
        return Ok(Rect::default());
    }
    let work = target.inflate(margin).intersect(&layer);
    let local = work.translate(-ox, -oy);
    let before = pixels.read_rect(local);
    let mut px = premultiply(&before);
    f(&mut px, work.w as usize, work.h as usize, (work.x, work.y))?;
    let after = unpremultiply(&px);
    // Mixed back by the selection; outside the target, nothing changes.
    let sel = selection.map(|m| m.read_rect(work));
    let mut out = before.clone();
    for i in 0..(work.w * work.h) as usize {
        let (x, y) = (work.x + (i as u32 % work.w) as i32, work.y + (i as u32 / work.w) as i32);
        if !target.contains(x, y) {
            continue;
        }
        let k = sel.as_ref().map_or(1.0, |s| s[i] as f32 / 255.0);
        if k <= 0.0 {
            continue;
        }
        for c in 0..4 {
            let (a, b) = (before[i * 4 + c] as f32, after[i * 4 + c] as f32);
            out[i * 4 + c] = (a + (b - a) * k).round() as u8;
        }
    }
    pixels.write_rect(local, &out);
    pixels.compact();
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_default_clamp_and_refuse() {
        let s = spec("gaussianBlur").unwrap();
        assert_eq!(values(s, &Map::new()).unwrap(), vec![4.0]);
        let mut m = Map::new();
        m.insert("radius".into(), serde_json::json!(9999));
        assert_eq!(values(s, &m).unwrap(), vec![250.0]);
        m.insert("radiu".into(), serde_json::json!(1));
        assert!(values(s, &m).unwrap_err().contains("Did you mean `radius`"));
        let n = spec("noise").unwrap();
        let mut m = Map::new();
        m.insert("distribution".into(), serde_json::json!("uniform"));
        assert_eq!(values(n, &m).unwrap()[1], 1.0);
    }

    #[test]
    fn blur_keeps_flat_areas_and_spreads_a_dot() {
        let (w, h) = (40, 30);
        let mut px = vec![[0.5f32, 0.5, 0.5, 1.0]; w * h];
        gaussian_blur(&mut px, w, h, 3.0);
        assert!(px.iter().all(|p| (p[0] - 0.5).abs() < 1e-4));
        let mut dot = vec![[0f32; 4]; w * h];
        dot[15 * w + 20] = [1.0, 1.0, 1.0, 1.0];
        gaussian_blur(&mut dot, w, h, 2.0);
        let total: f32 = dot.iter().map(|p| p[3]).sum();
        assert!((total - 1.0).abs() < 0.02, "{total}");
        assert!(dot[15 * w + 20][3] < 0.2 && dot[15 * w + 22][3] > 0.01);
    }

    #[test]
    fn filters_stay_inside_the_selection() {
        let mut r = Raster::solid(20, 20, [100, 100, 100, 255]);
        r.set(5, 5, [255, 255, 255, 255]);
        let mut sel = Mask::filled(20, 20, 0);
        for y in 0..10 {
            for x in 0..10 {
                sel.set(x, y, [255]);
            }
        }
        let changed = apply_in_selection(&mut r, 0, 0, Some(&sel), 5, |px, w, h, o| run("noise", &[50.0, 0.0, 0.0, 1.0], px, w, h, o)).unwrap();
        assert_eq!(changed, Rect::new(0, 0, 10, 10));
        assert_eq!(r.get(15, 15), [100, 100, 100, 255]);
        let inside: Vec<[u8; 4]> = (0..10).map(|x| r.get(x, 2)).collect();
        assert!(inside.iter().any(|p| *p != [100, 100, 100, 255]));
        // Pixelate makes cells of one colour.
        let mut p = vec![[0f32; 4]; 16];
        p[0] = [1.0, 1.0, 1.0, 1.0];
        run("pixelate", &[2.0], &mut p, 4, 4, (0, 0)).unwrap();
        assert_eq!(p[1], [0.25, 0.25, 0.25, 0.25]);
        assert_eq!(p[2], [0.0; 4]);
    }
}
