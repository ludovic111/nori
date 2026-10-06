//! `vector.*`: shapes and paths (Illustrator's side): add, restyle, edit nodes, path operations.

use std::sync::Arc;

use nori_core::vector::{Cap, FillRule, Geometry, Join, Node, Paint, Shape, Stroke, SubPath};
use nori_core::{Color, Content};
use serde_json::{Value, json};

use super::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    let tools = s.settings().tools;
    let fg = s.colors.read().foreground;
    match cx.spec.name {
        "vector.addShape" => s.edit(cx.label(), cx.source, None, |d| {
            let kind = a.str("shape")?.to_ascii_lowercase();
            let (x, y, w, h) = (a.f64("x")? as f32, a.f64("y")? as f32, a.f64("width")? as f32, a.f64("height")? as f32);
            let geometry = match kind.as_str() {
                "rect" | "rectangle" | "square" => Geometry::Rect { x: x.min(x + w), y: y.min(y + h), w: w.abs(), h: h.abs(), radius: a.opt_f64("radius").unwrap_or(0.0).max(0.0) as f32 },
                "ellipse" | "circle" => Geometry::Ellipse { cx: x + w / 2.0, cy: y + h / 2.0, rx: (w / 2.0).abs(), ry: (h / 2.0).abs() },
                "polygon" | "star" => Geometry::Polygon {
                    cx: x + w / 2.0,
                    cy: y + h / 2.0,
                    radius: (w.abs().min(h.abs())) / 2.0,
                    sides: a.opt_u32("sides").unwrap_or(if kind == "star" { 5 } else { 6 }).clamp(3, 200),
                    inner: (kind == "star").then(|| a.opt_f64("inner").unwrap_or(0.5).clamp(0.05, 1.0) as f32),
                    rotation: 0.0,
                },
                "line" => Geometry::Line { x1: x, y1: y, x2: x + w, y2: y + h },
                other => return Err(format!("`{other}` isn't a shape: rect, ellipse, polygon, star or line.")),
            };
            let default_fill = if kind == "line" { Paint::None } else { Paint::solid(Color::parse(&tools.shape_fill).unwrap_or(fg)) };
            let fill = match a.get("fill") {
                Some(v) => util::paint(v)?,
                None => default_fill,
            };
            let stroke = stroke_of(&a, &tools, kind == "line", fg)?;
            let base = match kind.as_str() {
                "rect" | "rectangle" | "square" => "Rectangle",
                "ellipse" | "circle" => "Ellipse",
                "polygon" => "Polygon",
                "star" => "Star",
                _ => "Line",
            };
            super::layer::add(d, &a, Content::Vector { shape: Shape::new(geometry, fill, stroke) }, base)
        }),
        "vector.addPath" => s.edit(cx.label(), cx.source, None, |d| {
            let geometry = match (a.array("subpaths"), a.opt_str("d")) {
                (Some(sp), _) => {
                    let subpaths: Vec<SubPath> = serde_json::from_value(Value::Array(sp.clone())).map_err(|e| format!("subpaths: {e}"))?;
                    Geometry::Path { subpaths }
                }
                (None, Some(dstr)) => parse_d(dstr)?,
                _ => return Err("Give subpaths or d.".into()),
            };
            let fill = match a.get("fill") {
                Some(v) => util::paint(v)?,
                None => Paint::None,
            };
            let mut stroke = stroke_of(&a, &tools, true, fg)?;
            if fill.is_none() && stroke.is_none() {
                stroke = Some(Stroke { paint: Paint::solid(fg), width: tools.shape_stroke_width as f32, ..Default::default() });
            }
            super::layer::add(d, &a, Content::Vector { shape: Shape::new(geometry, fill, stroke) }, "Path")
        }),
        "vector.update" => s.edit(cx.label(), cx.source, a.coalesce(), |d| {
            let id = util::layer_id(d, &a)?;
            let shape = shape_mut(d, &id)?;
            if let Some(v) = a.get("fill") {
                shape.fill = util::paint(v)?;
            }
            if let Some(v) = a.get("stroke") {
                let p = util::paint(v)?;
                if p.is_none() {
                    shape.stroke = None;
                } else {
                    shape.stroke.get_or_insert_with(Stroke::default).paint = p;
                }
            }
            if let Some(w) = a.opt_f64("strokeWidth") {
                if w <= 0.0 {
                    shape.stroke = None;
                } else {
                    stroke_mut(shape).width = w as f32;
                }
            }
            if let Some(c) = a.opt_str("cap") {
                let cap: Cap = serde_json::from_value(json!(c)).map_err(|_| "cap is butt, round or square")?;
                stroke_mut(shape).cap = cap;
            }
            if let Some(j) = a.opt_str("join") {
                let join: Join = serde_json::from_value(json!(j)).map_err(|_| "join is miter, round or bevel")?;
                stroke_mut(shape).join = join;
            }
            if let Some(dash) = a.array("dash") {
                let v: Vec<f32> = dash.iter().filter_map(Value::as_f64).map(|v| v as f32).collect();
                stroke_mut(shape).dash = v;
            }
            if let Some(r) = a.opt_str("fillRule") {
                shape.fill_rule = serde_json::from_value(json!(r)).map_err(|_| "fillRule is nonZero or evenOdd")?;
            }
            if let Some(r) = a.opt_f64("radius") {
                match &mut shape.geometry {
                    Geometry::Rect { radius, .. } => *radius = r.max(0.0) as f32,
                    _ => return Err("Only rectangles have a corner radius (live corners).".into()),
                }
            }
            Ok(json!({ "layerId": id, "shape": shape }))
        }),
        "vector.setGeometry" => s.edit(cx.label(), cx.source, a.coalesce(), |d| {
            let id = util::layer_id(d, &a)?;
            let g: Geometry = serde_json::from_value(Value::Object(a.object("geometry").cloned().unwrap_or_default())).map_err(|e| format!("geometry: {e}"))?;
            shape_mut(d, &id)?.geometry = g;
            Ok(json!({ "layerId": id }))
        }),
        "vector.toPath" => s.edit(cx.label(), cx.source, None, |d| {
            let id = util::layer_id(d, &a)?;
            let shape = shape_mut(d, &id)?;
            shape.geometry = shape.geometry.to_path();
            Ok(json!({ "layerId": id, "geometry": shape.geometry }))
        }),
        "vector.editNode" => s.edit(cx.label(), cx.source, a.coalesce(), |d| {
            let id = util::layer_id(d, &a)?;
            let shape = shape_mut(d, &id)?;
            if !matches!(shape.geometry, Geometry::Path { .. }) {
                shape.geometry = shape.geometry.to_path();
            }
            let Geometry::Path { subpaths } = &mut shape.geometry else { unreachable!() };
            let si = a.opt_u32("subpath").unwrap_or(0) as usize;
            let sp = subpaths.get_mut(si).ok_or_else(|| format!("There is no subpath {si}."))?;
            let i = a.opt_u32("index").unwrap_or(0) as usize;
            if i >= sp.nodes.len() {
                return Err(format!("Subpath {si} has {} nodes.", sp.nodes.len()));
            }
            if a.bool_or("delete", false) {
                sp.nodes.remove(i);
                return Ok(json!({ "layerId": id, "nodes": sp.nodes.len() }));
            }
            let n: &mut Node = &mut sp.nodes[i];
            let (ox, oy) = (n.x, n.y);
            if let Some(x) = a.opt_f64("x") {
                n.x = x as f32;
            }
            if let Some(y) = a.opt_f64("y") {
                n.y = y as f32;
            }
            // Handles follow their point when only the point moves.
            let (mx, my) = (n.x - ox, n.y - oy);
            for h in [&mut n.handle_in, &mut n.handle_out].into_iter().flatten() {
                h[0] += mx;
                h[1] += my;
            }
            let handle = |v: &Value| -> Option<Option<[f32; 2]>> {
                let arr = v.as_array()?;
                if arr.is_empty() {
                    return Some(None);
                }
                Some(Some([arr.first()?.as_f64()? as f32, arr.get(1)?.as_f64()? as f32]))
            };
            if let Some(h) = a.get("in").and_then(handle) {
                n.handle_in = h;
            }
            if let Some(h) = a.get("out").and_then(handle) {
                n.handle_out = h;
            }
            Ok(json!({ "layerId": id, "node": *n }))
        }),
        "vector.combine" => s.edit(cx.label(), cx.source, None, |d| {
            let ids: Vec<String> = a.strings("layerIds").iter().map(|k| d.resolve(k)).collect::<Result<_, _>>()?;
            if ids.len() < 2 {
                return Err("Give two vector layers or more.".into());
            }
            // Bottom first: the result takes the bottom layer's look and place.
            let mut ordered: Vec<(nori_core::Loc, String)> = ids.iter().filter_map(|id| Some((d.path_of(id)?, id.clone()))).collect();
            ordered.sort_by(|a, b| b.0.path.cmp(&a.0.path));
            let shapes: Vec<Shape> = ordered
                .iter()
                .map(|(_, id)| match d.layer(id).map(|l| &l.content) {
                    Some(Content::Vector { shape }) => Ok(shape.clone()),
                    _ => Err(format!("`{id}` isn't a vector layer.")),
                })
                .collect::<Result<_, _>>()?;
            let geometry = nori_render::shapes::boolean(&shapes, a.str("op")?)?;
            let bottom = ordered[0].1.clone();
            let mut look = shapes[0].clone();
            look.geometry = geometry;
            look.transform = nori_core::vector::IDENTITY;
            let keep = a.bool_or("keep", false);
            let id = d.new_id();
            let name = format!("{} ({})", d.layer(&bottom).map(|l| l.name.clone()).unwrap_or_default(), a.str("op")?);
            let top = ordered.last().map(|(_, id)| id.clone()).unwrap_or_default();
            d.insert_above(nori_core::Layer::new(id.clone(), name, Content::Vector { shape: look }), Some(&top));
            for (_, other) in &ordered {
                if keep {
                    if let Some(l) = d.layer_mut(other) {
                        l.visible = false;
                    }
                } else {
                    d.remove(other);
                }
            }
            d.active = Some(id.clone());
            Ok(json!({ "layerId": id }))
        }),
        _ => Err(super::unhandled(cx)),
    }
}

