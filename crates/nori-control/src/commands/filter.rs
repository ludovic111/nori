//! `filter.*`: filters that change a layer's pixels (stock ones and plugins), and adjustments
//! applied for good.

use std::sync::Arc;

use nori_core::{Adjustment, Content};
use serde_json::{Map, Value, json};

use super::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "filter.list" => {
            let disabled = s.settings().plugins.disabled;
            let mut list: Vec<Value> = nori_render::filters::STOCK
                .iter()
                .filter(|f| !disabled.contains(&f.id.to_string()))
                .map(|f| json!({ "id": f.id, "name": f.name, "group": f.group, "description": f.description, "params": f.params, "source": "stock" }))
                .collect();
            list.extend(s.plugins.filters().into_iter().filter(|p| !disabled.contains(&p["pluginId"].as_str().unwrap_or("").to_string())));
            Ok(json!({ "filters": list }))
        }
        "filter.apply" => {
            let fid = a.str("filter")?.to_string();
            let given: Map<String, Value> = a.object("params").cloned().unwrap_or_default();
            if s.settings().plugins.disabled.iter().any(|d| *d == fid || format!("plugin:{d}") == fid) {
                return Err(format!("`{fid}` is switched off (plugin.enable turns it on)."));
            }
            // What runs: a stock filter or a plugin, its values and margin.
            enum Runner {
                Stock(&'static str, Vec<f64>),
                Plugin(String, Vec<f64>),
            }
            let (runner, margin) = if let Some(pid) = fid.strip_prefix("plugin:") {
                let values = s.plugins.values(pid, &given)?;
                let m = s.plugins.margin(pid, &values)? as i32;
                (Runner::Plugin(pid.to_string(), values), m)
            } else {
                let spec = nori_render::filters::spec(&fid).ok_or_else(|| {
                    let names: Vec<&str> = nori_render::filters::STOCK.iter().map(|f| f.id).collect();
                    let hint = nori_core::closest(&fid, &names).map(|c| format!(" Did you mean `{c}`?")).unwrap_or_default();
                    format!("No filter `{fid}`.{hint} filter.list shows them (plugins are plugin:<id>).")
                })?;
                let values = nori_render::filters::values(spec, &given)?;
                let m = nori_render::filters::margin(spec.id, &values);
                (Runner::Stock(spec.id, values), m)
            };
            let (doc, rev, doc_id) = s.snapshot()?;
            let id = util::layer_id(&doc, &a)?;
            let host = s.plugins.clone();
            let result = tokio::task::spawn_blocking(move || -> CmdResult<(nori_core::Raster, i32, i32, nori_core::Rect, bool)> {
                let l = doc.layer(&id).ok_or("no layer")?;
                util::unlocked(l)?;
                if l.smart_source.is_some() { return Err("Rasterize the smart object or edit its source before applying a destructive filter.".into()); }
                // Text, vectors and fills become pixels first.
                let rasterize = l.raster().is_none();
                let (x, y, mut px) = super::layer::rasterized(&doc, &id)?;
                // Over the whole page, so a filter can spread past the old edges.
                let (pw, ph) = doc.page_of(&id).map(|p| (p.width, p.height)).ok_or("no page")?;
                let have = nori_core::Rect::new(x, y, px.width(), px.height());
                let page = nori_core::Rect::new(0, 0, pw, ph);
                let (x, y) = if have.intersect(&page) != page {
                    let want = have.union(&page);
                    let mut grown = nori_core::Raster::transparent(want.w, want.h);
                    grown.write_rect(nori_core::Rect::new(have.x - want.x, have.y - want.y, have.w, have.h), &px.to_vec());
                    px = grown;
                    (want.x, want.y)
                } else {
                    (x, y)
                };
                let changed = nori_render::filters::apply_in_selection(&mut px, x, y, doc.selection.as_ref(), margin, |buf, w, h, origin| match &runner {
                    Runner::Stock(fid, v) => nori_render::filters::run(fid, v, buf, w, h, origin),
                    Runner::Plugin(pid, v) => host.run(pid, v, buf, w as u32, h as u32, origin),
                })?;
                Ok((px, x, y, changed, rasterize))
            })
            .await
            .map_err(|e| e.to_string())??;
            let (px, x, y, changed, rasterize) = result;
            let id2 = util::layer_id(&s.document()?, &a)?;
            s.edit_at(doc_id,rev, cx.label(), cx.source, move |d| {
                let l = d.layer_mut(&id2).ok_or("no layer")?;
                l.content = Content::Raster { x, y, pixels: px };
                Ok(json!({ "layerId": id2, "filter": fid, "changed": [changed.x, changed.y, changed.w, changed.h], "rasterized": rasterize }))
            })
        }
        "filter.adjust" => {
            let mut adj = Adjustment::default_of(a.str("kind")?)?;
            if let Some(o) = a.object("settings") {
                adj = adj.merge(o)?;
            }
            let compiled = nori_render::adjust::compile(&adj);
            let (doc, rev, doc_id) = s.snapshot()?;
            let id = util::layer_id(&doc, &a)?;
            let (px, x, y) = tokio::task::spawn_blocking(move || -> CmdResult<(nori_core::Raster, i32, i32)> {
                let l = doc.layer(&id).ok_or("no layer")?;
                util::unlocked(l)?;
                if l.smart_source.is_some() { return Err("Rasterize the smart object or edit its source before applying a destructive filter.".into()); }
                let (x, y, mut px) = super::layer::rasterized(&doc, &id)?;
                nori_render::filters::apply_in_selection(&mut px, x, y, doc.selection.as_ref(), 0, |buf, _, _, _| {
                    for p in buf.iter_mut() {
                        if p[3] <= 0.0 {
                            continue;
                        }
                        let c = compiled.apply([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
                        *p = [c[0].clamp(0.0, 1.0) * p[3], c[1].clamp(0.0, 1.0) * p[3], c[2].clamp(0.0, 1.0) * p[3], p[3]];
                    }
                    Ok(())
                })?;
                Ok((px, x, y))
            })
            .await
            .map_err(|e| e.to_string())??;
            let id2 = util::layer_id(&s.document()?, &a)?;
            let label = adj.label();
            s.edit_at(doc_id,rev, cx.label(), cx.source, move |d| {
                let l = d.layer_mut(&id2).ok_or("no layer")?;
                l.content = Content::Raster { x, y, pixels: px };
                Ok(json!({ "layerId": id2, "adjusted": label }))
            })
        }
        _ => Err(super::unhandled(cx)),
    }
}
