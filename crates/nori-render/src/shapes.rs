//! Vector layers drawn: shapes and paths filled and stroked with tiny-skia (solid colours and
//! gradients, dashes, caps and joins), at any scale.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use nori_core::vector::{Cap, FillRule, Join, Paint, Seg, Shape, Stop};
use nori_core::Rect;
use tiny_skia::{GradientStop, LinearGradient, PathBuilder, Pixmap, Point, RadialGradient, SpreadMode, Transform};

use crate::text::TextImage;

/// The shape's path in tiny-skia's terms, scaled by `scale`.
pub fn path(segs: &[Seg], scale: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let s = |p: [f32; 2]| (p[0] * scale, p[1] * scale);
    for seg in segs {
        match *seg {
            Seg::Move(p) => {
                let (x, y) = s(p);
                pb.move_to(x, y)
            }
            Seg::Line(p) => {
                let (x, y) = s(p);
                pb.line_to(x, y)
            }
            Seg::Cubic(a, b, p) => {
                let ((ax, ay), (bx, by), (x, y)) = (s(a), s(b), s(p));
                pb.cubic_to(ax, ay, bx, by, x, y)
            }
            Seg::Close => pb.close(),
        }
    }
    pb.finish()
}

fn stops(list: &[Stop]) -> Vec<GradientStop> {
    let mut v: Vec<GradientStop> = list
        .iter()
        .map(|s| {
            let [r, g, b, a] = s.color.to_u8();
            GradientStop::new(s.offset.clamp(0.0, 1.0), tiny_skia::Color::from_rgba8(r, g, b, a))
        })
        .collect();
    if v.len() == 1 {
        v.push(v[0]);
    }
    v
}

/// A tiny-skia paint for a fill or stroke paint, in a pixmap whose origin is page (ox, oy).
pub fn skia_paint(p: &Paint, scale: f32, ox: f32, oy: f32) -> Option<tiny_skia::Paint<'static>> {
    let mut paint = tiny_skia::Paint { anti_alias: true, ..Default::default() };
    let at = |x: f32, y: f32| Point::from_xy(x * scale - ox, y * scale - oy);
    match p {
        Paint::None => return None,
        Paint::Solid { color } => {
            let [r, g, b, a] = color.to_u8();
            paint.set_color_rgba8(r, g, b, a);
        }
        Paint::Linear { x1, y1, x2, y2, stops: s } => {
            paint.shader = LinearGradient::new(at(*x1, *y1), at(*x2, *y2), stops(s), SpreadMode::Pad, Transform::identity())?;
        }
        Paint::Radial { cx, cy, r, stops: s } => {
            paint.shader = RadialGradient::new(at(*cx, *cy), at(*cx, *cy), r * scale, stops(s), SpreadMode::Pad, Transform::identity())?;
        }
    }
    Some(paint)
}

/// The shape drawn at `scale` (1: document pixels) into a picture covering its bounds.
pub fn draw(shape: &Shape, scale: f32) -> TextImage {
    let b = shape.bounds();
    let rect = Rect::new((b.x as f32 * scale).floor() as i32, (b.y as f32 * scale).floor() as i32, ((b.w as f32 * scale).ceil() as u32 + 2).min(20_000), ((b.h as f32 * scale).ceil() as u32 + 2).min(20_000));
    let Some(mut pm) = Pixmap::new(rect.w.max(1), rect.h.max(1)) else { return TextImage::default() };
    let segs = shape.transformed();
    let Some(p) = path(&segs, scale) else { return TextImage { rect, rgba: vec![], overflow: false } };
    let to = Transform::from_translate(-rect.x as f32, -rect.y as f32);
    let (ox, oy) = (0.0, 0.0);
    let rule = if shape.fill_rule == FillRule::EvenOdd { tiny_skia::FillRule::EvenOdd } else { tiny_skia::FillRule::Winding };
    if let Some(paint) = skia_paint(&shape.fill, scale, ox, oy) {
        pm.fill_path(&p, &paint, rule, to, None);
    }
    if let Some(st) = &shape.stroke
        && st.width > 0.0
        && let Some(paint) = skia_paint(&st.paint, scale, ox, oy)
    {
        let stroke = tiny_skia::Stroke {
            width: st.width * scale,
            miter_limit: st.miter_limit.max(1.0),
            line_cap: match st.cap {
                Cap::Butt => tiny_skia::LineCap::Butt,
                Cap::Round => tiny_skia::LineCap::Round,
                Cap::Square => tiny_skia::LineCap::Square,
            },
            line_join: match st.join {
                Join::Miter => tiny_skia::LineJoin::Miter,
                Join::Round => tiny_skia::LineJoin::Round,
                Join::Bevel => tiny_skia::LineJoin::Bevel,
            },
            dash: if st.dash.len() >= 2 && st.dash.iter().all(|d| *d > 0.0) {
                let mut d: Vec<f32> = st.dash.iter().map(|v| v * scale).collect();
                if d.len() % 2 == 1 {
                    d.extend(d.clone());
                }
                tiny_skia::StrokeDash::new(d, 0.0)
            } else {
                None
            },
        };
        pm.stroke_path(&p, &paint, &stroke, to, None);
    }
    let mut rgba = pm.take();
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    TextImage { rect, rgba, overflow: false }
}

