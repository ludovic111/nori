//! What the person is looking at when they ask, sent with each request.
//!
//! "Make this bigger", "put a title here", "blur the selection": the words point at the window.
//! Each request to the agent starts with a short `<context>` block (the document, the page, the
//! active layer, the selection, the tool), so the agent knows what "this" is without a round
//! trip, and the panel shows the same thing in one line above the composer. It is part of the
//! person's message, so the thread is only ever appended to (prompt caches and thinking blocks
//! stay valid).

use nori_control::{Session, UiState};
use nori_core::{Content, Document, Layer, Rect};

/// The person's view of nori at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glance {
    /// The `<context>` block's lines.
    pub lines: Vec<String>,
    /// One short line for the panel ("\"Title\" (text) · page 2 of 3").
    pub short: String,
}

impl Glance {
    /// The context block, then the request.
    pub fn frame(&self, prompt: &str) -> String {
        if self.lines.is_empty() {
            return prompt.to_string();
        }
        format!("<context>\n{}\n</context>\n\n{prompt}", self.lines.join("\n"))
    }
}

/// `text` without the context block [`Glance::frame`] put in front of it.
pub fn unframed(text: &str) -> &str {
    match text.strip_prefix("<context>\n").and_then(|rest| rest.split_once("\n</context>\n\n")) {
        Some((_, prompt)) => prompt,
        None => text,
    }
}

/// What the person sees now.
pub fn glance(session: &Session) -> Glance {
    let ui = session.ui_state();
    let saved = session.paths();
    // Under the read lock, without copying the document: the panel asks on every redraw.
    session
        .read(|ed| of(ed.doc(), ed.is_dirty(), saved.as_ref().and_then(|(p, s, _)| p.clone().or_else(|| s.clone())), &ui))
        .unwrap_or_else(|_| Glance { lines: vec!["No document is open in nori.".into()], short: "No document open".into() })
}

fn of(d: &Document, dirty: bool, file: Option<std::path::PathBuf>, ui: &UiState) -> Glance {
    let mut lines = vec!["What the person sees in nori as they ask (\"this\", \"here\" and \"the selected…\" mean these; doc_overview has the rest):".to_string()];
    let page = d.page();
    let (n, of_n) = (d.pages.iter().position(|p| p.id == page.id).map(|i| i + 1), d.pages.len());
    let where_ = match n {
        Some(i) => format!("page {i} of {of_n}"),
        None => "a master page".to_string(),
    };
    let layers = page.all().len();
    let state = match (&file, dirty) {
        (None, _) => "not saved yet".to_string(),
        (Some(f), false) => format!("saved as {}", file_name(f)),
        (Some(f), true) => format!("{} with unsaved changes", file_name(f)),
    };
    lines.push(format!(
        "Document {}: {where_} ({}, id {}, {}×{} px at {} dpi), {layers} layer{} on it; {state}.",
        quoted(&d.name),
        quoted(&page.name),
        page.id,
        page.width,
        page.height,
        trim_num(d.dpi as f64),
        plural(layers)
    ));

    let active = d.active_id().and_then(|id| d.layer(&id));
    if let Some(l) = active {
        lines.push(format!("Active layer: {}.", describe(d, l)));
    } else {
        lines.push("No layer is active.".into());
    }
    let selection = d.selection.as_ref().map(|_| d.selection_bounds()).filter(|r| !r.is_empty());
    if let Some(r) = selection {
        lines.push(format!("Selection: {} (pixel work and filters stay inside it).", rect(r)));
    }
    if ui.screen == "editor" || !ui.tool.is_empty() {
        let mut view = vec![];
        if !ui.tool.is_empty() {
            view.push(format!("tool {}", ui.tool));
        }
        if ui.zoom > 0.0 {
            view.push(format!("zoom {} %", trim_num(ui.zoom * 100.0)));
        }
        if !view.is_empty() {
            lines.push(format!("Window: {}.", view.join(", ")));
        }
    }
    let panels: Vec<&str> = ui.open.iter().map(String::as_str).filter(|o| *o != "agent").collect();
    if !panels.is_empty() {
        lines.push(format!("Also open: {}.", panels.join(", ")));
    }

    let page_short = match n {
        Some(i) if of_n > 1 => format!("page {i} of {of_n}"),
        Some(_) => format!("{}×{}", page.width, page.height),
        None => "master page".into(),
    };
    let short = match (active, selection) {
        (_, Some(r)) => format!("Selection {}×{} · {page_short}", r.w, r.h),
        (Some(l), None) => format!("{} ({}) · {page_short}", quoted(&l.name), l.content.kind()),
        (None, None) => format!("{} · {page_short}", quoted(&d.name)),
    };
    Glance { lines, short }
}

/// `"Title" (L12, text "Summer sale", at 120, 80, 900×140)`.
fn describe(d: &Document, l: &Layer) -> String {
    let what = match &l.content {
        Content::Raster { .. } => "pixels".to_string(),
        Content::Fill { color } => format!("colour fill {}", color.hex()),
        Content::Adjustment { adjustment } => format!("{} adjustment", adjustment.label()),
        Content::Text { text } => format!("text {}", quoted(&text.text)),
        Content::Vector { shape } => format!("vector {}", shape_kind(&shape.geometry)),
        Content::Group { children, .. } => format!("group of {} layer{}", children.len(), plural(children.len())),
    };
    let mut flags = vec![];
    if !l.visible {
        flags.push("hidden");
    }
    if l.locked {
        flags.push("locked");
    }
    let bounds = nori_control::commands::util::bounds(d, l).filter(|r| !r.is_empty()).map(|r| format!(", {}", rect(r))).unwrap_or_default();
    let flags = if flags.is_empty() { String::new() } else { format!(", {}", flags.join(", ")) };
    format!("{} ({}, {what}{bounds}{flags})", quoted(&l.name), l.id)
}

fn shape_kind(g: &nori_core::Geometry) -> &'static str {
    use nori_core::Geometry::*;
    match g {
        Rect { .. } => "rectangle",
        Ellipse { .. } => "ellipse",
        Polygon { inner: Some(_), .. } => "star",
        Polygon { .. } => "polygon",
        Line { .. } => "line",
        Path { .. } => "path",
    }
}

/// `at 120, 80, 900×140`.
fn rect(r: Rect) -> String {
    format!("at {}, {}, {}×{}", r.x, r.y, r.w, r.h)
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// A name on one line, quoted, at most 60 characters.
fn quoted(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = one.chars().take(60).collect();
    format!("\"{short}{}\"", if short.len() < one.len() { "…" } else { "" })
}

fn trim_num(x: f64) -> String {
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
