//! `page.*`: pages and artboards, master pages, margins, columns, guides; and looking at a page.

use std::sync::Arc;

use nori_core::{Guide, Margins, Page, PageRef};
use serde_json::{Value, json};

use super::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "page.list" => {
            let d = s.document()?;
            let pages: Vec<Value> = d.pages.iter().enumerate().map(|(i, p)| {
                let mut v = util::describe_page(&d, p);
                v["number"] = json!(i + 1);
                v
            }).collect();
            let masters: Vec<Value> = d.masters.iter().map(|p| util::describe_page(&d, p)).collect();
            Ok(json!({ "pages": pages, "masters": masters, "active": d.active_page }))
        }
        "page.add" => s.edit(cx.label(), cx.source, None, |d| {
            let count = a.opt_u32("count").unwrap_or(1).clamp(1, 500);
            let at = match a.opt_str("after") {
                Some(k) => match d.resolve_page(k)? {
                    PageRef::Page(i) => i + 1,
                    PageRef::Master(_) => return Err("Pages go after pages, not masters.".into()),
                },
                None => d.pages.len(),
            };
            let template = d.page().clone();
            let master = match a.opt_str("master") {
                Some(m) => match d.resolve_page(m)? {
                    PageRef::Master(i) => Some(d.masters[i].id.clone()),
                    PageRef::Page(_) => return Err(format!("`{m}` is a page, not a master (page.addMaster makes one).")),
                },
                None => template.master.clone(),
            };
            let mut made = vec![];
            for k in 0..count {
                let id = d.new_page_id();
                let n = d.pages.len() + 1;
                let mut p = Page::new(id.clone(), a.opt_str("name").map(|s| if count > 1 { format!("{s} {}", k + 1) } else { s.to_string() }).unwrap_or_else(|| format!("Page {n}")), a.opt_u32("width").unwrap_or(template.width).max(1), a.opt_u32("height").unwrap_or(template.height).max(1));
                p.margins = template.margins;
                p.columns = template.columns;
                p.gutter = template.gutter;
                p.bleed = template.bleed;
                p.master = master.clone();
                // Artboards sit side by side on the pasteboard.
                p.x = template.x + (template.width as f32 + 100.0) * (k + 1) as f32;
                p.y = template.y;
                if a.bool_or("duplicate", false) {
                    let mut layers = template.layers.clone();
                    renumber(d, &mut layers);
                    p.layers = layers;
                } else {
                    // A sheet of paper like the page it follows, when that one has paper.
                    if let Some(paper) = template.layers.last().filter(|l| matches!(l.content, nori_core::Content::Fill { .. }) && l.name == "Paper") {
                        let mut l = paper.clone();
                        l.id = d.new_id();
                        p.layers.push(l);
                    }
                }
                d.pages.insert((at + k as usize).min(d.pages.len()), p);
                made.push(id);
            }
            if let Some(last) = made.last() {
                d.active_page = last.clone();
                d.active = d.page().layers.first().map(|l| l.id.clone());
                d.selection = None;
            }
            Ok(json!({ "pages": made, "count": d.pages.len() }))
        }),
        "page.remove" => s.edit(cx.label(), cx.source, None, |d| {
            match d.resolve_page(a.str("page")?)? {
                PageRef::Page(i) => {
                    if d.pages.len() == 1 {
                        return Err("A document keeps at least one page.".into());
                    }
                    let p = d.pages.remove(i);
                    if d.active_page == p.id {
                        d.active_page = d.pages[i.min(d.pages.len() - 1)].id.clone();
                        d.active = d.page().layers.first().map(|l| l.id.clone());
                        d.selection = None;
                    }
                    // Threads into it lose their next frame.
                    let gone: Vec<String> = p.all().iter().map(|l| l.id.clone()).collect();
                    let ids: Vec<String> = d.all().iter().map(|l| l.id.clone()).collect();
                    for id in ids {
                        if let Some(nori_core::Layer { content: nori_core::Content::Text { text }, .. }) = d.layer_mut(&id)
                            && text.next.as_ref().is_some_and(|n| gone.contains(n))
                        {
                            text.next = None;
                        }
                    }
                    Ok(json!({ "removed": p.id, "pages": d.pages.len() }))
                }
                PageRef::Master(i) => {
                    let m = d.masters.remove(i);
                    for p in d.pages.iter_mut().filter(|p| p.master.as_deref() == Some(&m.id)) {
                        p.master = None;
                    }
                    Ok(json!({ "removed": m.id }))
                }
            }
        }),
        "page.move" => s.edit(cx.label(), cx.source, None, |d| {
            let PageRef::Page(i) = d.resolve_page(a.str("page")?)? else { return Err("Masters aren't in the page order.".into()) };
            let to = (a.opt_i64("to").unwrap_or(1).max(1) as usize - 1).min(d.pages.len() - 1);
            let p = d.pages.remove(i);
            d.pages.insert(to, p);
            Ok(json!({ "order": d.pages.iter().map(|p| p.id.clone()).collect::<Vec<_>>() }))
        }),
        "page.select" => s.edit(cx.label(), cx.source, None, |d| {
            let r = d.resolve_page(a.str("page")?)?;
            let p = d.page_at(r).ok_or("no page")?;
            let id = p.id.clone();
            let first = p.layers.first().map(|l| l.id.clone());
            if d.active_page != id {
                d.selection = None;
            }
            // Masters are edited like pages (what is drawn on one shows on every page using it).
            d.active_page = id.clone();
            if !d.active.as_ref().is_some_and(|a| d.page().all().iter().any(|l| &l.id == a)) {
                d.active = first;
            }
            Ok(json!({ "active": id, "master": matches!(r, PageRef::Master(_)) }))
        }),
        "page.update" => s.edit(cx.label(), cx.source, a.coalesce(), |d| {
            let r = util::page_ref(d, &a)?;
            let master = match a.opt_str("master") {
                Some("") => Some(None),
                Some(m) => match d.resolve_page(m)? {
                    PageRef::Master(i) => Some(Some(d.masters[i].id.clone())),
                    PageRef::Page(_) => return Err(format!("`{m}` is a page, not a master.")),
                },
                None => None,
            };
            let p = d.page_at_mut(r).ok_or("no page")?;
            if let Some(n) = a.opt_str("name") {
                p.name = n.to_string();
            }
            if let Some(w) = a.opt_u32("width") {
                p.width = w.clamp(1, nori_core::MAX_SIDE);
            }
            if let Some(h) = a.opt_u32("height") {
                p.height = h.clamp(1, nori_core::MAX_SIDE);
            }
            if let Some(x) = a.opt_f64("x") {
                p.x = x as f32;
            }
            if let Some(y) = a.opt_f64("y") {
                p.y = y as f32;
            }
            match a.get("margins") {
                Some(Value::Number(n)) => {
                    let v = n.as_f64().unwrap_or(0.0).max(0.0) as f32;
                    p.margins = Margins { top: v, right: v, bottom: v, left: v };
                }
                Some(v @ Value::Object(_)) => {
                    let mut m = serde_json::to_value(p.margins).map_err(|e| e.to_string())?;
                    for (k, val) in v.as_object().into_iter().flatten() {
                        if !["top", "right", "bottom", "left"].contains(&k.as_str()) {
                            return Err(format!("Margins are top, right, bottom and left, not `{k}`."));
                        }
                        m[k] = val.clone();
                    }
                    p.margins = serde_json::from_value(m).map_err(|e| e.to_string())?;
                }
                Some(other) => return Err(format!("margins is a number or {{top, right, bottom, left}}, not {other}")),
                None => {}
            }
            if let Some(c) = a.opt_u32("columns") {
                p.columns = c.clamp(1, 24);
            }
            if let Some(g) = a.opt_f64("gutter") {
                p.gutter = g.max(0.0) as f32;
            }
            if let Some(b) = a.opt_f64("bleed") {
                p.bleed = b.max(0.0) as f32;
            }
            if let Some(m) = master {
                p.master = m;
            }
            let v = serde_json::to_value(&*p).map_err(|e| e.to_string())?;
            Ok(json!({ "page": { "id": v["id"], "name": v["name"], "width": v["width"], "height": v["height"], "margins": v["margins"], "columns": v["columns"], "gutter": v["gutter"], "bleed": v["bleed"], "master": v["master"] } }))
        }),
        "page.addGuide" => s.edit(cx.label(), cx.source, None, |d| {
            let r = util::page_ref(d, &a)?;
            let p = d.page_at_mut(r).ok_or("no page")?;
            let g = Guide { vertical: a.bool_or("vertical", true), at: a.f64("at")? as f32 };
            p.guides.push(g);
            Ok(json!({ "guides": p.guides }))
        }),
        "page.clearGuides" => s.edit(cx.label(), cx.source, None, |d| {
            let r = util::page_ref(d, &a)?;
            d.page_at_mut(r).ok_or("no page")?.guides.clear();
            Ok(json!({ "guides": [] }))
        }),
        "page.addMaster" => s.edit(cx.label(), cx.source, None, |d| {
            let id = d.new_page_id();
            let name = a.opt_str("name").unwrap_or("A-Master").to_string();
            let mut m = match a.opt_str("from") {
                Some(k) => {
                    let r = d.resolve_page(k)?;
                    let src = d.page_at(r).ok_or("no page")?.clone();
                    let mut layers = src.layers.clone();
                    renumber(d, &mut layers);
                    let mut m = src;
                    m.layers = layers;
                    m
                }
                None => {
                    let p = d.page().clone();
                    let mut m = p;
                    m.layers = vec![];
                    m
                }
            };
            m.id = id.clone();
            m.name = name;
            m.master = None;
            d.masters.push(m);
            Ok(json!({ "master": id }))
        }),
        "page.look" => {
            let d = s.document()?;
            let r = util::page_ref(&d, &a)?;
            let page = d.page_at(r).ok_or("no page")?.clone();
            let width = a.opt_u32("width").unwrap_or(1024).clamp(16, 4096);
            let region = a.array("region").map(|v| v.iter().filter_map(Value::as_f64).collect::<Vec<f64>>());
            let s2 = s.clone();
            tokio::task::spawn_blocking(move || {
                let area = match region.as_deref() {
                    Some([x, y, w, h]) => nori_core::Rect::new(*x as i32, *y as i32, (*w as u32).max(1), (*h as u32).max(1)).intersect(&page.bounds()),
                    _ => page.bounds(),
                };
                if area.is_empty() {
                    return Err("That region is outside the page.".to_string());
                }
                let prep = nori_render::prepare(&d);
                let rgba = nori_render::composite::flatten_page_with(&d, &page, &prep, area);
                // Over a light grey checkerboard, so transparency reads.
                let rgba = checker(&rgba, area.w, area.h);
                let (w, h, small) = nori_render::transform::fit_rgba(&rgba, area.w, area.h, width.max(16));
                let path = util::save_look(&s2, &format!("page-{}", page.id), w, h, &small)?;
                Ok(json!({ "path": path, "width": w, "height": h, "page": page.id, "region": [area.x, area.y, area.w, area.h] }))
            })
            .await
            .map_err(|e| e.to_string())?
        }
        _ => Err(super::unhandled(cx)),
    }
}

/// New ids for copied layers.
pub fn renumber(d: &mut nori_core::Document, list: &mut [nori_core::Layer]) {
    for l in list {
        l.id = d.new_id();
        if let Some(c) = l.children_mut() {
            renumber(d, c);
        }
        if let nori_core::Content::Text { text } = &mut l.content {
            text.next = None;
        }
    }
}

/// Transparent pixels over a checkerboard (for pictures agents look at).
pub fn checker(rgba: &[u8], w: u32, _h: u32) -> Vec<u8> {
    rgba.chunks_exact(4)
        .enumerate()
        .flat_map(|(i, p)| {
            let (x, y) = (i as u32 % w, i as u32 / w);
            let bg: u32 = if ((x / 12) + (y / 12)) % 2 == 0 { 255 } else { 220 };
            let a = p[3] as u32;
            [0, 1, 2].map(|c| ((p[c] as u32 * a + bg * (255 - a) + 127) / 255) as u8).into_iter().chain([255u8])
        })
        .collect()
}
