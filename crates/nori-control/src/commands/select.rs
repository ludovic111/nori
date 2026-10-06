//! `select.*`: the selection: shapes, the magic wand, a layer's pixels, grow, shrink, feather.

use std::sync::Arc;

use nori_core::selection::{self, Combine};
use nori_core::{Mask, Rect};
use serde_json::{Value, json};

use super::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

fn combine(a: &Args) -> CmdResult<Combine> {
    a.opt_str("combine").map(Combine::parse).transpose().map(Option::unwrap_or_default)
}

fn set(d: &mut nori_core::Document, shape: Mask, how: Combine) -> Value {
    let m = selection::combine(d.selection.as_ref(), shape, how);
    d.selection = if m.is_empty() { None } else { Some(m) };
    summary(d)
}

pub fn summary(d: &nori_core::Document) -> Value {
    match &d.selection {
        None => json!({ "selection": null }),
        Some(m) => {
            let b = m.content_bounds();
            json!({ "selection": { "bounds": b.map(|b| [b.x, b.y, b.w as i32, b.h as i32]) } })
        }
    }
}

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    let rect = |a: &Args| -> CmdResult<Rect> { Ok(Rect::new(a.opt_i64("x").unwrap_or(0) as i32, a.opt_i64("y").unwrap_or(0) as i32, a.opt_u32("width").unwrap_or(0), a.opt_u32("height").unwrap_or(0))) };
    match cx.spec.name {
        "select.get" => Ok(summary(&s.document()?)),
        "select.all" => s.edit(cx.label(), cx.source, None, |d| {
            let (w, h) = (d.width(), d.height());
            d.selection = Some(Mask::filled(w, h, 255));
            Ok(summary(d))
        }),
        "select.none" => s.edit(cx.label(), cx.source, None, |d| {
            d.selection = None;
            Ok(summary(d))
        }),
        "select.invert" => s.edit(cx.label(), cx.source, None, |d| {
            let (w, h) = (d.width(), d.height());
            let inv = d.selection.as_ref().map(Mask::inverted).unwrap_or_else(|| Mask::filled(w, h, 0));
            d.selection = if inv.is_empty() { None } else { Some(inv) };
            Ok(summary(d))
        }),
        "select.rect" => {
            let how = combine(&a)?;
            let r = rect(&a)?;
            s.edit(cx.label(), cx.source, None, |d| {
                let m = selection::rect(d.width(), d.height(), r);
                Ok(set(d, m, how))
            })
        }
        "select.ellipse" => {
            let how = combine(&a)?;
            let r = rect(&a)?;
            s.edit(cx.label(), cx.source, None, |d| {
                let m = selection::ellipse(d.width(), d.height(), r, true);
                Ok(set(d, m, how))
            })
        }
        "select.polygon" => {
            let how = combine(&a)?;
            let pts: Vec<[f32; 2]> = a.array("points").into_iter().flatten().filter_map(|p| Some([p.get(0)?.as_f64()? as f32, p.get(1)?.as_f64()? as f32])).collect();
            if pts.len() < 3 {
                return Err("A polygon needs three points or more.".into());
            }
            s.edit(cx.label(), cx.source, None, |d| {
                let m = selection::polygon(d.width(), d.height(), &pts, true);
                Ok(set(d, m, how))
            })
        }
        "select.color" => {
            let how = combine(&a)?;
            let tools = s.settings().tools;
            let (x, y) = (a.opt_i64("x").unwrap_or(0) as i32, a.opt_i64("y").unwrap_or(0) as i32);
            let tolerance = a.opt_i64("tolerance").unwrap_or(tools.tolerance as i64).clamp(0, 255) as u8;
            let contiguous = a.bool_or("contiguous", tools.contiguous);
            let sample_all = a.bool_or("sampleAll", true);
            let (doc, rev, _) = s.snapshot()?;
            let region = tokio::task::spawn_blocking(move || -> CmdResult<Mask> {
                let (w, h) = (doc.width(), doc.height());
                let rgba = match (sample_all, doc.active_id().and_then(|id| doc.layer(&id).and_then(|l| l.raster()).map(|(lx, ly, p)| p.read_rect(Rect::new(-lx, -ly, w, h))))) {
                    (false, Some(px)) => px,
                    _ => nori_render::flatten(&doc),
                };
                Ok(nori_render::fill::region(&rgba, w, h, x, y, tolerance, contiguous))
            })
            .await
            .map_err(|e| e.to_string())??;
            s.edit_at(rev, cx.label(), cx.source, |d| Ok(set(d, region, how)))
        }
        "select.layer" => {
            let how = combine(&a)?;
            let (doc, rev, _) = s.snapshot()?;
            let id = util::layer_id(&doc, &a)?;
            let m = tokio::task::spawn_blocking(move || -> CmdResult<Mask> {
                let (w, h) = (doc.width(), doc.height());
                let (x, y, px) = super::layer::rasterized(&doc, &id)?;
                let alpha: Vec<u8> = px.read_rect(Rect::new(-x, -y, w, h)).chunks_exact(4).map(|p| p[3]).collect();
                Ok(Mask::from_vec(w, h, [0], &alpha))
            })
            .await
            .map_err(|e| e.to_string())??;
            s.edit_at(rev, cx.label(), cx.source, |d| Ok(set(d, m, how)))
        }
        "select.modify" => s.edit(cx.label(), cx.source, None, |d| {
            let Some(mut m) = d.selection.clone() else { return Err("Nothing is selected.".into()) };
            if let Some(g) = a.opt_i64("grow") {
                m = selection::grow(&m, g.clamp(-500, 500) as i32);
            }
            if let Some(r) = a.opt_f64("feather").filter(|r| *r > 0.0) {
                let (w, h) = (m.width() as usize, m.height() as usize);
                let mut px: Vec<[f32; 4]> = m.to_vec().iter().map(|v| {
                    let f = *v as f32 / 255.0;
                    [f, f, f, f]
                }).collect();
                nori_render::filters::gaussian_blur(&mut px, w, h, (r as f32 / 2.0).max(0.3));
                let data: Vec<u8> = px.iter().map(|p| (p[3].clamp(0.0, 1.0) * 255.0).round() as u8).collect();
                m = Mask::from_vec(m.width(), m.height(), m.fill(), &data);
            }
            d.selection = if m.is_empty() { None } else { Some(m) };
            Ok(summary(d))
        }),
        _ => Err(super::unhandled(cx)),
    }
}
