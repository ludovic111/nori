//! `doc.*`: the document as a whole: new, open, save, overview, size, crop, turn, batches.

use std::sync::Arc;
use std::time::Duration;

use nori_core::{Color, Content, Document, Layer, Page, Raster, Rect};
use serde_json::{Value, json};

use super::util;
use crate::registry::{self, Args, Ctx};
use crate::session::{CmdResult, DocId, Session, Source};

const BATCH_WAIT: Duration = Duration::from_secs(5);

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "doc.overview" => overview(s),
        "doc.get" => Ok(json!(s.document()?)),
        "doc.new" => new(s, &a),
        "doc.open" => {
            let path = util::path(&a, "path")?;
            let p = path.clone();
            let doc = tokio::task::spawn_blocking(move || nori_io::open(&p)).await.map_err(|e| e.to_string())??;
            let is_nori = nori_io::format_of(&path).is_some_and(|f| f.id == "nori");
            let (pages, layers, w, h) = (doc.pages.len(), doc.all().len(), doc.width(), doc.height());
            let name = doc.name.clone();
            s.open_doc(doc, is_nori.then(|| path.clone()), (!is_nori).then(|| path.clone()), false);
            s.remember(&path);
            Ok(json!({ "opened": path.display().to_string(), "name": name, "width": w, "height": h, "pages": pages, "layers": layers }))
        }
        "doc.save" => {
            let path = a.opt_str("path").map(|_| util::path(&a, "path")).transpose()?;
            let s2 = s.clone();
            let saved = tokio::task::spawn_blocking(move || s2.save(path)).await.map_err(|e| e.to_string())??;
            Ok(json!({ "saved": saved.display().to_string() }))
        }
        "doc.close" => {
            s.close_doc();
            Ok(json!({ "closed": true }))
        }
        "doc.setInfo" => s.edit(cx.label(), cx.source, None, |d| {
            if let Some(n) = a.opt_str("name") {
                d.name = n.trim().to_string();
            }
            if let Some(dpi) = a.opt_f64("dpi") {
                if !(1.0..=10_000.0).contains(&dpi) {
                    return Err("dpi goes from 1 to 10000".into());
                }
                d.dpi = dpi as f32;
            }
            if let Some(u) = a.opt_str("units") {
                d.units = serde_json::from_value(json!(u)).map_err(|_| "units are px, pt, mm or in".to_string())?;
            }
            Ok(json!({ "name": d.name, "dpi": d.dpi, "units": d.units }))
        }),
        "doc.resize" => resize(s, cx, &a).await,
        "doc.resizeCanvas" => s.edit(cx.label(), cx.source, None, |d| {
            let r = util::page_ref(d, &a)?;
            let (w, h) = (a.opt_u32("width").unwrap_or(1), a.opt_u32("height").unwrap_or(1));
            if w == 0 || h == 0 || w > nori_core::MAX_SIDE || h > nori_core::MAX_SIDE {
                return Err(format!("A page goes from 1 to {} pixels a side.", nori_core::MAX_SIDE));
            }
            let page = d.page_at_mut(r).ok_or("no page")?;
            let (ow, oh) = (page.width as i32, page.height as i32);
            let anchor = a.opt_str("anchor").unwrap_or("center").to_ascii_lowercase();
            let fx = if anchor.contains("left") { 0.0 } else if anchor.contains("right") { 1.0 } else { 0.5 };
            let fy = if anchor.contains("top") { 0.0 } else if anchor.contains("bottom") { 1.0 } else { 0.5 };
            let dx = ((w as i32 - ow) as f32 * fx).round();
            let dy = ((h as i32 - oh) as f32 * fy).round();
            for l in page.layers.iter_mut() {
                shift(l, dx, dy);
            }
            page.width = w;
            page.height = h;
            for l in page.layers.iter_mut() {
                reframe_masks(l, w, h, -dx as i32, -dy as i32);
            }
            d.selection = None;
            Ok(json!({ "width": w, "height": h, "moved": [dx, dy] }))
        }),
        "doc.crop" => s.edit(cx.label(), cx.source, None, |d| {
            let r = match (a.opt_i64("x"), a.opt_i64("y"), a.opt_u32("width"), a.opt_u32("height")) {
                (Some(x), Some(y), Some(w), Some(h)) => Rect::new(x as i32, y as i32, w, h),
                _ => d.selection_bounds(),
            };
            let r = r.intersect(&d.bounds());
            if r.is_empty() {
                return Err("The crop rectangle is outside the page.".into());
            }
            let page = d.page_mut();
            for l in page.layers.iter_mut() {
                shift(l, -r.x as f32, -r.y as f32);
                trim(l, r.w, r.h);
                reframe_masks(l, r.w, r.h, r.x, r.y);
            }
            page.width = r.w;
            page.height = r.h;
            d.selection = None;
            Ok(json!({ "width": r.w, "height": r.h }))
        }),
        "doc.rotate" => rotate(s, cx, &a),
        "doc.batch" => batch(s, cx, a).await,
        "doc.recent" => {
            let recent: Vec<Value> = s
                .settings()
                .recent
                .iter()
                .map(|p| {
                    let path = std::path::Path::new(p);
                    json!({ "path": p, "exists": path.exists(), "name": path.file_name().map(|n| n.to_string_lossy().into_owned()) })
                })
                .collect();
            Ok(json!({ "recent": recent }))
        }
        _ => Err(super::unhandled(cx)),
    }
}

