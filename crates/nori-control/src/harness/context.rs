//! The live context (HARNESS.md part 3): what an agent is told before every model step.
//!
//! "Make this bigger", "put a title here", "blur the selection": the words point at the window.
//! A compact summary of the document goes to the agent with each request and again before each
//! later step when something changed: the page and its grid, the layers on it, the active layer,
//! the selection, the tool, the problems the quick checks find, and what the person (or another
//! client) changed since the agent's last step. The full state stays one command away
//! (`doc.overview`).

use nori_core::{Content, Document, Layer, Rect};
use serde::Serialize;

use super::checks;
use crate::session::{Change, Session, Source, UiState};

/// Layers listed by name; the rest are counted.
const LISTED_LAYERS: usize = 14;

/// What an agent sees of nori at one moment.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Glance {
    /// The `<context>` block's lines.
    pub lines: Vec<String>,
    /// One short line for the Agent panel ("\"Title\" (text) · page 2 of 3").
    pub short: String,
    /// The latest command's sequence number when this was taken: pass it back as `since`.
    pub seq: u64,
    /// Changes by other clients after `since`.
    pub changes: Vec<Change>,
}

impl Glance {
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// The context block, then the request.
    pub fn frame(&self, prompt: &str) -> String {
        if self.lines.is_empty() {
            return prompt.to_string();
        }
        format!("<context>\n{}\n</context>\n\n{prompt}", self.text())
    }

    /// The block alone, as it is sent before a later step.
    pub fn block(&self) -> String {
        format!("<context>\n{}\n</context>", self.text())
    }
}

/// `text` without the context block [`Glance::frame`] put in front of it.
pub fn unframed(text: &str) -> &str {
    match text.strip_prefix("<context>\n").and_then(|rest| rest.split_once("\n</context>\n\n")) {
        Some((_, prompt)) => prompt,
        None => text,
    }
}

/// What nori shows now. `since`: the `seq` of the previous glance, to report what others changed
/// after it (`mine`: the sources whose changes the caller made itself and knows).
pub fn glance(session: &Session, since: Option<u64>, mine: &[Source]) -> Glance {
    let seq = session.last_seq();
    let changes: Vec<Change> = since.map(|s| session.changes_since(s).into_iter().filter(|c| !mine.contains(&c.source)).collect()).unwrap_or_default();
    let ui = session.ui_state();
    let saved = session.paths();
    let doc = session.document();
    let mut g = match doc {
        Ok(d) => of(&d, saved.as_ref().is_some_and(|(_, _, dirty)| *dirty), saved.as_ref().and_then(|(p, s, _)| p.clone().or_else(|| s.clone())), &ui),
        Err(_) => Glance { lines: vec!["No document is open in nori (doc_new or doc_open first).".into()], short: "No document open".into(), ..Default::default() },
    };
    if !changes.is_empty() {
        g.lines.push(format!("Since your last step, others changed the document: {}.", describe_changes(&changes)));
    }
    g.seq = seq;
    g.changes = changes;
    g
}

fn describe_changes(changes: &[Change]) -> String {
    let mut parts: Vec<String> = changes
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|c| {
            let who = match c.source {
                Source::Window => "the person",
                Source::Agent => "the built-in agent",
                Source::Cli => "a script",
                Source::Mcp => "an MCP client",
            };
            let on = if c.layers.is_empty() { String::new() } else { format!(" on {}", c.layers.join(", ")) };
            format!("{who} ran {}{on}", c.command)
        })
        .collect();
    if changes.len() > 8 {
        parts.insert(0, format!("{} earlier changes, then", changes.len() - 8));
    }
    parts.join("; ")
}