fn stroke_mut(shape: &mut Shape) -> &mut Stroke {
    shape.stroke.get_or_insert_with(|| Stroke { paint: Paint::solid(Color::BLACK), ..Default::default() })
}

fn shape_mut<'a>(d: &'a mut nori_core::Document, id: &str) -> CmdResult<&'a mut Shape> {
    let l = d.layer_mut(id).ok_or("no layer")?;
    util::unlocked(l)?;
    let name = l.name.clone();
    match &mut l.content {
        Content::Vector { shape } => Ok(shape),
        _ => Err(format!("`{name}` isn't a vector layer.")),
    }
}

fn stroke_of(a: &Args, tools: &crate::settings::ToolSettings, default_on: bool, fg: Color) -> CmdResult<Option<Stroke>> {
    let width = a.opt_f64("strokeWidth").unwrap_or(tools.shape_stroke_width) as f32;
    let paint = match a.get("stroke") {
        Some(v) => util::paint(v)?,
        None if !tools.shape_stroke.is_empty() => Paint::solid(Color::parse(&tools.shape_stroke).unwrap_or(fg)),
        None if default_on || a.has("strokeWidth") => Paint::solid(fg),
        None => Paint::None,
    };
    Ok((!paint.is_none() && width > 0.0).then(|| Stroke { paint, width, ..Default::default() }))
}