/// Moves a layer (and everything in it) by (dx, dy).
pub fn shift(l: &mut Layer, dx: f32, dy: f32) {
    match &mut l.content {
        Content::Raster { x, y, .. } => {
            *x += dx.round() as i32;
            *y += dy.round() as i32;
        }
        Content::Text { text } => {
            text.x += dx;
            text.y += dy;
        }
        Content::Vector { shape } => {
            if shape.transform == nori_core::vector::IDENTITY {
                shape.geometry.translate(dx, dy);
            } else {
                shape.transform[4] += dx;
                shape.transform[5] += dy;
            }
            let move_paint = |p: &mut nori_core::Paint| match p {
                nori_core::Paint::Linear { x1, y1, x2, y2, .. } => {
                    *x1 += dx;
                    *y1 += dy;
                    *x2 += dx;
                    *y2 += dy;
                }
                nori_core::Paint::Radial { cx, cy, .. } => {
                    *cx += dx;
                    *cy += dy;
                }
                _ => {}
            };
            move_paint(&mut shape.fill);
            if let Some(s) = &mut shape.stroke {
                move_paint(&mut s.paint);
            }
        }
        Content::Group { children, .. } => children.iter_mut().for_each(|c| shift(c, dx, dy)),
        Content::Fill { .. } | Content::Adjustment { .. } => {}
    }
}

/// Cuts pixels outside a `w`×`h` page (after a crop).
fn trim(l: &mut Layer, w: u32, h: u32) {
    match &mut l.content {
        Content::Raster { x, y, pixels } => {
            let keep = Rect::new(*x, *y, pixels.width(), pixels.height()).intersect(&Rect::new(0, 0, w, h));
            if keep.is_empty() {
                *pixels = Raster::transparent(1, 1);
                *x = 0;
                *y = 0;
                return;
            }
            let data = pixels.read_rect(keep.translate(-*x, -*y));
            *pixels = Raster::from_rgba(keep.w, keep.h, &data);
            *x = keep.x;
            *y = keep.y;
        }
        Content::Group { children, .. } => children.iter_mut().for_each(|c| trim(c, w, h)),
        _ => {}
    }
}

/// Masks are page-sized: after a page changes size, each one is cut or padded to it (the old
/// page's (ox, oy) is the new one's origin).
fn reframe_masks(l: &mut Layer, w: u32, h: u32, ox: i32, oy: i32) {
    if let Some(m) = &mut l.mask {
        let fill = m.mask.fill();
        let data = m.mask.read_rect(Rect::new(ox, oy, w, h));
        m.mask = nori_core::Mask::from_vec(w, h, fill, &data);
    }
    if let Content::Group { children, .. } = &mut l.content {
        children.iter_mut().for_each(|c| reframe_masks(c, w, h, ox, oy));
    }
}

