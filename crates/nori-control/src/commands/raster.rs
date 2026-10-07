//! `raster.*` and `brushes.*`: painting pixels: brush and eraser strokes, the paint bucket,
//! gradients, clearing, the eyedropper; brush tips.

use std::sync::Arc;

use nori_core::{Color, Rect};
use nori_render::brush::{Brush, Mode};
use serde_json::{Value, json};

use super::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "raster.stroke" => {
            let erase = a.bool_or("erase", false);
            let tools = s.settings().tools;
            let base = if erase { tools.eraser } else { tools.brush };
            let brush = Brush {
                size: a.opt_f64("size").map_or(base.size, |v| v as f32),
                hardness: a.opt_f64("hardness").map_or(base.hardness, |v| v as f32),
                opacity: a.opt_f64("opacity").map_or(base.opacity, |v| v as f32),
                flow: a.opt_f64("flow").map_or(base.flow, |v| v as f32),
                spacing: a.opt_f64("spacing").map_or(base.spacing, |v| v as f32),
                pressure_size: a.bool_or("pressureSize", base.pressure_size),
                pressure_opacity: a.bool_or("pressureOpacity", base.pressure_opacity),
                tip: a.opt_str("tip").map(str::to_string).or(base.tip.clone()),
            }
            .clamped();
            let color = util::color(&a, "color", s.colors.read().foreground)?;
            let points: Vec<[f32; 3]> = a
                .array("points")
                .ok_or("points is required")?
                .iter()
                .map(|p| {
                    let v: Vec<f64> = p.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
                    match v.as_slice() {
                        [x, y] => Ok([*x as f32, *y as f32, 1.0]),
                        [x, y, pr, ..] => Ok([*x as f32, *y as f32, *pr as f32]),
                        _ => Err("Each point is [x, y] or [x, y, pressure].".to_string()),
                    }
                })
                .collect::<Result<_, _>>()?;
            if points.is_empty() {
                return Err("A stroke needs points.".into());
            }
            let tip = match &brush.tip {
                Some(name) => Some(s.brushes.read().iter().find(|t| t.name.eq_ignore_ascii_case(name)).cloned().ok_or_else(|| format!("No brush tip `{name}` (brushes.list).")).map_err(|e| e.to_string())?),
                None => None,
            };
            s.edit(cx.label(), cx.source, None, |d| {
                let id = util::layer_id(d, &a)?;
                let origin = util::prepare_paint(d, &id)?;
                let sel = d.selection.clone();
                let px = util::raster_of(d, &id)?;
                let touched = nori_render::brush::paint(px, origin, &brush, if erase { Mode::Erase } else { Mode::Paint(color) }, sel.as_ref(), tip.clone(), &points);
                Ok(json!({ "layerId": id, "touched": [touched.x, touched.y, touched.w, touched.h], "points": points.len() }))
            })
        }
        "raster.fill" => {
            let fg = s.colors.read().foreground;
            let color = util::color(&a, "color", fg)?;
            let tools = s.settings().tools;
            let tolerance = a.opt_i64("tolerance").unwrap_or(tools.tolerance as i64).clamp(0, 255) as u8;
            let contiguous = a.bool_or("contiguous", tools.contiguous);
            let sample_all = a.bool_or("sampleAll", tools.sample_all);
            let opacity = a.opt_f64("opacity").unwrap_or(1.0) as f32;
            let (doc, rev, doc_id) = s.snapshot()?;
            let id = util::layer_id(&doc, &a)?;
            let at = match (a.opt_i64("x"), a.opt_i64("y")) {
                (Some(x), Some(y)) => Some((x as i32, y as i32)),
                _ => None,
            };
            // The region (page pixels) worked out off the lock.
            let region = tokio::task::spawn_blocking({
                let id = id.clone();
                move || -> CmdResult<nori_core::Mask> {
                    let (w, h) = (doc.width(), doc.height());
                    let mut m = match at {
                        Some((x, y)) => {
                            let rgba = if sample_all {
                                nori_render::flatten(&doc)
                            } else {
                                let l = doc.layer(&id).ok_or("no layer")?;
                                match l.raster() {
                                    Some((lx, ly, p)) => p.read_rect(Rect::new(-lx, -ly, w, h)),
                                    None => nori_render::flatten(&doc),
                                }
                            };
                            nori_render::fill::region(&rgba, w, h, x, y, tolerance, contiguous)
                        }
                        None => nori_core::Mask::filled(w, h, 255),
                    };
                    if let Some(sel) = &doc.selection {
                        m = nori_core::selection::combine(Some(&m), sel.clone(), nori_core::selection::Combine::Intersect);
                    }
                    Ok(m)
                }
            })
            .await
            .map_err(|e| e.to_string())??;
            s.edit_at(doc_id,rev, cx.label(), cx.source, |d| {
                let (ox, oy) = util::prepare_paint(d, &id)?;
                let px = util::raster_of(d, &id)?;
                // The region in the raster's own pixels.
                let local = nori_core::Mask::from_vec(px.width(), px.height(), [0], &region.read_rect(Rect::new(ox, oy, px.width(), px.height())));
                let r = nori_render::fill::fill_mask(px, &local, color, opacity);
                px.compact();
                Ok(json!({ "layerId": id, "filled": [r.x + ox, r.y + oy, r.w, r.h] }))
            })
        }
        "raster.gradient" => {
            let c = *s.colors.read();
            let from = util::color(&a, "from", c.foreground)?;
            let to = match a.opt_str("to") {
                Some("transparent") => from.with_alpha(0.0),
                _ => util::color(&a, "to", c.background)?,
            };
            let kind = nori_render::fill::GradientKind::parse(a.opt_str("kind").unwrap_or(&s.settings().tools.gradient))?;
            let p = [a.f64("x1")? as f32, a.f64("y1")? as f32, a.f64("x2")? as f32, a.f64("y2")? as f32];
            let opacity = a.opt_f64("opacity").unwrap_or(1.0) as f32;
            s.edit(cx.label(), cx.source, None, |d| {
                let id = util::layer_id(d, &a)?;
                let origin = util::prepare_paint(d, &id)?;
                let sel = d.selection.clone();
                let px = util::raster_of(d, &id)?;
                nori_render::fill::gradient(px, origin, kind, [p[0], p[1]], [p[2], p[3]], from, to, opacity, sel.as_ref());
                Ok(json!({ "layerId": id }))
            })
        }
        "raster.clear" => s.edit(cx.label(), cx.source, None, |d| {
            let id = util::layer_id(d, &a)?;
            let sel = d.selection.clone();
            let l = d.layer_mut(&id).ok_or("no layer")?;
            util::unlocked(l)?;
            let name = l.name.clone();
            let (x, y, px) = l.raster_mut().ok_or_else(|| format!("`{name}` isn't pixels."))?;
            let r = Rect::new(*x, *y, px.width(), px.height());
            match sel {
                None => px.clear(),
                Some(m) => {
                    let mut data = px.read_rect(Rect::new(0, 0, r.w, r.h));
                    let mv = m.read_rect(r);
                    for (i, p) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        p[3] = ((p[3] as u32 * (255 - mv[i] as u32) + 127) / 255) as u8;
                    }
                    px.write_rect(Rect::new(0, 0, r.w, r.h), &data);
                    px.compact();
                }
            }
            Ok(json!({ "layerId": id }))
        }),
        "raster.pick" => {
            let d = s.document()?;
            let (x, y) = (a.opt_i64("x").unwrap_or(0) as i32, a.opt_i64("y").unwrap_or(0) as i32);
            let px = match a.opt_str("layerId") {
                Some(k) => {
                    let id = d.resolve(k)?;
                    let l = d.layer(&id).ok_or("no layer")?;
                    match l.raster() {
                        Some((lx, ly, p)) => p.get(x - lx, y - ly),
                        None => nori_render::composite::pick(&d, x, y),
                    }
                }
                None => nori_render::composite::pick(&d, x, y),
            };
            let c = Color::from_u8(px);
            if a.bool_or("setForeground", false) {
                s.colors.write().foreground = c.with_alpha(1.0);
                s.emit(crate::session::Event::ToolsChanged);
            }
            Ok(json!({ "color": c, "rgba": px }))
        }
        "brushes.list" => {
            let tips: Vec<Value> = s.brushes.read().iter().map(|t| json!({ "name": t.name, "width": t.width, "height": t.height })).collect();
            Ok(json!({ "round": true, "tips": tips }))
        }
        "brushes.import" => {
            let path = util::path(&a, "path")?;
            let bytes = std::fs::read(&path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
            let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Brush".into());
            let tip = nori_io::assets::read_gbr(&bytes, &stem).map_err(|e| format!("{}: {e}", path.display()))?;
            // Kept with nori's data, so it is there next time.
            let dir = s.data_dir.join("brushes");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let file = dir.join(path.file_name().unwrap_or_default());
            if file != path {
                std::fs::copy(&path, &file).map_err(|e| e.to_string())?;
            }
            let name = tip.name.clone();
            let mut list = s.brushes.write();
            list.retain(|t| t.name != name);
            list.push(Arc::new(tip));
            drop(list);
            s.emit(crate::session::Event::ToolsChanged);
            Ok(json!({ "tip": name }))
        }
        _ => Err(super::unhandled(cx)),
    }
}