/// Parses an SVG path string (M, L, H, V, C, S, Q, T, Z, absolute and relative; arcs as
/// lines) into a path.
pub fn parse_d(d: &str) -> CmdResult<Geometry> {
    let mut toks: Vec<String> = vec![];
    let mut cur = String::new();
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() && ch != 'e' && ch != 'E' {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            toks.push(ch.to_string());
        } else if ch == ',' || ch.is_whitespace() {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
        } else if ch == '-' && !cur.is_empty() && !cur.ends_with(['e', 'E']) {
            toks.push(std::mem::take(&mut cur));
            cur.push(ch);
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    let mut subpaths: Vec<SubPath> = vec![];
    let (mut x, mut y) = (0f32, 0f32);
    let mut cmd = 'M';
    let mut last_ctrl: Option<[f32; 2]> = None;
    let mut i = 0;
    let num = |i: &mut usize| -> CmdResult<f32> {
        let t = toks.get(*i).ok_or("the path ends early")?;
        *i += 1;
        t.parse::<f32>().map_err(|_| format!("`{t}` isn't a number in the path"))
    };
    while i < toks.len() {
        if toks[i].chars().all(|c| c.is_ascii_alphabetic()) {
            cmd = toks[i].chars().next().unwrap_or('M');
            i += 1;
            if cmd == 'Z' || cmd == 'z' {
                if let Some(sp) = subpaths.last_mut() {
                    sp.closed = true;
                    if let Some(f) = sp.nodes.first() {
                        x = f.x;
                        y = f.y;
                    }
                }
                last_ctrl = None;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (bx, by) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                x = bx + num(&mut i)?;
                y = by + num(&mut i)?;
                subpaths.push(SubPath { nodes: vec![Node::corner(x, y)], closed: false });
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' | 'H' | 'V' | 'A' => {
                match cmd.to_ascii_uppercase() {
                    'L' => {
                        x = bx + num(&mut i)?;
                        y = by + num(&mut i)?;
                    }
                    'H' => x = bx + num(&mut i)?,
                    'V' => y = by + num(&mut i)?,
                    _ => {
                        for _ in 0..5 {
                            num(&mut i)?;
                        }
                        x = bx + num(&mut i)?;
                        y = by + num(&mut i)?;
                    }
                }
                if subpaths.is_empty() {
                    subpaths.push(SubPath::default());
                }
                subpaths.last_mut().expect("subpath").nodes.push(Node::corner(x, y));
                last_ctrl = None;
            }
            'C' | 'S' | 'Q' | 'T' => {
                let u = cmd.to_ascii_uppercase();
                let (c1, c2, p) = match u {
                    'C' => {
                        let c1 = [bx + num(&mut i)?, by + num(&mut i)?];
                        let c2 = [bx + num(&mut i)?, by + num(&mut i)?];
                        (c1, c2, [bx + num(&mut i)?, by + num(&mut i)?])
                    }
                    'S' => {
                        let c1 = last_ctrl.map_or([x, y], |c| [2.0 * x - c[0], 2.0 * y - c[1]]);
                        let c2 = [bx + num(&mut i)?, by + num(&mut i)?];
                        (c1, c2, [bx + num(&mut i)?, by + num(&mut i)?])
                    }
                    'Q' => {
                        let q = [bx + num(&mut i)?, by + num(&mut i)?];
                        let p = [bx + num(&mut i)?, by + num(&mut i)?];
                        ([x + 2.0 / 3.0 * (q[0] - x), y + 2.0 / 3.0 * (q[1] - y)], [p[0] + 2.0 / 3.0 * (q[0] - p[0]), p[1] + 2.0 / 3.0 * (q[1] - p[1])], p)
                    }
                    _ => {
                        let q = last_ctrl.map_or([x, y], |c| [2.0 * x - c[0], 2.0 * y - c[1]]);
                        let p = [bx + num(&mut i)?, by + num(&mut i)?];
                        ([x + 2.0 / 3.0 * (q[0] - x), y + 2.0 / 3.0 * (q[1] - y)], [p[0] + 2.0 / 3.0 * (q[0] - p[0]), p[1] + 2.0 / 3.0 * (q[1] - p[1])], p)
                    }
                };
                if subpaths.is_empty() {
                    subpaths.push(SubPath { nodes: vec![Node::corner(x, y)], closed: false });
                }
                let sp = subpaths.last_mut().expect("subpath");
                if let Some(n) = sp.nodes.last_mut() {
                    n.handle_out = Some(c1);
                }
                sp.nodes.push(Node { x: p[0], y: p[1], handle_in: Some(c2), handle_out: None });
                last_ctrl = Some(c2);
                x = p[0];
                y = p[1];
            }
            other => return Err(format!("Path command `{other}` isn't supported.")),
        }
    }
    if subpaths.is_empty() {
        return Err("The path draws nothing.".into());
    }
    let _ = FillRule::NonZero;
    Ok(Geometry::Path { subpaths })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_svg_path_strings() {
        let Geometry::Path { subpaths } = parse_d("M10 10 L 50,10 l0 40 h-40 Z M60 60 C 70 50 80 50 90 60 s 20 10 30 0").unwrap() else { panic!() };
        assert_eq!(subpaths.len(), 2);
        assert!(subpaths[0].closed);
        assert_eq!(subpaths[0].nodes.len(), 4);
        assert_eq!((subpaths[0].nodes[2].x, subpaths[0].nodes[2].y), (50.0, 50.0));
        assert_eq!(subpaths[1].nodes.len(), 3);
        assert_eq!(subpaths[1].nodes[2].x, 120.0);
        assert!(parse_d("M 1").is_err());
        assert!(parse_d("").is_err());
    }
}
