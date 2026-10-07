//! `layer.*`: the layer stack: add, place, change, move, reorder, group, merge, masks,
//! transforms, adjustment layers.

use std::sync::Arc;

use nori_core::{Adjustment, BlendMode, Color, Content, Document, Layer, LayerMask, Mask, Raster, Rect};
use serde_json::{Value, json};

use super::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "layer.makeSmartObject" | "layer.resizeSmartObject" | "layer.replaceSmartObject" | "layer.extractSmartObject" => super::smart::run(s, cx, a).await,
        "layer.list" => {
            let d = s.document()?;
            let r = util::page_ref(&d, &a)?;
            let p = d.page_at(r).ok_or("no page")?;
            Ok(json!({ "page": p.id, "active": d.active_id(), "layers": p.layers.iter().map(|l| util::describe(&d, l, 0)).collect::<Vec<_>>() }))
        }
        "layer.get" => {
            let d = s.document()?;
            let id = d.resolve(a.str("layerId")?)?;
            let l = d.layer(&id).ok_or("no layer")?;
            let mut v = util::describe(&d, l, 0);
            if let Content::Text { text } = &l.content {
                v["text"] = json!(text);
            }
            v["page"] = json!(d.page_of(&id).map(|p| p.id.clone()));
            Ok(v)
        }
        "layer.add" => {
            let fg = s.colors.read().foreground;
            s.edit(cx.label(), cx.source, None, |d| {
                let kind = a.opt_str("kind").unwrap_or("raster");
                let (w, h) = (d.width(), d.height());
                let (content, base) = match kind {
                    "raster" | "pixels" | "layer" => (Content::Raster { x: 0, y: 0, pixels: Raster::transparent(w, h) }, "Layer"),
                    "fill" | "solid" => (Content::Fill { color: util::color(&a, "color", fg)? }, "Color Fill"),
                    "group" => (Content::Group { children: vec![], expanded: true }, "Group"),
                    other => return Err(format!("`{other}` isn't a layer kind here: raster, fill or group (text.add, vector.addShape and layer.addAdjustment make the others).")),
                };
                add(d, &a, content, base)
            })
        }
        "layer.addAdjustment" => s.edit(cx.label(), cx.source, None, |d| {
            let kind = a.str("kind")?;
            let mut adj = Adjustment::default_of(kind)?;
            if let Some(o) = a.object("settings") {
                adj = adj.merge(o)?;
            }
            let base = adj.label();
            add(d, &a, Content::Adjustment { adjustment: adj }, base)
        }),
        "layer.addLut" => {
            let path = util::path(&a, "path")?;
            let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
            let (size, table) = nori_core::file::parse_cube(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "LUT".into());
            let amount = a.opt_f64("amount").unwrap_or(1.0).clamp(0.0, 1.0) as f32;
            s.edit(cx.label(), cx.source, None, |d| {
                let adj = Adjustment::Lut { name: name.clone(), size, amount, table: Arc::new(table) };
                let mut args = a.clone();
                args.0.insert("name".into(), json!(name));
                add(d, &args, Content::Adjustment { adjustment: adj }, "Color Lookup")
            })
        }
        "layer.place" => {
            let path = util::path(&a, "path")?;
            let at = match (a.opt_i64("x"), a.opt_i64("y")) {
                (Some(x), Some(y)) => Some((x as i32, y as i32)),
                _ => None,
            };
            let (tw, th) = (a.opt_u32("width"), a.opt_u32("height"));
            // Decode outside the document lock.
            let p2 = path.clone();
            let placed = tokio::task::spawn_blocking(move || {
                let mut scratch = Document::empty("place", 1, 1);
                let id = nori_io::place(&mut scratch, &p2, Some((0, 0)))?;
                let mut l = scratch.layer(&id).cloned().ok_or("nothing placed")?;
                if let (Content::Raster { pixels, .. }, true) = (&mut l.content, tw.is_some() || th.is_some()) {
                    let (w, h) = (pixels.width() as f32, pixels.height() as f32);
                    let (nw, nh) = match (tw, th) {
                        (Some(w2), Some(h2)) => (w2, h2),
                        (Some(w2), None) => (w2, (h * w2 as f32 / w).round() as u32),
                        (None, Some(h2)) => ((w * h2 as f32 / h).round() as u32, h2),
                        _ => (w as u32, h as u32),
                    };
                    *pixels = nori_render::transform::resize_raster(pixels, nw.max(1), nh.max(1), nori_render::transform::Filter::Bicubic);
                }
                Ok::<Layer, String>(l)
            })
            .await
            .map_err(|e| e.to_string())??;
            s.edit(cx.label(), cx.source, None, move |d| {
                let mut l = placed;
                super::page::renumber(d, std::slice::from_mut(&mut l));
                // Pictures land centred unless told where.
                if let (Content::Raster { x, y, pixels }, None) = (&mut l.content, at) {
                    *x = (d.width() as i32 - pixels.width() as i32) / 2;
                    *y = (d.height() as i32 - pixels.height() as i32) / 2;
                } else if let (Content::Raster { x, y, .. }, Some((ax, ay))) = (&mut l.content, at) {
                    *x = ax;
                    *y = ay;
                }
                let id = l.id.clone();
                let above = target_above(d, &a)?;
                d.insert_above(l, above.as_deref());
                d.active = Some(id.clone());
                let b = d.layer(&id).and_then(|l| util::bounds(d, l));
                Ok(json!({ "layerId": id, "bounds": b.map(|b| [b.x, b.y, b.w as i32, b.h as i32]) }))
            })
        }
        "layer.update" => s.edit(cx.label(), cx.source, a.coalesce(), |d| {
            let id = util::layer_id(d, &a)?;
            let l = d.layer_mut(&id).ok_or("no layer")?;
            if let Some(n) = a.opt_str("name") {
                l.name = n.trim().to_string();
            }
            if let Some(v) = a.opt_bool("visible") {
                l.visible = v;
            }
            if let Some(v) = a.opt_bool("locked") {
                l.locked = v;
            }
            if let Some(o) = a.opt_f64("opacity") {
                // 0–1, or a percentage when it is clearly one.
                let o = if o > 1.0 { o / 100.0 } else { o };
                l.opacity = o.clamp(0.0, 1.0) as f32;
            }
            if let Some(b) = a.opt_str("blend") {
                let m = BlendMode::parse(b)?;
                if m == BlendMode::PassThrough && !l.is_group() {
                    return Err("Pass-through is for groups.".into());
                }
                l.blend = m;
            }
            if let Some(c) = a.opt_bool("clipped") {
                l.clipped = c;
            }
            if let (Some(e), Content::Group { expanded, .. }) = (a.opt_bool("expanded"), &mut l.content) {
                *expanded = e;
            }
            if let Some(c) = a.opt_str("color") {
                match &mut l.content {
                    Content::Fill { color } => *color = Color::parse(c)?,
                    _ => return Err(format!("`{}` isn't a colour fill layer: color is for fills (vector.update and text.update colour the others).", l.name)),
                }
            }
            Ok(json!({ "layerId": id, "name": l.name, "visible": l.visible, "locked": l.locked, "opacity": l.opacity, "blend": l.blend.id(), "clipped": l.clipped }))
        }),
        "layer.select" => s.look(|d| {
            let id = d.resolve(a.str("layerId")?)?;
            if let Some(p) = d.page_of(&id).map(|p| p.id.clone())
                && p != d.active_page
            {
                d.active_page = p;
                d.selection = None;
            }
            d.active = Some(id.clone());
            Ok(json!({ "active": id }))
        }),
        "layer.move" => s.edit(cx.label(), cx.source, a.coalesce(), |d| {
            let id = util::layer_id(d, &a)?;
            let b = d.layer(&id).and_then(|l| util::bounds(d, l));
            let l = d.layer_mut(&id).ok_or("no layer")?;
            util::unlocked(l)?;
            let (mut dx, mut dy) = (a.opt_f64("dx").unwrap_or(0.0) as f32, a.opt_f64("dy").unwrap_or(0.0) as f32);
            if let (Some(x), Some(b)) = (a.opt_f64("x"), b) {
                dx = x as f32 - b.x as f32;
            }
            if let (Some(y), Some(b)) = (a.opt_f64("y"), b) {
                dy = y as f32 - b.y as f32;
            }
            super::doc::shift(l, dx, dy);
            let nb = d.layer(&id).and_then(|l| util::bounds(d, l));
            Ok(json!({ "layerId": id, "moved": [dx, dy], "bounds": nb.map(|b| [b.x, b.y, b.w as i32, b.h as i32]) }))
        }),
        "layer.align" => s.edit(cx.label(), cx.source, None, |d| {
            let ids: Vec<String> = a.strings("layerIds").iter().map(|k| d.resolve(k)).collect::<Result<_, _>>()?;
            let to = a.str("to")?.to_ascii_lowercase();
            let boxes: Vec<(String, Rect)> = ids.iter().filter_map(|id| Some((id.clone(), util::bounds(d, d.layer(id)?)?))).collect();
            if boxes.is_empty() {
                return Err("Those layers have nothing to align.".into());
            }
            let page = d.page().clone();
            let frame = match a.opt_str("relativeTo").unwrap_or("page") {
                "margins" => {
                    let m = page.margins;
                    Rect::new(m.left as i32, m.top as i32, (page.width as f32 - m.left - m.right).max(1.0) as u32, (page.height as f32 - m.top - m.bottom).max(1.0) as u32)
                }
                "selection" => boxes.iter().map(|(_, b)| *b).reduce(|a, b| a.union(&b)).unwrap_or_default(),
                _ => page.bounds(),
            };
            let mut moves = vec![];
            if to.starts_with("distribute") {
                let horizontal = to.contains("horizontal");
                let mut sorted = boxes.clone();
                sorted.sort_by_key(|(_, b)| if horizontal { b.x } else { b.y });
                let n = sorted.len();
                if n >= 3 {
                    let (first, last) = (sorted[0].1, sorted[n - 1].1);
                    let total: i64 = sorted.iter().map(|(_, b)| if horizontal { b.w as i64 } else { b.h as i64 }).sum();
                    let span = if horizontal { last.right() - first.x } else { last.bottom() - first.y } as i64;
                    let gap = (span - total) as f32 / (n - 1) as f32;
                    let mut at = if horizontal { first.x } else { first.y } as f32;
                    for (id, b) in &sorted {
                        let cur = if horizontal { b.x } else { b.y } as f32;
                        moves.push((id.clone(), if horizontal { (at - cur, 0.0) } else { (0.0, at - cur) }));
                        at += if horizontal { b.w } else { b.h } as f32 + gap;
                    }
                }
            } else {
                for (id, b) in &boxes {
                    let (dx, dy) = match to.as_str() {
                        "left" => ((frame.x - b.x) as f32, 0.0),
                        "center" => ((frame.x as f32 + frame.w as f32 / 2.0) - (b.x as f32 + b.w as f32 / 2.0), 0.0),
                        "right" => ((frame.right() - b.right()) as f32, 0.0),
                        "top" => (0.0, (frame.y - b.y) as f32),
                        "middle" => (0.0, (frame.y as f32 + frame.h as f32 / 2.0) - (b.y as f32 + b.h as f32 / 2.0)),
                        "bottom" => (0.0, (frame.bottom() - b.bottom()) as f32),
                        other => return Err(format!("`{other}` isn't an alignment: left, center, right, top, middle, bottom, distributeHorizontal or distributeVertical.")),
                    };
                    moves.push((id.clone(), (dx.round(), dy.round())));
                }
            }
            for (id, (dx, dy)) in &moves {
                if let Some(l) = d.layer_mut(id) {
                    util::unlocked(l)?;
                    super::doc::shift(l, *dx, *dy);
                }
            }
            Ok(json!({ "moved": moves.iter().map(|(id, (x, y))| json!({ "layerId": id, "by": [x, y] })).collect::<Vec<_>>() }))
        }),
        "layer.reorder" => s.edit(cx.label(), cx.source, None, |d| {
            let id = util::layer_id(d, &a)?;
            let loc = d.path_of(&id).ok_or("no layer")?;
            if let Some(target) = a.opt_str("above").or(a.opt_str("below")).or(a.opt_str("into")) {
                let t = d.resolve(target)?;
                if t == id || d.layer(&id).is_some_and(|l| l.walk().iter().any(|x| x.id == t)) {
                    return Err("A layer can't go inside itself.".into());
                }
                let l = d.remove(&id).ok_or("no layer")?;
                if a.has("into") {
                    let g = d.layer_mut(&t).ok_or("no layer")?;
                    let kids = g.children_mut().ok_or_else(|| format!("`{target}` isn't a group."))?;
                    kids.insert(0, l);
                } else {
                    let tl = d.path_of(&t).ok_or("no layer")?;
                    let i = *tl.path.last().unwrap_or(&0) + usize::from(a.has("below"));
                    d.siblings_mut(&tl).ok_or("no list")?.insert(i, l);
                }
                d.active = Some(id.clone());
                return Ok(json!({ "layerId": id }));
            }
            let to = a.opt_str("to").ok_or("Give to (top, bottom, up, down), above, below or into.")?;
            let i = *loc.path.last().unwrap_or(&0);
            let list = d.siblings_mut(&loc).ok_or("no list")?;
            let n = list.len();
            let j = match to {
                "top" | "front" => 0,
                "bottom" | "back" => n - 1,
                "up" | "forward" => i.saturating_sub(1),
                "down" | "backward" => (i + 1).min(n - 1),
                other => return Err(format!("`{other}`: to is top, bottom, up or down.")),
            };
            let l = list.remove(i);
            list.insert(j, l);
            Ok(json!({ "layerId": id, "index": j }))
        }),
        "layer.duplicate" => s.edit(cx.label(), cx.source, None, |d| {
            let id = util::layer_id(d, &a)?;
            let mut copy = d.layer(&id).cloned().ok_or("no layer")?;
            super::page::renumber(d, std::slice::from_mut(&mut copy));
            copy.name = a.opt_str("name").map(str::to_string).unwrap_or_else(|| format!("{} copy", copy.name));
            let new = copy.id.clone();
            d.insert_above(copy, Some(&id));
            d.active = Some(new.clone());
            Ok(json!({ "layerId": new }))
        }),
        "layer.delete" => s.edit(cx.label(), cx.source, None, |d| {
            let ids = util::layer_ids(d, &a, "layerIds")?;
            for id in &ids {
                if let Some(l) = d.layer(id) {
                    util::unlocked(l)?;
                }
            }
            let mut removed = vec![];
            for id in ids {
                if d.remove(&id).is_some() {
                    removed.push(id);
                }
            }
            Ok(json!({ "removed": removed }))
        }),
        "layer.group" => s.edit(cx.label(), cx.source, None, |d| {
            let ids: Vec<String> = a.strings("layerIds").iter().map(|k| d.resolve(k)).collect::<Result<_, _>>()?;
            if ids.is_empty() {
                return Err("Give the layers to group.".into());
            }
            // In stack order (top first), into the place of the topmost.
            let mut locs: Vec<(nori_core::Loc, String)> = ids.iter().filter_map(|id| Some((d.path_of(id)?, id.clone()))).collect();
            locs.sort_by(|a, b| a.0.path.cmp(&b.0.path));
            let anchor = locs[0].1.clone();
            let gid = d.new_id();
            let name = a.opt_str("name").map(str::to_string).unwrap_or_else(|| d.new_name("Group"));
            let placeholder = Layer::new(gid.clone(), name, Content::Group { children: vec![], expanded: true });
            d.insert_above(placeholder, Some(&anchor));
            let mut kids = vec![];
            for (_, id) in &locs {
                if let Some(l) = d.remove(id) {
                    kids.push(l);
                }
            }
            if let Some(g) = d.layer_mut(&gid).and_then(Layer::children_mut) {
                *g = kids;
            }
            d.active = Some(gid.clone());
            Ok(json!({ "layerId": gid }))
        }),
        "layer.ungroup" => s.edit(cx.label(), cx.source, None, |d| {
            let id = util::layer_id(d, &a)?;
            let loc = d.path_of(&id).ok_or("no layer")?;
            let g = d.layer(&id).ok_or("no layer")?;
            if !g.is_group() {
                return Err(format!("`{}` isn't a group.", g.name));
            }
            let g = d.remove(&id).ok_or("no layer")?;
            let kids = g.children().to_vec();
            let ids: Vec<String> = kids.iter().map(|k| k.id.clone()).collect();
            let i = *loc.path.last().unwrap_or(&0);
            let list = d.siblings_mut(&loc).ok_or("no list")?;
            for (k, child) in kids.into_iter().enumerate() {
                list.insert((i + k).min(list.len()), child);
            }
            d.active = ids.first().cloned();
            Ok(json!({ "layers": ids }))
        }),
        "layer.merge" => merge(s, cx, &a).await,
        "layer.flatten" => {
            let (doc, rev, doc_id) = s.snapshot()?;
            let page = doc.page().clone();
            let rgba = tokio::task::spawn_blocking(move || nori_render::flatten_page(&doc, &page)).await.map_err(|e| e.to_string())?;
            s.edit_at(doc_id,rev, cx.label(), cx.source, move |d| {
                let (w, h) = (d.width(), d.height());
                let id = d.new_id();
                let page = d.page_mut();
                page.layers = vec![Layer::new(id.clone(), "Background", Content::Raster { x: 0, y: 0, pixels: Raster::from_rgba(w, h, &rgba) })];
                d.active = Some(id.clone());
                Ok(json!({ "layerId": id }))
            })
        }
        "layer.rasterize" => {
            let (doc, rev, doc_id) = s.snapshot()?;
            let id = util::layer_id(&doc, &a)?;
            let pixels = tokio::task::spawn_blocking({
                let id = id.clone();
                move || rasterized(&doc, &id)
            })
            .await
            .map_err(|e| e.to_string())??;
            s.edit_at(doc_id,rev, cx.label(), cx.source, move |d| {
                let l = d.layer_mut(&id).ok_or("no layer")?;
                util::unlocked(l)?;
                let (x, y, px) = pixels;
                l.content = Content::Raster { x, y, pixels: px };
                l.smart_source = None;
                l.blend = if l.blend == BlendMode::PassThrough { BlendMode::Normal } else { l.blend };
                Ok(json!({ "layerId": id }))
            })
        }
        "layer.setAdjustment" => s.edit(cx.label(), cx.source, a.coalesce(), |d| {
            let id = util::layer_id(d, &a)?;
            let l = d.layer_mut(&id).ok_or("no layer")?;
            let name = l.name.clone();
            let Content::Adjustment { adjustment } = &mut l.content else { return Err(format!("`{name}` isn't an adjustment layer.")) };
            *adjustment = adjustment.merge(a.object("settings").ok_or("settings is required")?)?;
            Ok(json!({ "layerId": id, "adjustment": adjustment }))
        }),
        "layer.addMask" => s.edit(cx.label(), cx.source, None, |d| {
            let id = util::layer_id(d, &a)?;
            let (w, h) = d.page_of(&id).map(|p| (p.width, p.height)).ok_or("no page")?;
            let mask = match a.opt_str("from").unwrap_or("reveal") {
                "reveal" | "white" => Mask::filled(w, h, 255),
                "hide" | "black" => Mask::filled(w, h, 0),
                "selection" => d.selection.clone().filter(|m| m.width() == w && m.height() == h).ok_or("There is no selection on this page.")?,
                other => return Err(format!("`{other}`: from is reveal, hide or selection.")),
            };
            let l = d.layer_mut(&id).ok_or("no layer")?;
            l.mask = Some(LayerMask { mask, enabled: true });
            Ok(json!({ "layerId": id, "mask": true }))
        }),
        "layer.mask" => s.edit(cx.label(), cx.source, None, |d| {
            let id = util::layer_id(d, &a)?;
            let action = a.str("action")?.to_ascii_lowercase();
            let l = d.layer_mut(&id).ok_or("no layer")?;
            let name = l.name.clone();
            let Some(m) = &mut l.mask else { return Err(format!("`{name}` has no mask (layer.addMask).")) };
            match action.as_str() {
                "enable" => m.enabled = true,
                "disable" => m.enabled = false,
                "invert" => m.mask = m.mask.inverted(),
                "delete" => l.mask = None,
                "apply" => {
                    let mask = m.mask.clone();
                    let Some((x, y, px)) = l.raster_mut() else { return Err(format!("`{name}` isn't pixels: only a pixel layer's mask can be applied (layer.rasterize first).")) };
                    let r = Rect::new(*x, *y, px.width(), px.height());
                    let mut data = px.read_rect(Rect::new(0, 0, r.w, r.h));
                    let mv = mask.read_rect(r);
                    for (i, p) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        p[3] = ((p[3] as u32 * mv[i] as u32 + 127) / 255) as u8;
                    }
                    px.write_rect(Rect::new(0, 0, r.w, r.h), &data);
                    px.compact();
                    l.mask = None;
                }
                other => return Err(format!("`{other}`: action is enable, disable, invert, apply or delete.")),
            }
            Ok(json!({ "layerId": id, "action": action }))
        }),
        "layer.transform" => transform(s, cx, &a).await,
        "layer.look" => {
            let d = s.document()?;
            let id = util::layer_id(&d, &a)?;
            let width = a.opt_u32("width").unwrap_or(768).clamp(16, 4096);
            let s2 = s.clone();
            tokio::task::spawn_blocking(move || {
                let (w, h, rgba) = nori_render::composite::layer_image(&d, &id, width).ok_or("no layer")?;
                let rgba = super::page::checker(&rgba, w, h);
                let path = util::save_look(&s2, &format!("layer-{id}"), w, h, &rgba)?;
                Ok(json!({ "path": path, "width": w, "height": h, "layerId": id }))
            })
            .await
            .map_err(|e| e.to_string())?
        }
        _ => Err(super::unhandled(cx)),
    }
}

