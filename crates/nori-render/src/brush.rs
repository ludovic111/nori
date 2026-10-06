//! The brush engine: round (or picture) tips stamped along a stroke, with hardness, flow,
//! opacity, spacing and pen pressure.
//!
//! A [`Stroke`] keeps the layer as it was when the stroke began and how much paint each pixel
//! has received so far (coverage, built up dab by dab with flow, capped at 1). The layer is then
//! "before, with the colour laid over at coverage × opacity" (or erased by that much), so
//! repainting over the same spot in one stroke never goes past the opacity, as in other
//! editors. The window feeds points as the pointer moves and draws what changed; the
//! `paint.stroke` command feeds the same points in one go, so both give the very same pixels.

use std::collections::HashMap;
use std::sync::Arc;

use nori_core::{Color, Mask, Raster, Rect, TILE};
use serde::{Deserialize, Serialize};

/// A brush tip picture (from a `.gbr` brush): grey coverage, `width`×`height`.
#[derive(Clone, Debug, PartialEq)]
pub struct TipImage {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Brush {
    /// Diameter in pixels.
    pub size: f32,
    /// 0 (soft) .. 1 (hard edge).
    pub hardness: f32,
    /// The most paint one stroke lays down, 0..1.
    pub opacity: f32,
    /// Paint per dab, 0..1 (strokes build up to the opacity).
    pub flow: f32,
    /// Distance between dabs as a share of the size.
    pub spacing: f32,
    /// Pressure changes the size.
    pub pressure_size: bool,
    /// Pressure changes the flow.
    pub pressure_opacity: bool,
    /// A picture tip by name (from `brush.import`); none: round.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
}

impl Default for Brush {
    fn default() -> Self {
        Self { size: 24.0, hardness: 0.8, opacity: 1.0, flow: 1.0, spacing: 0.15, pressure_size: true, pressure_opacity: false, tip: None }
    }
}

impl Brush {
    pub fn clamped(mut self) -> Self {
        self.size = self.size.clamp(1.0, 5000.0);
        self.hardness = self.hardness.clamp(0.0, 1.0);
        self.opacity = self.opacity.clamp(0.0, 1.0);
        self.flow = self.flow.clamp(0.01, 1.0);
        self.spacing = self.spacing.clamp(0.01, 5.0);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Paint(Color),
    Erase,
}

/// A stroke in progress on one raster layer.
pub struct Stroke {
    brush: Brush,
    mode: Mode,
    tip: Option<Arc<TipImage>>,
    before: Raster,
    /// Where the raster's (0, 0) is on the page.
    origin: (i32, i32),
    selection: Option<Mask>,
    /// Paint received per layer tile.
    coverage: HashMap<(u32, u32), Vec<f32>>,
    last: Option<[f32; 3]>,
    /// Distance travelled since the last dab.
    carry: f32,
    /// Layer pixels touched since the last [`Stroke::apply`].
    dirty: Rect,
    /// Everything the stroke touched, in page pixels.
    touched: Rect,
}

impl Stroke {
    /// Starts a stroke on `raster` (placed at `origin` on the page) as it is now.
    pub fn new(raster: &Raster, origin: (i32, i32), brush: Brush, mode: Mode, selection: Option<Mask>, tip: Option<Arc<TipImage>>) -> Self {
        Self { brush: brush.clamped(), mode, tip, before: raster.clone(), origin, selection, coverage: HashMap::new(), last: None, carry: 0.0, dirty: Rect::default(), touched: Rect::default() }
    }

