//! Building selections: rectangles, ellipses, polygons (lasso), and how a new shape combines
//! with the selection there is (replace, add, subtract, intersect).

use serde::{Deserialize, Serialize};

use crate::raster::{Mask, Rect, TILE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Combine {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

impl Combine {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "replace" | "new" => Ok(Combine::Replace),
            "add" | "union" => Ok(Combine::Add),
            "subtract" | "remove" => Ok(Combine::Subtract),
            "intersect" => Ok(Combine::Intersect),
            _ => Err(format!("`{s}` isn't a way to combine selections: replace, add, subtract or intersect")),
        }
    }
}

/// Coverage of a pixel by a shape, 0..255, from `inside(x, y) -> 0..1` sampled 4×4 in the pixel
/// (smooth edges), with a quick path for pixels well inside or outside.
fn rasterize(width: u32, height: u32, bounds: Rect, antialias: bool, inside: impl Fn(f32, f32) -> f32 + Sync) -> Mask {
    let mut m = Mask::filled(width, height, 0);
    let area = bounds.intersect(&m.bounds());
    if area.is_empty() {
        return m;
    }
    let mut row = vec![0u8; area.w as usize];
    let mut data = Vec::with_capacity((area.w * area.h) as usize);
    for y in area.y..area.bottom() {
        for (i, x) in (area.x..area.right()).enumerate() {
            let v = if antialias {
                let mut s = 0.0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        s += inside(x as f32 + (sx as f32 + 0.5) / 4.0, y as f32 + (sy as f32 + 0.5) / 4.0);
                    }
                }
                s / 16.0
            } else {
                inside(x as f32 + 0.5, y as f32 + 0.5)
            };
            row[i] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        data.extend_from_slice(&row);
    }
    m.write_rect(area, &data);
    m.compact();
    m
}

pub fn rect(width: u32, height: u32, r: Rect) -> Mask {
    let mut m = Mask::filled(width, height, 0);
    let area = r.intersect(&m.bounds());
    if !area.is_empty() {
        m.write_rect(area, &vec![255u8; (area.w * area.h) as usize]);
    }
    m
}

pub fn ellipse(width: u32, height: u32, r: Rect, antialias: bool) -> Mask {
    let (cx, cy) = (r.x as f32 + r.w as f32 / 2.0, r.y as f32 + r.h as f32 / 2.0);
    let (rx, ry) = ((r.w as f32 / 2.0).max(0.5), (r.h as f32 / 2.0).max(0.5));
    rasterize(width, height, r, antialias, |x, y| {
        let (dx, dy) = ((x - cx) / rx, (y - cy) / ry);
        if dx * dx + dy * dy <= 1.0 { 1.0 } else { 0.0 }
    })
}

