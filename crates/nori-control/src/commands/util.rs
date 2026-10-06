//! Helpers the command families share: finding layers and pages from what a client gave, colours
//! and paints from JSON, and describing layers to clients.

use nori_core::vector::{Paint, Stop};
use nori_core::{Color, Content, Document, Layer, Page, PageRef, Rect};
use serde_json::{Value, json};

use crate::registry::Args;
use crate::session::{CmdResult, Session};

/// The layer a command acts on: `layerId` (id or name), else the active layer.
pub fn layer_id(d: &Document, a: &Args) -> CmdResult<String> {
    match a.opt_str("layerId").filter(|s| !s.trim().is_empty()) {
        Some(k) => d.resolve(k),
        None => d.active_id().or_else(|| d.active.clone().filter(|id| d.path_of(id).is_some())).ok_or_else(|| "No layer is active: give layerId (layer.list shows them).".to_string()),
    }
}

pub fn layer_ids(d: &Document, a: &Args, name: &str) -> CmdResult<Vec<String>> {
    let given = a.strings(name);
    if given.is_empty() {
        return Ok(vec![layer_id(d, a)?]);
    }
    given.iter().map(|k| d.resolve(k)).collect()
}

/// The page a command acts on: `page` (id, name, number), else the active page.
pub fn page_ref(d: &Document, a: &Args) -> CmdResult<PageRef> {
    match a.opt_str("page").filter(|s| !s.trim().is_empty()) {
        Some(k) => d.resolve_page(k),
        None => Ok(d.active_ref()),
    }
}

/// A colour from a parameter, else `default`.
pub fn color(a: &Args, name: &str, default: Color) -> CmdResult<Color> {
    match a.opt_str(name).filter(|s| !s.trim().is_empty()) {
        Some(s) => Color::parse(s),
        None => Ok(default),
    }
}

/// A paint from JSON: `"#rrggbb"`, `"none"`, or `{type: solid|linear|radial, …}`.
pub fn paint(v: &Value) -> CmdResult<Paint> {
    match v {
        Value::Null => Ok(Paint::None),
        Value::String(s) if s.trim().is_empty() || s.eq_ignore_ascii_case("none") => Ok(Paint::None),
        Value::String(s) => Ok(Paint::Solid { color: Color::parse(s)? }),
        Value::Object(o) => {
            // Stops may come as ["#000", "#fff"] (evenly spread) or [{offset, color}].
            let mut v = v.clone();
            if let Some(stops) = o.get("stops").and_then(Value::as_array)
                && stops.iter().all(Value::is_string)
            {
                let n = stops.len().max(2) - 1;
                v["stops"] = Value::Array(stops.iter().enumerate().map(|(i, c)| json!({ "offset": i as f64 / n as f64, "color": c })).collect());
            }
            if o.get("type").is_none() && o.get("color").is_some() {
                v["type"] = json!("solid");
            }
            let p: Paint = serde_json::from_value(v).map_err(|e| format!("A paint is \"#rrggbb\", \"none\", or {{\"type\": \"linear\"|\"radial\", …, \"stops\": […]}}: {e}"))?;
            if let Paint::Linear { stops, .. } | Paint::Radial { stops, .. } = &p
                && stops.is_empty()
            {
                return Err("A gradient needs stops.".into());
            }
            Ok(p)
        }
        other => Err(format!("A paint is \"#rrggbb\", \"none\" or an object, not {other}")),
    }
}

pub fn stops2(a: Color, b: Color) -> Vec<Stop> {
    vec![Stop { offset: 0.0, color: a }, Stop { offset: 1.0, color: b }]
}

/// Where a layer draws on its page, if it can say.
pub fn bounds(d: &Document, l: &Layer) -> Option<Rect> {
    match &l.content {
        Content::Raster { .. } => l.raster().and_then(|(x, y, p)| p.content_bounds().map(|b| b.translate(x, y))),
        Content::Text { text } => {
            let r = nori_render::text::measure(text);
            Some(r)
        }
        Content::Vector { shape } => Some(shape.bounds()),
        Content::Group { children, .. } => children.iter().filter_map(|c| bounds(d, c)).reduce(|a, b| a.union(&b)),
        Content::Fill { .. } | Content::Adjustment { .. } => d.page_of(&l.id).map(Page::bounds),
    }
}

fn rect_json(r: Rect) -> Value {
    json!([r.x, r.y, r.w, r.h])
}