    /// Adds a point (page x, y, pressure 0..1); stamps every dab up to it.
    pub fn add(&mut self, p: [f32; 3]) {
        let p = [p[0], p[1], p[2].clamp(0.0, 1.0)];
        let Some(last) = self.last else {
            self.dab(p);
            self.last = Some(p);
            self.carry = 0.0;
            return;
        };
        let (dx, dy) = (p[0] - last[0], p[1] - last[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 0.0 {
            return;
        }
        let mut t = 0.0;
        loop {
            let pressure = last[2] + (p[2] - last[2]) * (t / len).clamp(0.0, 1.0);
            let step = (self.size_at(pressure) * self.brush.spacing).max(0.5);
            let need = step - self.carry;
            if t + need > len {
                self.carry += len - t;
                break;
            }
            t += need;
            self.carry = 0.0;
            let k = t / len;
            self.dab([last[0] + dx * k, last[1] + dy * k, last[2] + (p[2] - last[2]) * k]);
        }
        self.last = Some(p);
    }

    fn size_at(&self, pressure: f32) -> f32 {
        if self.brush.pressure_size { (self.brush.size * pressure.max(0.05)).max(1.0) } else { self.brush.size }
    }

    /// One dab at a point (page coordinates).
    fn dab(&mut self, p: [f32; 3]) {
        let size = self.size_at(p[2]);
        let flow = self.brush.flow * if self.brush.pressure_opacity { p[2] } else { 1.0 };
        let r = size / 2.0;
        // Layer coordinates.
        let (cx, cy) = (p[0] - self.origin.0 as f32, p[1] - self.origin.1 as f32);
        let area = Rect::new((cx - r - 1.0).floor() as i32, (cy - r - 1.0).floor() as i32, (size + 3.0).ceil() as u32, (size + 3.0).ceil() as u32).intersect(&self.before.bounds());
        if area.is_empty() {
            return;
        }
        let hard = self.brush.hardness;
        let inner = r * hard;
        let tip = self.tip.clone();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let (fx, fy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let a = match &tip {
                    Some(t) => {
                        // The picture scaled to the brush size (its longer side), bilinear.
                        let s = size / t.width.max(t.height) as f32;
                        let (u, v) = (fx / s + t.width as f32 / 2.0 - 0.5, fy / s + t.height as f32 / 2.0 - 0.5);
                        sample_tip(t, u, v)
                    }
                    None => {
                        let d = (fx * fx + fy * fy).sqrt();
                        if d >= r + 0.5 {
                            0.0
                        } else if r - inner < 1.0 {
                            // A hard edge, smoothed over one pixel.
                            (r + 0.5 - d).clamp(0.0, 1.0)
                        } else if d <= inner {
                            1.0
                        } else {
                            let t = ((d - inner) / (r - inner)).clamp(0.0, 1.0);
                            1.0 - t * t * (3.0 - 2.0 * t)
                        }
                    }
                } * flow;
                if a <= 0.0 {
                    continue;
                }
                let (tx, ty) = (x as u32 / TILE, y as u32 / TILE);
                let cov = self.coverage.entry((tx, ty)).or_insert_with(|| vec![0.0; (TILE * TILE) as usize]);
                let i = ((y as u32 % TILE) * TILE + x as u32 % TILE) as usize;
                cov[i] = cov[i] + a * (1.0 - cov[i]);
            }
        }
        self.dirty = self.dirty.union(&area);
        self.touched = self.touched.union(&area.translate(self.origin.0, self.origin.1));
    }

    /// Writes the stroke so far into `raster` (the layer the stroke began on) where it changed
    /// since the last call. Returns the page rectangle that changed.
    pub fn apply(&mut self, raster: &mut Raster) -> Rect {
        let area = std::mem::take(&mut self.dirty);
        if area.is_empty() {
            return area;
        }
        let before = self.before.read_rect(area);
        let sel = self.selection.as_ref().map(|m| m.read_rect(area.translate(self.origin.0, self.origin.1)));
        let mut out = before.clone();
        let opacity = self.brush.opacity;
        for row in 0..area.h {
            for col in 0..area.w {
                let (x, y) = (area.x as u32 + col, area.y as u32 + row);
                let Some(cov) = self.coverage.get(&(x / TILE, y / TILE)) else { continue };
                let c = cov[((y % TILE) * TILE + x % TILE) as usize];
                if c <= 0.0 {
                    continue;
                }
                let i = (row * area.w + col) as usize;
                let k = c * opacity * sel.as_ref().map_or(1.0, |s| s[i] as f32 / 255.0);
                if k <= 0.0 {
                    continue;
                }
                let px = &mut out[i * 4..i * 4 + 4];
                match self.mode {
                    Mode::Erase => px[3] = (px[3] as f32 * (1.0 - k)).round() as u8,
                    Mode::Paint(color) => {
                        let sa = k * color.a;
                        let da = px[3] as f32 / 255.0;
                        let oa = sa + da * (1.0 - sa);
                        if oa > 0.0 {
                            let src = [color.r, color.g, color.b];
                            for ch in 0..3 {
                                let d = px[ch] as f32 / 255.0;
                                px[ch] = (((src[ch] * sa + d * da * (1.0 - sa)) / oa).clamp(0.0, 1.0) * 255.0).round() as u8;
                            }
                            px[3] = (oa * 255.0).round() as u8;
                        }
                    }
                }
            }
        }
        raster.write_rect(area, &out);
        area.translate(self.origin.0, self.origin.1)
    }

    /// Everything the stroke touched so far, in page pixels.
    pub fn touched(&self) -> Rect {
        self.touched
    }
}

fn sample_tip(t: &TipImage, u: f32, v: f32) -> f32 {
    let get = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= t.width as i64 || y >= t.height as i64 { 0.0 } else { t.data[(y as u32 * t.width + x as u32) as usize] as f32 / 255.0 }
    };
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (u - x0, v - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    get(x0, y0) * (1.0 - fx) * (1.0 - fy) + get(x0 + 1, y0) * fx * (1.0 - fy) + get(x0, y0 + 1) * (1.0 - fx) * fy + get(x0 + 1, y0 + 1) * fx * fy
}

/// Paints a whole stroke at once (the `paint.stroke` command): the same pixels the window shows.
pub fn paint(raster: &mut Raster, origin: (i32, i32), brush: &Brush, mode: Mode, selection: Option<&Mask>, tip: Option<Arc<TipImage>>, points: &[[f32; 3]]) -> Rect {
    let mut s = Stroke::new(raster, origin, brush.clone(), mode, selection.cloned(), tip);
    for p in points {
        s.add(*p);
    }
    s.apply(raster);
    raster.compact();
    s.touched()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stroke_paints_a_line_and_respects_opacity() {
        let mut r = Raster::transparent(100, 50);
        let b = Brush { size: 10.0, hardness: 1.0, opacity: 0.5, ..Default::default() };
        let touched = paint(&mut r, (0, 0), &b, Mode::Paint(Color::rgb(1.0, 0.0, 0.0)), None, None, &[[10.0, 25.0, 1.0], [90.0, 25.0, 1.0]]);
        assert!(touched.w > 80);
        let p = r.get(50, 25);
        assert_eq!(&p[..3], &[255, 0, 0]);
        // Overlapping dabs don't go past the opacity.
        assert!((p[3] as i32 - 128).abs() <= 1, "{p:?}");
        assert_eq!(r.get(50, 40)[3], 0);
    }

    #[test]
    fn incremental_equals_all_at_once() {
        let pts = [[5.0, 5.0, 0.3], [40.0, 30.0, 0.8], [70.0, 10.0, 1.0], [95.0, 45.0, 0.5]];
        let b = Brush { size: 14.0, hardness: 0.3, flow: 0.4, ..Default::default() };
        let mut a = Raster::solid(100, 50, [0, 0, 255, 255]);
        paint(&mut a, (0, 0), &b, Mode::Paint(Color::WHITE), None, None, &pts);
        let mut live = Raster::solid(100, 50, [0, 0, 255, 255]);
        let mut s = Stroke::new(&live, (0, 0), b.clone(), Mode::Paint(Color::WHITE), None, None);
        for p in pts {
            s.add(p);
            s.apply(&mut live);
        }
        live.compact();
        assert!(a.same_pixels(&live));
    }

    #[test]
    fn eraser_and_selection() {
        let mut r = Raster::solid(40, 40, [10, 20, 30, 255]);
        let mut sel = Mask::filled(40, 40, 0);
        for y in 0..40 {
            for x in 0..20 {
                sel.set(x, y, [255]);
            }
        }
        let b = Brush { size: 30.0, hardness: 1.0, ..Default::default() };
        paint(&mut r, (0, 0), &b, Mode::Erase, Some(&sel), None, &[[20.0, 20.0, 1.0]]);
        assert_eq!(r.get(15, 20)[3], 0);
        assert_eq!(r.get(25, 20)[3], 255);
        // A layer placed elsewhere on the page is painted where the pointer is.
        let mut moved = Raster::transparent(20, 20);
        paint(&mut moved, (100, 100), &b, Mode::Paint(Color::BLACK), None, None, &[[110.0, 110.0, 1.0]]);
        assert_eq!(moved.get(10, 10)[3], 255);
    }

    #[test]
    fn picture_tips() {
        let tip = Arc::new(TipImage { name: "square".into(), width: 4, height: 4, data: vec![255; 16] });
        let mut r = Raster::transparent(40, 40);
        let b = Brush { size: 8.0, tip: Some("square".into()), ..Default::default() };
        paint(&mut r, (0, 0), &b, Mode::Paint(Color::BLACK), None, Some(tip), &[[20.0, 20.0, 1.0]]);
        assert_eq!(r.get(17, 17)[3], 255);
        assert_eq!(r.get(10, 10)[3], 0);
    }
}
