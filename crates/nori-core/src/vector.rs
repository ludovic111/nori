//! Vector content (Illustrator's side of nori): shapes and Bézier paths with fills, strokes and
//! gradients. Coordinates are document pixels as `f32`, so vectors stay sharp at any output
//! resolution: they are only turned into pixels when the picture is drawn or exported.

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::raster::Rect;

/// One point of a path with its Bézier handles (absolute positions; none: a sharp corner on
/// that side).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub x: f32,
    pub y: f32,
    /// The handle shaping the curve coming into this point.
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    pub handle_in: Option<[f32; 2]>,
    /// The handle shaping the curve leaving this point.
    #[serde(default, rename = "out", skip_serializing_if = "Option::is_none")]
    pub handle_out: Option<[f32; 2]>,
}

impl Node {
    pub fn corner(x: f32, y: f32) -> Self {
        Self { x, y, handle_in: None, handle_out: None }
    }

    pub fn smooth(x: f32, y: f32, handle_in: [f32; 2], handle_out: [f32; 2]) -> Self {
        Self { x, y, handle_in: Some(handle_in), handle_out: Some(handle_out) }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubPath {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub closed: bool,
}

/// One drawing instruction, in absolute coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move([f32; 2]),
    Line([f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

/// What a vector layer draws: a live shape (its parameters stay editable) or a free path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Geometry {
    /// A rectangle with live corners of `radius`.
    #[serde(rename_all = "camelCase")]
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(default)]
        radius: f32,
    },
    #[serde(rename_all = "camelCase")]
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    /// A regular polygon, or a star when `inner` (the inner radius, 0..1 of `radius`) is set.
    #[serde(rename_all = "camelCase")]
    Polygon {
        cx: f32,
        cy: f32,
        radius: f32,
        sides: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inner: Option<f32>,
        /// Degrees; 0 puts a point at the top.
        #[serde(default)]
        rotation: f32,
    },
    #[serde(rename_all = "camelCase")]
    Line { x1: f32, y1: f32, x2: f32, y2: f32 },
    #[serde(rename_all = "camelCase")]
    Path { subpaths: Vec<SubPath> },
}

const KAPPA: f32 = 0.552_284_8;

impl Geometry {
    /// The shape as path segments.
    pub fn segments(&self) -> Vec<Seg> {
        match self {
            Geometry::Rect { x, y, w, h, radius } => {
                let r = radius.max(0.0).min(w.abs() / 2.0).min(h.abs() / 2.0);
                let (x0, y0, x1, y1) = (*x, *y, x + w, y + h);
                if r <= 0.0 {
                    return vec![Seg::Move([x0, y0]), Seg::Line([x1, y0]), Seg::Line([x1, y1]), Seg::Line([x0, y1]), Seg::Close];
                }
                let k = r * KAPPA;
                vec![
                    Seg::Move([x0 + r, y0]),
                    Seg::Line([x1 - r, y0]),
                    Seg::Cubic([x1 - r + k, y0], [x1, y0 + r - k], [x1, y0 + r]),
                    Seg::Line([x1, y1 - r]),
                    Seg::Cubic([x1, y1 - r + k], [x1 - r + k, y1], [x1 - r, y1]),
                    Seg::Line([x0 + r, y1]),
                    Seg::Cubic([x0 + r - k, y1], [x0, y1 - r + k], [x0, y1 - r]),
                    Seg::Line([x0, y0 + r]),
                    Seg::Cubic([x0, y0 + r - k], [x0 + r - k, y0], [x0 + r, y0]),
                    Seg::Close,
                ]
            }
            Geometry::Ellipse { cx, cy, rx, ry } => {
                let (kx, ky) = (rx * KAPPA, ry * KAPPA);
                vec![
                    Seg::Move([cx + rx, *cy]),
                    Seg::Cubic([cx + rx, cy + ky], [cx + kx, cy + ry], [*cx, cy + ry]),
                    Seg::Cubic([cx - kx, cy + ry], [cx - rx, cy + ky], [cx - rx, *cy]),
                    Seg::Cubic([cx - rx, cy - ky], [cx - kx, cy - ry], [*cx, cy - ry]),
                    Seg::Cubic([cx + kx, cy - ry], [cx + rx, cy - ky], [cx + rx, *cy]),
                    Seg::Close,
                ]
            }
            Geometry::Polygon { cx, cy, radius, sides, inner, rotation } => {
                let n = (*sides).clamp(3, 200) as usize;
                let pts = if inner.is_some() { n * 2 } else { n };
                let mut out = vec![];
                for i in 0..pts {
                    let r = match inner {
                        Some(k) if i % 2 == 1 => radius * k.clamp(0.01, 1.0),
                        _ => *radius,
                    };
                    let a = (rotation - 90.0).to_radians() + std::f32::consts::TAU * i as f32 / pts as f32;
                    let p = [cx + r * a.cos(), cy + r * a.sin()];
                    out.push(if i == 0 { Seg::Move(p) } else { Seg::Line(p) });
                }
                out.push(Seg::Close);
                out
            }
            Geometry::Line { x1, y1, x2, y2 } => vec![Seg::Move([*x1, *y1]), Seg::Line([*x2, *y2])],
            Geometry::Path { subpaths } => {
                let mut out = vec![];
                for sp in subpaths {
                    let Some(first) = sp.nodes.first() else { continue };
                    out.push(Seg::Move([first.x, first.y]));
                    let n = sp.nodes.len();
                    let edges = if sp.closed { n } else { n - 1 };
                    for i in 0..edges {
                        let a = sp.nodes[i];
                        let b = sp.nodes[(i + 1) % n];
                        match (a.handle_out, b.handle_in) {
                            (None, None) => out.push(Seg::Line([b.x, b.y])),
                            (h1, h2) => out.push(Seg::Cubic(h1.unwrap_or([a.x, a.y]), h2.unwrap_or([b.x, b.y]), [b.x, b.y])),
                        }
                    }
                    if sp.closed {
                        out.push(Seg::Close);
                    }
                }
                out
            }
        }
    }