/// A closed polygon (the lasso), even-odd fill.
pub fn polygon(width: u32, height: u32, points: &[[f32; 2]], antialias: bool) -> Mask {
    if points.len() < 3 {
        return Mask::filled(width, height, 0);
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let bounds = Rect::new(x0.floor() as i32, y0.floor() as i32, (x1.ceil() - x0.floor()) as u32 + 1, (y1.ceil() - y0.floor()) as u32 + 1);
    rasterize(width, height, bounds, antialias, |x, y| {
        let mut inside = false;
        let mut j = points.len() - 1;
        for i in 0..points.len() {
            let (pi, pj) = (points[i], points[j]);
            if (pi[1] > y) != (pj[1] > y) && x < (pj[0] - pi[0]) * (y - pi[1]) / (pj[1] - pi[1]) + pi[0] {
                inside = !inside;
            }
            j = i;
        }
        if inside { 1.0 } else { 0.0 }
    })
}

/// Combines `shape` with `current` (none: nothing selected yet).
pub fn combine(current: Option<&Mask>, shape: Mask, how: Combine) -> Mask {
    let (w, h) = (shape.width(), shape.height());
    let base = match current {
        Some(c) => c.clone(),
        None => Mask::filled(w, h, 0),
    };
    if how == Combine::Replace {
        return shape;
    }
    let op = |a: u8, b: u8| -> u8 {
        match how {
            Combine::Replace => b,
            Combine::Add => a.max(b),
            Combine::Subtract => ((a as u16 * (255 - b) as u16) / 255) as u8,
            Combine::Intersect => ((a as u16 * b as u16) / 255) as u8,
        }
    };
    let fill = op(base.fill()[0], shape.fill()[0]);
    let mut out = Mask::filled(w, h, fill);
    let (gx, gy) = out.grid();
    for ty in 0..gy {
        for tx in 0..gx {
            if base.tile(tx, ty).is_none() && shape.tile(tx, ty).is_none() {
                continue;
            }
            let r = out.tile_rect(tx, ty);
            let a = base.read_rect(r);
            let b = shape.read_rect(r);
            let data: Vec<u8> = a.iter().zip(b.iter()).map(|(x, y)| op(*x, *y)).collect();
            out.write_rect(r, &data);
        }
    }
    out.compact();
    out
}

/// Whether a selection selects nothing at all (then commands act on everything, as with none).
pub fn is_empty(m: &Mask) -> bool {
    m.is_empty()
}

/// The selection grown (positive) or shrunk (negative) by `by` pixels (square neighbourhood,
/// separable max/min: fast at any size).
pub fn grow(m: &Mask, by: i32) -> Mask {
    if by == 0 {
        return m.clone();
    }
    let (w, h) = (m.width() as usize, m.height() as usize);
    let src = m.to_vec();
    let r = by.unsigned_abs() as usize;
    let pick = |a: u8, b: u8| if by > 0 { a.max(b) } else { a.min(b) };
    let mut tmp = vec![0u8; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        for x in 0..w {
            let (lo, hi) = (x.saturating_sub(r), (x + r).min(w - 1));
            tmp[y * w + x] = row[lo..=hi].iter().copied().fold(row[x], pick);
        }
    }
    let mut out = vec![0u8; w * h];
    for x in 0..w {
        for y in 0..h {
            let (lo, hi) = (y.saturating_sub(r), (y + r).min(h - 1));
            out[y * w + x] = (lo..=hi).map(|yy| tmp[yy * w + x]).fold(tmp[y * w + x], pick);
        }
    }
    let _ = TILE;
    Mask::from_vec(m.width(), m.height(), m.fill(), &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_and_combinations() {
        let a = rect(100, 100, Rect::new(10, 10, 20, 20));
        assert_eq!(a.get(10, 10), [255]);
        assert_eq!(a.get(30, 30), [0]);
        let e = ellipse(100, 100, Rect::new(0, 0, 40, 40), true);
        assert_eq!(e.get(20, 20), [255]);
        assert_eq!(e.get(0, 0), [0]);
        let edge = e.get(20, 0)[0];
        assert!(edge > 0 && edge < 255, "antialiased edge: {edge}");
        let both = combine(Some(&a), e.clone(), Combine::Add);
        assert_eq!(both.get(25, 25), [255]);
        let cut = combine(Some(&e), a.clone(), Combine::Subtract);
        assert_eq!(cut.get(20, 20), [0]);
        assert_eq!(cut.get(5, 20), [255]);
        let inter = combine(Some(&e), a, Combine::Intersect);
        assert_eq!(inter.get(5, 20), [0]);
        assert_eq!(inter.get(20, 20), [255]);
        let tri = polygon(50, 50, &[[0.0, 0.0], [40.0, 0.0], [0.0, 40.0]], false);
        assert_eq!(tri.get(5, 5), [255]);
        assert_eq!(tri.get(35, 35), [0]);
    }

    #[test]
    fn grow_and_shrink() {
        let a = rect(50, 50, Rect::new(20, 20, 10, 10));
        let g = grow(&a, 3);
        assert_eq!(g.get(17, 17), [255]);
        assert_eq!(g.get(16, 20), [0]);
        let s = grow(&a, -3);
        assert_eq!(s.get(22, 22), [0]);
        assert_eq!(s.get(25, 25), [255]);
    }
}