fn preset(name: &str) -> Option<(u32, u32, f32)> {
    Some(match name.trim().to_ascii_lowercase().replace([' ', '_'], "-").as_str() {
        "screen" | "hd" | "1080p" => (1920, 1080, 72.0),
        "4k" => (3840, 2160, 72.0),
        "square" => (2048, 2048, 72.0),
        "a4" => (2480, 3508, 300.0),
        "a4-landscape" => (3508, 2480, 300.0),
        "a5" => (1748, 2480, 300.0),
        "a3" => (3508, 4961, 300.0),
        "letter" => (2550, 3300, 300.0),
        "poster-a2" | "a2" => (4961, 7016, 300.0),
        "story" => (1080, 1920, 72.0),
        "instagram" | "portrait" => (1080, 1350, 72.0),
        "card" | "business-card" => (1050, 600, 300.0),
        _ => return None,
    })
}

fn new(s: &Arc<Session>, a: &Args) -> CmdResult {
    let (mut w, mut h, mut dpi) = (a.opt_u32("width").unwrap_or(1920), a.opt_u32("height").unwrap_or(1080), a.opt_f64("dpi").map(|v| v as f32).unwrap_or(72.0));
    let print = match a.opt_str("preset") {
        Some(p) => {
            let (pw, ph, pd) = preset(p).ok_or_else(|| format!("No preset `{p}`: screen, 4k, square, a4, a4-landscape, a5, a3, letter, poster-a2, story, instagram, card."))?;
            (w, h) = (pw, ph);
            if a.opt_f64("dpi").is_none() {
                dpi = pd;
            }
            pd > 72.0
        }
        None => dpi > 72.0,
    };
    if w == 0 || h == 0 || w > nori_core::MAX_SIDE || h > nori_core::MAX_SIDE {
        return Err(format!("A page goes from 1 to {} pixels a side.", nori_core::MAX_SIDE));
    }
    let name = a.opt_str("name").filter(|n| !n.trim().is_empty()).unwrap_or("Untitled").to_string();
    let bg = match a.opt_str("background").map(str::trim) {
        Some("transparent" | "none" | "") => None,
        Some(c) => Some(Color::parse(c)?),
        None => Some(Color::WHITE),
    };
    let pages = a.opt_u32("pages").unwrap_or(1).clamp(1, 999);
    let mut d = Document::empty(name.clone(), w, h);
    d.dpi = dpi;
    let margins = a.opt_f64("margins").unwrap_or(0.0) as f32;
    let columns = a.opt_u32("columns").unwrap_or(1).clamp(1, 24);
    for i in 0..pages {
        if i > 0 {
            let id = d.new_page_id();
            d.pages.push(Page::new(id, format!("Page {}", i + 1), w, h));
        }
        let p = &mut d.pages[i as usize];
        p.margins = nori_core::Margins { top: margins, right: margins, bottom: margins, left: margins };
        p.columns = columns;
        p.gutter = if columns > 1 { (w as f32 * 0.02).round() } else { 0.0 };
    }
    // Screen pictures get a pixel background to paint on (as other photo editors do); print
    // pages and booklets get paper (a fill layer: no pixels to keep per page).
    let layout = pages > 1 || print;
    for i in 0..pages as usize {
        let id = d.new_id();
        let layer = match (bg, layout) {
            (Some(c), false) => Layer::new(id.clone(), "Background", Content::Raster { x: 0, y: 0, pixels: Raster::solid(w, h, c.to_u8()) }),
            (Some(c), true) => Layer::new(id.clone(), "Paper", Content::Fill { color: c }),
            (None, _) => Layer::new(id.clone(), "Layer 1", Content::Raster { x: 0, y: 0, pixels: Raster::transparent(w, h) }),
        };
        d.pages[i].layers.push(layer);
        if i == 0 {
            d.active = Some(id);
        }
    }
    let first = d.pages[0].id.clone();
    d.active_page = first;
    s.open_doc(d, None, None, false);
    Ok(json!({ "name": name, "width": w, "height": h, "dpi": dpi, "pages": pages }))
}