fn of(d: &Document, dirty: bool, file: Option<std::path::PathBuf>, ui: &UiState) -> Glance {
    let mut lines = vec!["What nori shows now (\"this\", \"here\" and \"the selected…\" mean these; doc_overview has the rest):".to_string()];
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
    let print = if d.dpi >= 150.0 { format!(" = {}×{} mm", mm(page.width, d.dpi), mm(page.height, d.dpi)) } else { String::new() };
    lines.push(format!(
        "Document {}: {where_} ({}, id {}, {}×{} px at {} dpi{print}), {layers} layer{} on it; {state}.",
        quoted(&d.name),
        quoted(&page.name),
        page.id,
        page.width,
        page.height,
        trim_num(d.dpi as f64),
        plural(layers)
    ));
    let m = &page.margins;
    let mut grid = vec![];
    if m.top > 0.0 || m.right > 0.0 || m.bottom > 0.0 || m.left > 0.0 {
        grid.push(format!("margins {}/{}/{}/{} px (top/right/bottom/left)", trim_num(m.top as f64), trim_num(m.right as f64), trim_num(m.bottom as f64), trim_num(m.left as f64)));
    }
    if page.columns > 1 {
        grid.push(format!("{} columns, gutter {} px", page.columns, trim_num(page.gutter as f64)));
    }
    if page.bleed > 0.0 {
        grid.push(format!("bleed {} px", trim_num(page.bleed as f64)));
    }
    if let Some(master) = page.master.as_ref().and_then(|id| d.masters.iter().find(|p| &p.id == id)) {
        grid.push(format!("master {}", quoted(&master.name)));
    }
    if !grid.is_empty() {
        lines.push(format!("Grid: {}.", grid.join(", ")));
    }
    if !page.layers.is_empty() {
        let listed: Vec<String> = page.layers.iter().take(LISTED_LAYERS).map(|l| describe(d, l)).collect();
        let more = page.layers.len().saturating_sub(LISTED_LAYERS);
        let more = if more > 0 { format!("; and {more} more (layer_list)") } else { String::new() };
        lines.push(format!("Layers, top first: {}{more}.", listed.join("; ")));
    }

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
    if let Ok(report) = checks::check(d, d.active_ref(), checks::Depth::Quick) {
        let found: Vec<&checks::Problem> = report.problems.iter().filter(|p| p.severity != checks::Severity::Info).collect();
        if !found.is_empty() {
            let listed: Vec<String> = found.iter().take(6).map(|p| p.message.clone()).collect();
            let more = if found.len() > 6 { format!(" (+{} more: harness_check)", found.len() - 6) } else { String::new() };
            lines.push(format!("Problems on this page: {}{more}", listed.join(" ")));
        }
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
    Glance { lines, short, ..Default::default() }
}

/// `"Title" (L12, text "Summer sale" 120 px Manrope 800 #ffffff, at 120, 80, 900×140)`.
fn describe(d: &Document, l: &Layer) -> String {
    let what = match &l.content {
        Content::Raster { pixels, .. } => format!("pixels {}×{}", pixels.width(), pixels.height()),
        Content::Fill { color } => format!("colour fill {}", color.hex()),
        Content::Adjustment { adjustment } => format!("{} adjustment", adjustment.label()),
        Content::Text { text } => format!("text {} {} px {} {} {}", quoted(&text.text), trim_num(text.size as f64), text.font, text.weight, text.color.hex()),
        Content::Vector { shape } => format!("vector {}", shape_kind(&shape.geometry)),
        Content::Group { children, .. } => format!("group of {} layer{}", children.len(), plural(children.len())),
    };
    let mut flags = vec![];
    if !l.visible {
        flags.push("hidden".to_string());
    }
    if l.locked {
        flags.push("locked".into());
    }
    if l.opacity < 0.999 {
        flags.push(format!("opacity {}", trim_num(l.opacity as f64)));
    }
    if l.blend != nori_core::BlendMode::Normal && l.blend != nori_core::BlendMode::PassThrough {
        flags.push(l.blend.id().to_string());
    }
    if l.mask.is_some() {
        flags.push("masked".into());
    }
    if l.clipped {
        flags.push("clipped".into());
    }
    let bounds = match &l.content {
        Content::Fill { .. } | Content::Adjustment { .. } => String::new(),
        _ => crate::commands::util::bounds(d, l).filter(|r| !r.is_empty()).map(|r| format!(", {}", rect(r))).unwrap_or_default(),
    };
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

/// Pixels at `dpi` in millimetres, to one decimal when it isn't whole.
pub(crate) fn mm(px: u32, dpi: f32) -> String {
    trim_num((px as f64 / dpi.max(1.0) as f64 * 25.4 * 10.0).round() / 10.0)
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// A name on one line, quoted, at most 40 characters.
fn quoted(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = one.chars().take(40).collect();
    format!("\"{short}{}\"", if short.len() < one.len() { "…" } else { "" })
}

pub(crate) fn trim_num(x: f64) -> String {
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