    /// The shape as an editable path (what the direct-selection tool works on).
    pub fn to_path(&self) -> Geometry {
        if let Geometry::Path { .. } = self {
            return self.clone();
        }
        let mut subpaths: Vec<SubPath> = vec![];
        for s in self.segments() {
            match s {
                Seg::Move(p) => subpaths.push(SubPath { nodes: vec![Node::corner(p[0], p[1])], closed: false }),
                Seg::Line(p) => {
                    if let Some(sp) = subpaths.last_mut() {
                        sp.nodes.push(Node::corner(p[0], p[1]));
                    }
                }
                Seg::Cubic(c1, c2, p) => {
                    if let Some(sp) = subpaths.last_mut() {
                        if let Some(last) = sp.nodes.last_mut() {
                            last.handle_out = Some(c1);
                        }
                        sp.nodes.push(Node { x: p[0], y: p[1], handle_in: Some(c2), handle_out: None });
                    }
                }
                Seg::Close => {
                    if let Some(sp) = subpaths.last_mut() {
                        sp.closed = true;
                        // The closing point repeats the first: fold its handle into the first.
                        if sp.nodes.len() > 1 {
                            let (f, l) = (sp.nodes[0], *sp.nodes.last().expect("not empty"));
                            if (f.x - l.x).abs() < 1e-3 && (f.y - l.y).abs() < 1e-3 {
                                let last = sp.nodes.pop().expect("not empty");
                                sp.nodes[0].handle_in = last.handle_in;
                            }
                        }
                    }
                }
            }
        }
        Geometry::Path { subpaths }
    }