/// Where a new layer goes: `above`, else above the active layer.
fn target_above(d: &Document, a: &Args) -> CmdResult<Option<String>> {
    match a.opt_str("above") {
        Some(k) => Ok(Some(d.resolve(k)?)),
        None => Ok(d.active_id()),
    }
}

/// Adds a layer above the target; it becomes active. Returns `{layerId}`.
pub fn add(d: &mut Document, a: &Args, content: Content, base: &str) -> CmdResult {
    let id = d.new_id();
    let name = a.opt_str("name").filter(|n| !n.trim().is_empty()).map(str::to_string).unwrap_or_else(|| d.new_name(base));
    let above = target_above(d, a)?;
    // Above a group's member: into that group; above a collapsed group: beside it.
    d.insert_above(Layer::new(id.clone(), name.clone(), content), above.as_deref());
    d.active = Some(id.clone());
    Ok(json!({ "layerId": id, "name": name }))
}

/// A layer drawn into pixels on its page: where they go and the raster.
pub fn rasterized(d: &Document, id: &str) -> CmdResult<(i32, i32, Raster)> {
    let l = d.layer(id).ok_or("no layer")?;
    if let Some((x, y, p)) = l.raster() {
        return Ok((x, y, p.clone()));
    }
    let page = d.page_of(id).ok_or("no page")?;
    let mut alone = page.clone();
    alone.master = None;
    let mut solo = l.clone();
    solo.opacity = 1.0;
    solo.blend = BlendMode::Normal;
    solo.mask = None;
    solo.clipped = false;
    solo.visible = true;
    if matches!(l.content, Content::Adjustment { .. }) {
        return Err(format!("`{}` is an adjustment: it has no pixels of its own (merge it down instead).", l.name));
    }
    alone.layers = vec![solo];
    let rgba = nori_render::flatten_page(d, &alone);
    let full = Raster::from_rgba(page.width, page.height, &rgba);
    let b = full.content_bounds().unwrap_or(Rect::new(0, 0, 1, 1));
    Ok((b.x, b.y, Raster::from_rgba(b.w, b.h, &full.read_rect(b))))
}