/// [`draw`] at document scale, remembered.
pub fn render(shape: &Shape) -> Arc<TextImage> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<TextImage>>>> = OnceLock::new();
    let key = serde_json::to_string(shape).unwrap_or_default();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(i) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return i.clone();
    }
    let img = Arc::new(draw(shape, 1.0));
    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > 512 {
        map.clear();
    }
    map.insert(key, img.clone());
    img
}

/// Whether page point (x, y) is on the shape's fill or stroke (the direct-selection tool's
/// hit test), within `slop` pixels.
pub fn hit(shape: &Shape, x: f32, y: f32, slop: f32) -> bool {
    let img = render(shape);
    let r = slop.ceil() as i32;
    (-r..=r).any(|dy| (-r..=r).any(|dx| img.get(x as i32 + dx, y as i32 + dy)[3] > 24))
}

/// Combines shapes into one path: `union`, `subtract` (the first minus the others),
/// `intersect` or `exclude`. Curves are followed to within a tenth of a pixel; the result is a
/// path of corner points (in page coordinates, transforms applied).
pub fn boolean(shapes: &[Shape], op: &str) -> Result<nori_core::Geometry, String> {
    use i_overlay::core::fill_rule::FillRule as F;
    use i_overlay::core::overlay_rule::OverlayRule as R;
    use i_overlay::float::single::SingleFloatOverlay;
    let rule = match op.to_ascii_lowercase().as_str() {
        "union" | "unite" | "add" => R::Union,
        "subtract" | "minus" | "minusfront" | "difference" => R::Difference,
        "intersect" | "intersection" => R::Intersect,
        "exclude" | "xor" => R::Xor,
        other => return Err(format!("`{other}` isn't a path operation: union, subtract, intersect or exclude")),
    };
    if shapes.len() < 2 {
        return Err("A path operation needs two shapes or more.".into());
    }
    let contours = |s: &Shape| -> Vec<Vec<[f64; 2]>> {
        nori_core::vector::flatten(&s.transformed(), 0.1).into_iter().map(|c| c.into_iter().map(|p| [p[0] as f64, p[1] as f64]).collect()).collect()
    };
    let mut acc: Vec<Vec<[f64; 2]>> = contours(&shapes[0]);
    for s in &shapes[1..] {
        let next = contours(s);
        let out = acc.overlay(&next, rule, F::NonZero);
        acc = out.into_iter().flatten().collect();
    }
    let subpaths = acc
        .into_iter()
        .filter(|c| c.len() >= 3)
        .map(|c| nori_core::vector::SubPath { nodes: c.into_iter().map(|p| nori_core::vector::Node::corner(p[0] as f32, p[1] as f32)).collect(), closed: true })
        .collect();
    Ok(nori_core::Geometry::Path { subpaths })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_core::vector::{Geometry, Stroke};
    use nori_core::Color;

    #[test]
    fn fills_strokes_and_gradients() {
        let s = Shape::new(Geometry::Rect { x: 10.0, y: 10.0, w: 40.0, h: 20.0, radius: 0.0 }, Paint::solid(Color::rgb(1.0, 0.0, 0.0)), None);
        let img = render(&s);
        assert_eq!(img.get(30, 20), [255, 0, 0, 255]);
        assert_eq!(img.get(5, 5)[3], 0);
        let mut ring = Shape::new(Geometry::Ellipse { cx: 50.0, cy: 50.0, rx: 30.0, ry: 30.0 }, Paint::None, Some(Stroke { width: 4.0, ..Default::default() }));
        let img = render(&ring);
        assert_eq!(img.get(50, 50)[3], 0);
        assert!(img.get(80, 50)[3] > 200);
        ring.fill = Paint::Linear { x1: 20.0, y1: 0.0, x2: 80.0, y2: 0.0, stops: vec![Stop { offset: 0.0, color: Color::BLACK }, Stop { offset: 1.0, color: Color::WHITE }] };
        let img = draw(&ring, 1.0);
        assert!(img.get(30, 50)[0] < img.get(70, 50)[0]);
        // Twice the scale: twice the pixels.
        let big = draw(&s, 2.0);
        assert!(big.rect.w >= 80);
        assert!(hit(&s, 12.0, 12.0, 0.0) && !hit(&s, 70.0, 70.0, 1.0));
    }

    #[test]
    fn path_operations() {
        let a = Shape::new(Geometry::Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0, radius: 0.0 }, Paint::solid(Color::BLACK), None);
        let b = Shape::new(Geometry::Rect { x: 5.0, y: 0.0, w: 10.0, h: 10.0, radius: 0.0 }, Paint::solid(Color::BLACK), None);
        let area = |g: &Geometry| -> f32 {
            nori_core::vector::flatten(&g.segments(), 0.1).iter().map(|c| {
                let mut s = 0.0;
                for i in 0..c.len() {
                    let (p, q) = (c[i], c[(i + 1) % c.len()]);
                    s += p[0] * q[1] - q[0] * p[1];
                }
                (s / 2.0).abs()
            }).sum()
        };
        assert!((area(&boolean(&[a.clone(), b.clone()], "union").unwrap()) - 150.0).abs() < 1.0);
        assert!((area(&boolean(&[a.clone(), b.clone()], "intersect").unwrap()) - 50.0).abs() < 1.0);
        assert!((area(&boolean(&[a.clone(), b.clone()], "subtract").unwrap()) - 50.0).abs() < 1.0);
        assert!(boolean(&[a], "union").is_err());
    }
}