async fn resize(s: &Arc<Session>, cx: &Ctx, a: &Args) -> CmdResult {
    let (doc, rev, _) = s.snapshot()?;
    let (w, h) = (doc.width() as f64, doc.height() as f64);
    let k = match (a.opt_f64("scale"), a.opt_f64("width"), a.opt_f64("height")) {
        (Some(k), _, _) => k,
        (None, Some(nw), _) => nw / w,
        (None, None, Some(nh)) => nh / h,
        _ => return Err("Give width, height or scale.".into()),
    } as f32;
    if !(0.001..=64.0).contains(&k) || (w * k as f64).round() < 1.0 || (w * k as f64) > nori_core::MAX_SIDE as f64 || (h * k as f64) > nori_core::MAX_SIDE as f64 {
        return Err("That size is out of range.".into());
    }
    let filter = nori_render::transform::Filter::parse(a.opt_str("filter").unwrap_or("bicubic"))?;
    let scaled = tokio::task::spawn_blocking(move || nori_render::transform::scale_document(&doc, k, filter)).await.map_err(|e| e.to_string())?;
    let (nw, nh) = (scaled.width(), scaled.height());
    s.edit_at(rev, cx.label(), cx.source, move |d| {
        *d = scaled;
        Ok(json!({ "width": nw, "height": nh, "scale": k }))
    })
}