async fn merge(s: &Arc<Session>, cx: &Ctx, a: &Args) -> CmdResult {
    let (doc, rev, doc_id) = s.snapshot()?;
    let mode = a.opt_str("mode").unwrap_or("down").to_string();
    if mode == "visible" {
        let page = doc.page().clone();
        let rgba = tokio::task::spawn_blocking(move || nori_render::flatten_page(&doc, &page)).await.map_err(|e| e.to_string())?;
        return s.edit_at(doc_id,rev, cx.label(), cx.source, move |d| {
            let (w, h) = (d.width(), d.height());
            let id = d.new_id();
            let page = d.page_mut();
            let hidden: Vec<Layer> = page.layers.iter().filter(|l| !l.visible).cloned().collect();
            page.layers = hidden;
            page.layers.insert(0, Layer::new(id.clone(), "Merged", Content::Raster { x: 0, y: 0, pixels: Raster::from_rgba(w, h, &rgba) }));
            d.active = Some(id.clone());
            Ok(json!({ "layerId": id }))
        });
    }
    let top = util::layer_id(&doc, a)?;
    let loc = doc.path_of(&top).ok_or("no layer")?;
    let i = *loc.path.last().unwrap_or(&0);
    let below_loc = nori_core::Loc { page: loc.page, path: { let mut p = loc.path.clone(); *p.last_mut().expect("path") = i + 1; p } };
    let below = doc.at(&below_loc).ok_or("There is no layer under it to merge into.")?.clone();
    let top_layer = doc.layer(&top).ok_or("no layer")?.clone();
    // The two drawn together, on their page.
    let page = doc.page_at(loc.page).ok_or("no page")?.clone();
    let (bid, tid) = (below.id.clone(), top.clone());
    let rgba = tokio::task::spawn_blocking(move || {
        let mut alone = page.clone();
        alone.master = None;
        let mut b = below;
        b.visible = true;
        b.opacity = 1.0;
        b.blend = BlendMode::Normal;
        alone.layers = vec![top_layer, b];
        (nori_render::flatten_page(&doc, &alone), alone.width, alone.height)
    })
    .await
    .map_err(|e| e.to_string())?;
    s.edit_at(doc_id,rev, cx.label(), cx.source, move |d| {
        let (rgba, w, h) = rgba;
        let full = Raster::from_rgba(w, h, &rgba);
        let bnd = full.content_bounds().unwrap_or(Rect::new(0, 0, 1, 1));
        let l = d.layer_mut(&bid).ok_or("no layer")?;
        util::unlocked(l)?;
        let opacity = l.opacity;
        l.content = Content::Raster { x: bnd.x, y: bnd.y, pixels: Raster::from_rgba(bnd.w, bnd.h, &full.read_rect(bnd)) };
        l.opacity = opacity;
        d.remove(&tid);
        d.active = Some(bid.clone());
        Ok(json!({ "layerId": bid }))
    })
}