/// A layer described for clients (bounded: no pixels, text cut at 400 characters).
pub fn describe(d: &Document, l: &Layer, depth: usize) -> Value {
    let mut v = json!({
        "id": l.id,
        "name": l.name,
        "kind": l.content.kind(),
        "visible": l.visible,
        "opacity": (l.opacity * 1000.0).round() / 1000.0,
        "blend": l.blend.id(),
    });
    let o = v.as_object_mut().expect("object");
    if l.locked {
        o.insert("locked".into(), json!(true));
    }
    if l.clipped {
        o.insert("clipped".into(), json!(true));
    }
    if let Some(m) = &l.mask {
        o.insert("mask".into(), json!({ "enabled": m.enabled }));
    }
    if let Some(b) = bounds(d, l) {
        o.insert("bounds".into(), rect_json(b));
    }
    match &l.content {
        Content::Raster { x, y, pixels } => {
            o.insert("pixels".into(), json!({ "x": x, "y": y, "width": pixels.width(), "height": pixels.height(), "empty": pixels.used_tiles() == 0 }));
        }
        Content::Fill { color } => {
            o.insert("color".into(), json!(color));
        }
        Content::Adjustment { adjustment } => {
            o.insert("adjustment".into(), json!(adjustment));
        }
        Content::Text { text } => {
            let words: String = text.text.chars().take(400).collect();
            let mut t = json!({
                "text": words,
                "font": text.font,
                "size": text.size,
                "weight": text.weight,
                "color": text.color,
                "x": text.x,
                "y": text.y,
                "align": text.align,
            });
            if let Some(f) = text.frame {
                t["frame"] = json!(f);
            }
            if let Some(n) = &text.next {
                t["next"] = json!(n);
            }
            if let Some(s) = &text.style {
                t["style"] = json!(s);
            }
            if !text.runs.is_empty() {
                t["runs"] = json!(text.runs.len());
            }
            o.insert("text".into(), t);
        }
        Content::Vector { shape } => {
            o.insert("shape".into(), json!(shape));
        }
        Content::Group { children, expanded } => {
            o.insert("expanded".into(), json!(expanded));
            if depth < 12 {
                o.insert("children".into(), Value::Array(children.iter().map(|c| describe(d, c, depth + 1)).collect()));
            }
        }
    }
    v
}

pub fn describe_page(d: &Document, p: &Page) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "width": p.width,
        "height": p.height,
        "x": p.x,
        "y": p.y,
        "margins": p.margins,
        "columns": p.columns,
        "gutter": p.gutter,
        "bleed": p.bleed,
        "master": p.master,
        "guides": p.guides,
        "layers": p.all().len(),
        "active": p.id == d.active_page,
    })
}

/// Saves a picture (straight RGBA) as a PNG in the session's looks folder and returns its path.
pub fn save_look(s: &Session, name: &str, w: u32, h: u32, rgba: &[u8]) -> CmdResult<String> {
    let dir = s.data_dir.join("looks");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{name}-{}.png", chrono::Utc::now().format("%H%M%S%3f")));
    std::fs::write(&path, nori_core::file::encode_png_rgba(w, h, rgba)?).map_err(|e| e.to_string())?;
    // Keep the folder small.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = entries.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
        if files.len() > 40 {
            files.sort();
            for (_, p) in files.iter().take(files.len() - 40) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Ok(path.display().to_string())
}

/// A path from a parameter: `~` expanded, relative paths taken from the current folder.
pub fn path(a: &Args, name: &str) -> CmdResult<std::path::PathBuf> {
    let p = a.str(name)?.trim();
    if p.is_empty() {
        return Err(format!("`{name}` is empty"));
    }
    let p = if let Some(rest) = p.strip_prefix("~/") { dirs::home_dir().unwrap_or_default().join(rest) } else { std::path::PathBuf::from(p) };
    Ok(if p.is_absolute() { p } else { std::env::current_dir().unwrap_or_default().join(p) })
}

/// The layer must be one this command can change.
pub fn unlocked(l: &Layer) -> CmdResult<()> {
    if l.locked { Err(format!("`{}` is locked: unlock it first (layer.update locked=false).", l.name)) } else { Ok(()) }
}

/// Gets a layer ready to paint on: its raster grown to cover the whole page (so nothing
/// painted past its old edges is lost). Text, vectors and fills are refused (rasterize them
/// first). Returns where the raster's top-left corner is.
pub fn prepare_paint(d: &mut Document, id: &str) -> CmdResult<(i32, i32)> {
    let (pw, ph) = d.page_of(id).map(|p| (p.width, p.height)).ok_or("That layer has no page")?;
    let l = d.layer_mut(id).ok_or("No such layer")?;
    unlocked(l)?;
    let name = l.name.clone();
    let kind = l.content.kind();
    let Some((x, y, px)) = l.raster_mut() else {
        return Err(format!("`{name}` is a {kind} layer, not pixels: paint on a pixel layer (layer.add) or rasterize it first (layer.rasterize)."));
    };
    let have = Rect::new(*x, *y, px.width(), px.height());
    let page = Rect::new(0, 0, pw, ph);
    if have.intersect(&page) != page {
        let want = have.union(&page);
        let mut grown = nori_core::Raster::transparent(want.w, want.h);
        grown.write_rect(Rect::new(have.x - want.x, have.y - want.y, have.w, have.h), &px.to_vec());
        grown.compact();
        *px = grown;
        *x = want.x;
        *y = want.y;
    }
    Ok((*x, *y))
}

/// A layer's raster (after [`prepare_paint`]).
pub fn raster_of<'a>(d: &'a mut Document, id: &str) -> CmdResult<&'a mut nori_core::Raster> {
    d.layer_mut(id).and_then(|l| l.raster_mut()).map(|(_, _, p)| p).ok_or_else(|| "not a pixel layer".to_string())
}