    /// Moved by (dx, dy).
    pub fn translate(&mut self, dx: f32, dy: f32) {
        match self {
            Geometry::Rect { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Geometry::Ellipse { cx, cy, .. } | Geometry::Polygon { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            Geometry::Line { x1, y1, x2, y2 } => {
                *x1 += dx;
                *y1 += dy;
                *x2 += dx;
                *y2 += dy;
            }
            Geometry::Path { subpaths } => {
                for n in subpaths.iter_mut().flat_map(|s| s.nodes.iter_mut()) {
                    n.x += dx;
                    n.y += dy;
                    if let Some(h) = &mut n.handle_in {
                        h[0] += dx;
                        h[1] += dy;
                    }
                    if let Some(h) = &mut n.handle_out {
                        h[0] += dx;
                        h[1] += dy;
                    }
                }
            }
        }
    }

    /// The box around the shape (control points included, so a little generous on curves).
    pub fn bounds(&self) -> [f32; 4] {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut add = |p: [f32; 2]| {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        };
        for s in self.segments() {
            match s {
                Seg::Move(p) | Seg::Line(p) => add(p),
                Seg::Cubic(a, b, p) => {
                    add(a);
                    add(b);
                    add(p);
                }
                Seg::Close => {}
            }
        }
        if x0 > x1 { [0.0; 4] } else { [x0, y0, x1 - x0, y1 - y0] }
    }
}

/// Applies an affine matrix `[a, b, c, d, e, f]` (x' = a·x + c·y + e, y' = b·x + d·y + f).
pub fn apply(m: &[f32; 6], p: [f32; 2]) -> [f32; 2] {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

pub const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    pub offset: f32,
    pub color: Color,
}

/// How a shape is filled or stroked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Paint {
    #[default]
    None,
    #[serde(rename_all = "camelCase")]
    Solid { color: Color },
    #[serde(rename_all = "camelCase")]
    Linear { x1: f32, y1: f32, x2: f32, y2: f32, stops: Vec<Stop> },
    #[serde(rename_all = "camelCase")]
    Radial { cx: f32, cy: f32, r: f32, stops: Vec<Stop> },
}

impl Paint {
    pub fn solid(c: Color) -> Self {
        Paint::Solid { color: c }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Paint::None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Cap {
    #[default]
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Join {
    #[default]
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Stroke {
    pub paint: Paint,
    pub width: f32,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f32,
    /// Dash and gap lengths, repeating; empty for a solid line.
    pub dash: Vec<f32>,
}

impl Default for Stroke {
    fn default() -> Self {
        Self { paint: Paint::solid(Color::BLACK), width: 2.0, cap: Cap::Butt, join: Join::Miter, miter_limit: 4.0, dash: vec![] }
    }
}

/// A vector layer's content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub geometry: Geometry,
    #[serde(default)]
    pub fill: Paint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    #[serde(default)]
    pub fill_rule: FillRule,
    /// Applied to the geometry when drawn (rotation and scale stay editable).
    #[serde(default = "identity", skip_serializing_if = "is_identity")]
    pub transform: [f32; 6],
}

fn identity() -> [f32; 6] {
    IDENTITY
}

fn is_identity(m: &[f32; 6]) -> bool {
    *m == IDENTITY
}

impl Shape {
    pub fn new(geometry: Geometry, fill: Paint, stroke: Option<Stroke>) -> Self {
        Self { geometry, fill, stroke, fill_rule: FillRule::NonZero, transform: IDENTITY }
    }

    /// Where it draws on the page (transform and stroke included), in whole pixels.
    pub fn bounds(&self) -> Rect {
        let [x, y, w, h] = self.geometry.bounds();
        let corners = [[x, y], [x + w, y], [x, y + h], [x + w, y + h]].map(|p| apply(&self.transform, p));
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in corners {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        let pad = self.stroke.as_ref().map_or(0.0, |s| s.width * s.miter_limit.max(1.0) / 2.0) + 2.0;
        Rect::new((x0 - pad).floor() as i32, (y0 - pad).floor() as i32, (x1 - x0 + 2.0 * pad).ceil().max(1.0) as u32, (y1 - y0 + 2.0 * pad).ceil().max(1.0) as u32)
    }

    /// The segments with the transform applied.
    pub fn transformed(&self) -> Vec<Seg> {
        let m = &self.transform;
        self.geometry
            .segments()
            .into_iter()
            .map(|s| match s {
                Seg::Move(p) => Seg::Move(apply(m, p)),
                Seg::Line(p) => Seg::Line(apply(m, p)),
                Seg::Cubic(a, b, p) => Seg::Cubic(apply(m, a), apply(m, b), apply(m, p)),
                Seg::Close => Seg::Close,
            })
            .collect()
    }
}

/// Flattens segments into polygons (one per subpath), curves cut into pieces no more than
/// `tolerance` pixels off.
pub fn flatten(segs: &[Seg], tolerance: f32) -> Vec<Vec<[f32; 2]>> {
    let mut out: Vec<Vec<[f32; 2]>> = vec![];
    let mut cur: Vec<[f32; 2]> = vec![];
    for s in segs {
        match *s {
            Seg::Move(p) => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur = vec![p];
            }
            Seg::Line(p) => cur.push(p),
            Seg::Cubic(a, b, p) => {
                let p0 = *cur.last().unwrap_or(&p);
                let len = dist(p0, a) + dist(a, b) + dist(b, p);
                let n = ((len / tolerance.max(0.05)).sqrt().ceil() as usize).clamp(2, 256);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    let q = [
                        u * u * u * p0[0] + 3.0 * u * u * t * a[0] + 3.0 * u * t * t * b[0] + t * t * t * p[0],
                        u * u * u * p0[1] + 3.0 * u * u * t * a[1] + 3.0 * u * t * t * b[1] + t * t * t * p[1],
                    ];
                    cur.push(q);
                }
            }
            Seg::Close => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_become_paths_and_back() {
        let r = Geometry::Rect { x: 10.0, y: 20.0, w: 100.0, h: 50.0, radius: 10.0 };
        let p = r.to_path();
        let Geometry::Path { subpaths } = &p else { panic!() };
        assert_eq!(subpaths.len(), 1);
        assert!(subpaths[0].closed);
        assert_eq!(subpaths[0].nodes.len(), 8);
        // Drawn the same either way.
        let a = flatten(&r.segments(), 0.25);
        let b = flatten(&p.segments(), 0.25);
        assert_eq!(a.len(), b.len());
        let [x, y, w, h] = p.bounds();
        assert!((x - 10.0).abs() < 1e-3 && (y - 20.0).abs() < 1e-3 && (w - 100.0).abs() < 1e-3 && (h - 50.0).abs() < 1e-3);
        let star = Geometry::Polygon { cx: 0.0, cy: 0.0, radius: 10.0, sides: 5, inner: Some(0.5), rotation: 0.0 };
        assert_eq!(star.segments().len(), 11);
        // The first point of a polygon is at the top.
        assert_eq!(star.segments()[0], Seg::Move([0.0, -10.0]));
    }

    #[test]
    fn json_is_readable() {
        let s = Shape::new(Geometry::Ellipse { cx: 5.0, cy: 5.0, rx: 4.0, ry: 2.0 }, Paint::solid(Color::WHITE), Some(Stroke::default()));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["geometry"]["type"], "ellipse");
        assert_eq!(v["fill"]["type"], "solid");
        assert_eq!(v["fill"]["color"], "#ffffff");
        assert!(v.get("transform").is_none());
        let back: Shape = serde_json::from_value(v).unwrap();
        assert_eq!(back, s);
        let b = s.bounds();
        assert!(b.x <= 1 && b.right() >= 9);
    }
}