async fn transform(s: &Arc<Session>, cx: &Ctx, a: &Args) -> CmdResult {
    let (doc, rev, doc_id) = s.snapshot()?;
    let id = util::layer_id(&doc, a)?;
    let l = doc.layer(&id).ok_or("no layer")?.clone();
    util::unlocked(&l)?;
    let b = util::bounds(&doc, &l).ok_or("That layer has nothing to transform.")?;
    let k = a.opt_f64("scale").unwrap_or(1.0) as f32;
    let (mut sx, mut sy) = (a.opt_f64("scaleX").map_or(k, |v| v as f32), a.opt_f64("scaleY").map_or(k, |v| v as f32));
    match a.opt_str("flip").map(str::to_ascii_lowercase).as_deref() {
        Some(f) if f.starts_with('h') => sx = -sx,
        Some(f) if f.starts_with('v') => sy = -sy,
        Some(other) => return Err(format!("`{other}`: flip is horizontal or vertical.")),
        None => {}
    }
    if sx.abs() < 1e-3 || sy.abs() < 1e-3 || sx.abs() > 100.0 || sy.abs() > 100.0 {
        return Err("Scales go from 0.001 to 100.".into());
    }
    let rot = a.opt_f64("rotate").unwrap_or(0.0) as f32;
    let (dx, dy) = (a.opt_f64("dx").unwrap_or(0.0) as f32, a.opt_f64("dy").unwrap_or(0.0) as f32);
    let cxp = a.opt_f64("originX").map_or(b.x as f32 + b.w as f32 / 2.0, |v| v as f32);
    let cyp = a.opt_f64("originY").map_or(b.y as f32 + b.h as f32 / 2.0, |v| v as f32);
    let filter = nori_render::transform::Filter::parse(a.opt_str("filter").unwrap_or("bicubic"))?;
    let (sin, cos) = rot.to_radians().sin_cos();
    // Page → page: about (cx, cy), scale then turn, then move.
    let m: [f32; 6] = [cos * sx, sin * sx, -sin * sy, cos * sy, 0.0, 0.0];
    let m = [m[0], m[1], m[2], m[3], cxp - (m[0] * cxp + m[2] * cyp) + dx, cyp - (m[1] * cxp + m[3] * cyp) + dy];
    let new = tokio::task::spawn_blocking(move || -> CmdResult<Layer> {
        fn apply(l: &mut Layer, m: &[f32; 6], p: (f32, f32, f32, f32, f32, f32, f32, f32), filter: nori_render::transform::Filter) -> CmdResult<()> {
            let (sx, sy, rot, cx, cy, dx, dy, _) = p;
            if l.smart_source.is_some() { return Err("Use layer.resizeSmartObject to resize from the original, or rasterize before rotating/flipping.".into()); }
            match &mut l.content {
                Content::Raster { x, y, pixels } => {
                    let (np, nx, ny) = nori_render::transform::affine(pixels, *x, *y, sx, sy, rot, cx, cy, dx, dy, filter);
                    *pixels = np;
                    *x = nx;
                    *y = ny;
                }
                Content::Vector { shape } => {
                    let t = shape.transform;
                    shape.transform = [
                        m[0] * t[0] + m[2] * t[1],
                        m[1] * t[0] + m[3] * t[1],
                        m[0] * t[2] + m[2] * t[3],
                        m[1] * t[2] + m[3] * t[3],
                        m[0] * t[4] + m[2] * t[5] + m[4],
                        m[1] * t[4] + m[3] * t[5] + m[5],
                    ];
                    if let Some(st) = &mut shape.stroke {
                        st.width *= ((sx.abs() * sy.abs()).sqrt()).max(0.001);
                    }
                }
                Content::Text { text } => {
                    if rot.abs() > 0.01 || sx < 0.0 || sy < 0.0 {
                        return Err(format!("Text can be scaled and moved but not turned or flipped: rasterize `{}` first (layer.rasterize) or turn a copy.", l.name));
                    }
                    let k = (sx + sy) / 2.0;
                    let (nx, ny) = (m[0] * text.x + m[2] * text.y + m[4], m[1] * text.x + m[3] * text.y + m[5]);
                    text.x = nx;
                    text.y = ny;
                    text.size *= k;
                    text.letter_spacing *= k;
                    if let Some(f) = &mut text.frame {
                        f[0] *= sx;
                        f[1] *= sy;
                    }
                }
                Content::Group { children, .. } => {
                    for c in children.iter_mut() {
                        apply(c, m, p, filter)?;
                    }
                }
                Content::Fill { .. } | Content::Adjustment { .. } => {}
            }
            Ok(())
        }
        let mut l = l;
        apply(&mut l, &m, (sx, sy, rot, cxp, cyp, dx, dy, 0.0), filter)?;
        Ok(l)
    })
    .await
    .map_err(|e| e.to_string())??;
    s.edit_at(doc_id,rev, cx.label(), cx.source, move |d| {
        let slot = d.layer_mut(&id).ok_or("no layer")?;
        *slot = new;
        let nb = d.layer(&id).and_then(|l| util::bounds(d, l));
        Ok(json!({ "layerId": id, "bounds": nb.map(|b| [b.x, b.y, b.w as i32, b.h as i32]) }))
    })
}

#[allow(dead_code)]
fn _unused(_: Color, _: Value) {}