fn rotate(s: &Arc<Session>, cx: &Ctx, a: &Args) -> CmdResult {
    let q = a.opt_i64("quarters").unwrap_or(0).rem_euclid(4) as i32;
    let flip = a.opt_str("flip").map(str::to_ascii_lowercase);
    if q == 0 && flip.is_none() {
        return Err("Give quarters (1, 2, 3, -1) or flip (horizontal or vertical).".into());
    }
    s.edit(cx.label(), cx.source, None, |d| {
        let prep = nori_render::prepare(d);
        let doc = d.clone();
        let page = d.page_mut();
        let (w, h) = (page.width as f32, page.height as f32);
        let mut rasterized = vec![];
        fn walk(l: &mut Layer, doc: &Document, prep: &nori_render::Prepared, w: f32, h: f32, q: i32, flip: Option<&str>, rasterized: &mut Vec<String>) -> Result<(), String> {
            // Text can't turn: it becomes pixels first.
            if matches!(l.content, Content::Text { .. }) && (q != 0 || flip.is_some()) {
                let img = prep.text(&l.id).cloned().unwrap_or_default();
                let _ = doc;
                l.content = Content::Raster { x: img.rect.x, y: img.rect.y, pixels: Raster::from_rgba(img.rect.w.max(1), img.rect.h.max(1), &if img.rgba.is_empty() { vec![0; (img.rect.w.max(1) * img.rect.h.max(1) * 4) as usize] } else { img.rgba.clone() }) };
                rasterized.push(l.name.clone());
            }
            if let Some(m) = &mut l.mask {
                let grey: Vec<u8> = m.mask.to_vec().iter().flat_map(|v| [*v, *v, *v, 255]).collect();
                let r = Raster::from_rgba(m.mask.width(), m.mask.height(), &grey);
                let r = match flip {
                    Some(f) => nori_render::transform::flip(&r, f.starts_with('h')),
                    None => nori_render::transform::rotate90(&r, q),
                };
                let fill = m.mask.fill();
                m.mask = nori_core::Mask::from_vec(r.width(), r.height(), fill, &r.to_vec().chunks_exact(4).map(|p| p[0]).collect::<Vec<u8>>());
            }
            // Where a point of the page goes.
            let map = |px: f32, py: f32| -> (f32, f32) {
                match flip {
                    Some(f) if f.starts_with('h') => (w - px, py),
                    Some(_) => (px, h - py),
                    None => match q {
                        1 => (h - py, px),
                        2 => (w - px, h - py),
                        _ => (py, w - px),
                    },
                }
            };
            match &mut l.content {
                Content::Raster { x, y, pixels } => {
                    let (pw, ph) = (pixels.width() as f32, pixels.height() as f32);
                    let corners = [map(*x as f32, *y as f32), map(*x as f32 + pw, *y as f32 + ph)];
                    *pixels = match flip {
                        Some(f) => nori_render::transform::flip(pixels, f.starts_with('h')),
                        None => nori_render::transform::rotate90(pixels, q),
                    };
                    *x = corners[0].0.min(corners[1].0).round() as i32;
                    *y = corners[0].1.min(corners[1].1).round() as i32;
                }
                Content::Vector { shape } => {
                    // Page mapping as an affine matrix, applied after the shape's own.
                    let m: [f32; 6] = match flip {
                        Some(f) if f.starts_with('h') => [-1.0, 0.0, 0.0, 1.0, w, 0.0],
                        Some(_) => [1.0, 0.0, 0.0, -1.0, 0.0, h],
                        None => match q {
                            1 => [0.0, 1.0, -1.0, 0.0, h, 0.0],
                            2 => [-1.0, 0.0, 0.0, -1.0, w, h],
                            _ => [0.0, -1.0, 1.0, 0.0, 0.0, w],
                        },
                    };
                    let t = shape.transform;
                    shape.transform = [
                        m[0] * t[0] + m[2] * t[1],
                        m[1] * t[0] + m[3] * t[1],
                        m[0] * t[2] + m[2] * t[3],
                        m[1] * t[2] + m[3] * t[3],
                        m[0] * t[4] + m[2] * t[5] + m[4],
                        m[1] * t[4] + m[3] * t[5] + m[5],
                    ];
                }
                Content::Group { children, .. } => {
                    for c in children.iter_mut() {
                        walk(c, doc, prep, w, h, q, flip, rasterized)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        for l in page.layers.iter_mut() {
            walk(l, &doc, &prep, w, h, q, flip.as_deref(), &mut rasterized)?;
        }
        if flip.is_none() && q % 2 == 1 {
            std::mem::swap(&mut page.width, &mut page.height);
        }
        let (nw, nh) = (page.width, page.height);
        d.selection = None;
        Ok(json!({ "width": nw, "height": nh, "rasterized": rasterized }))
    })
}

/// The document in one answer.
fn overview(s: &Arc<Session>) -> CmdResult {
    let (d, _, _) = s.snapshot()?;
    let (path, source, dirty) = s.paths().unwrap_or((None, None, false));
    let (undo, redo) = s.read(|ed| (ed.undo_steps(), ed.redo_steps()))?;
    let prep = nori_render::prepare(&d);
    let mut problems: Vec<String> = vec![];
    let pages: Vec<Value> = d
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut v = util::describe_page(&d, p);
            v["number"] = json!(i + 1);
            v["layers"] = Value::Array(p.layers.iter().map(|l| util::describe(&d, l, 0)).collect());
            for l in p.all() {
                if let Content::Text { .. } = l.content
                    && prep.text(&l.id).is_some_and(|t| t.overflow)
                {
                    problems.push(format!("Text `{}` ({}) on page {} doesn't fit its frame: make it bigger, smaller type, or thread it to another frame (text.thread).", l.name, l.id, i + 1));
                }
                if !l.visible {
                    problems.push(format!("`{}` ({}) on page {} is hidden.", l.name, l.id, i + 1));
                }
            }
            v
        })
        .collect();
    let masters: Vec<Value> = d.masters.iter().map(|p| {
        let mut v = util::describe_page(&d, p);
        v["layers"] = Value::Array(p.layers.iter().map(|l| util::describe(&d, l, 0)).collect());
        v
    }).collect();
    let selection = d.selection.as_ref().map(|m| json!({ "bounds": m.content_bounds().map(|b| json!([b.x, b.y, b.w, b.h])) }));
    let c = *s.colors.read();
    let mut steps: Vec<Value> = undo.iter().rev().take(12).map(|st| json!({ "command": st.label, "by": st.source })).collect();
    steps.reverse();
    Ok(json!({
        "name": d.name,
        "dpi": d.dpi,
        "units": d.units,
        "savedTo": path.map(|p| p.display().to_string()),
        "openedFrom": source.map(|p| p.display().to_string()),
        "unsavedChanges": dirty,
        "activePage": d.active_page,
        "activeLayer": d.active_id(),
        "pages": pages,
        "masters": masters,
        "selection": selection,
        "styles": d.styles,
        "colors": { "foreground": c.foreground, "background": c.background },
        "history": { "undo": undo.len(), "redo": redo.len(), "last": steps },
        "problems": problems,
    }))
}

async fn batch(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    let commands = a.array("commands").cloned().unwrap_or_default();
    let atomic = a.bool_or("atomic", true);
    let label = a.opt_str("label").unwrap_or("batch").to_string();
    let mut calls = Vec::with_capacity(commands.len());
    for (i, c) in commands.iter().enumerate() {
        let name = c.get("command").and_then(Value::as_str).ok_or_else(|| format!("commands[{i}] needs \"command\""))?;
        let spec = registry::spec(name).ok_or_else(|| format!("commands[{i}]: unknown command `{name}`"))?;
        if matches!(spec.name, "doc.batch" | "doc.new" | "doc.open" | "doc.close") {
            return Err(format!("commands[{i}]: `{name}` can't be part of a batch"));
        }
        let params = c.get("params").cloned().unwrap_or(json!({}));
        registry::allowed(s, cx.source, spec)?;
        registry::validate(spec, &params).map_err(|e| format!("commands[{i}]: {e}"))?;
        calls.push((spec, params));
    }
    let open = match s.current_id() {
        Some(_) => {
            let lock = tokio::time::timeout(BATCH_WAIT, s.batch_lock.clone().lock_owned()).await.map_err(|_| "Another doc.batch is still running; try again when it ends.".to_string())?;
            let id = s.begin_batch(&label, cx.source)?;
            Some(BatchGuard { s: s.clone(), id, rollback: atomic, closed: false, _lock: lock })
        }
        None => None,
    };
    let (results, failure) = crate::session::batch_scope(async {
        let mut results = Vec::with_capacity(calls.len());
        let mut failure = None;
        for (i, (spec, params)) in calls.into_iter().enumerate() {
            match registry::call_boxed(s, cx.source, spec, params).await {
                Ok(v) => results.push(json!({ "command": spec.name, "ok": true, "result": v })),
                Err(e) => {
                    results.push(json!({ "command": spec.name, "ok": false, "error": e }));
                    failure = Some(format!("commands[{i}] ({}) failed: {e}", spec.name));
                    if atomic {
                        break;
                    }
                }
            }
        }
        (results, failure)
    })
    .await;
    if let Some(mut g) = open {
        g.rollback = failure.is_some() && atomic;
        g.close()?;
    }
    match failure {
        Some(e) if atomic => Err(format!("{e}. Nothing was changed.")),
        _ => Ok(json!({ "results": results })),
    }
}

/// An open batch; dropped unclosed (the caller gave up), it rolls back or ends.
struct BatchGuard {
    s: Arc<Session>,
    id: DocId,
    rollback: bool,
    closed: bool,
    _lock: tokio::sync::OwnedMutexGuard<()>,
}

impl BatchGuard {
    fn close(&mut self) -> CmdResult<()> {
        self.closed = true;
        self.s.close_batch(self.id, self.rollback)
    }
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.s.close_batch(self.id, self.rollback);
        }
    }
}

#[allow(dead_code)]
fn _source_is_used(_: Source) {}
