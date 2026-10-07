//! InDesign Markup (`.idml`), the open format InDesign and Affinity Publisher export: spreads and
//! their pages, text frames with their stories (threads kept, paragraph and character settings as
//! runs), rectangles, ovals, polygons and lines with their fills and strokes, and placed pictures
//! that are found on disk. Points become pixels at 300 dpi.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use nori_core::vector::{FillRule, Geometry, Node, Paint, Shape, Stroke, SubPath};
use nori_core::{Color, Content, Document, Layer, Page, Raster, TextAlign, TextLayer, TextRun};
use quick_xml::events::{BytesStart, Event};

/// Pixels per point.
const K: f32 = 300.0 / 72.0;

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == name.as_bytes()).and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

fn nums(s: &str) -> Vec<f32> {
    s.split_whitespace().filter_map(|v| v.parse().ok()).collect()
}

type M = [f32; 6];
const ID: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn mat(s: Option<String>) -> M {
    let v = s.map(|s| nums(&s)).unwrap_or_default();
    if v.len() == 6 { [v[0], v[1], v[2], v[3], v[4], v[5]] } else { ID }
}

/// `a` after `b`.
fn mul(a: &M, b: &M) -> M {
    [a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1], a[0] * b[2] + a[2] * b[3], a[1] * b[2] + a[3] * b[3], a[0] * b[4] + a[2] * b[5] + a[4], a[1] * b[4] + a[3] * b[5] + a[5]]
}

fn apply(m: &M, p: [f32; 2]) -> [f32; 2] {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

fn read_zip(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Swatches: `Color/…` id → colour.
fn colors(xml: &str) -> HashMap<String, Color> {
    let mut out = HashMap::new();
    out.insert("Color/Black".to_string(), Color::BLACK);
    out.insert("Color/Paper".to_string(), Color::WHITE);
    out.insert("Swatch/None".to_string(), Color::TRANSPARENT);
    let mut r = quick_xml::Reader::from_str(xml);
    while let Ok(ev) = r.read_event() {
        match ev {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"Color" => {
                let (Some(id), Some(vals)) = (attr(&e, "Self"), attr(&e, "ColorValue")) else { continue };
                let v = nums(&vals);
                let space = attr(&e, "Space").unwrap_or_default();
                let c = match (space.as_str(), v.as_slice()) {
                    ("CMYK", [c, m, y, k]) => Color::rgb((1.0 - c / 100.0) * (1.0 - k / 100.0), (1.0 - m / 100.0) * (1.0 - k / 100.0), (1.0 - y / 100.0) * (1.0 - k / 100.0)),
                    ("RGB", [r, g, b]) => Color::rgb(r / 255.0, g / 255.0, b / 255.0),
                    ("LAB", [l, _, _]) => Color::rgb(l / 100.0, l / 100.0, l / 100.0),
                    _ => continue,
                };
                out.insert(id, c);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

/// A story: its words and how parts of them are set.
#[derive(Default, Clone)]
struct Story {
    text: String,
    runs: Vec<TextRun>,
    size: Option<f32>,
    font: Option<String>,
    color: Option<Color>,
    align: Option<TextAlign>,
    weight: Option<u16>,
}

fn weight_of(style: &str) -> (u16, bool) {
    let s = style.to_lowercase();
    let w = if s.contains("black") || s.contains("heavy") {
        900
    } else if s.contains("extrabold") || s.contains("extra bold") {
        800
    } else if s.contains("semibold") || s.contains("demi") {
        600
    } else if s.contains("bold") {
        700
    } else if s.contains("medium") {
        500
    } else if s.contains("light") {
        300
    } else {
        400
    };
    (w, s.contains("italic") || s.contains("oblique"))
}

fn story(xml: &str, colors: &HashMap<String, Color>) -> (String, Story) {
    let mut r = quick_xml::Reader::from_str(xml);
    let mut st = Story::default();
    let mut id = String::new();
    // The character range being read: its settings.
    let mut cur: Option<TextRun> = None;
    let mut in_content = false;
    let mut in_font = false;
    let mut first = true;
    while let Ok(ev) = r.read_event() {
        match ev {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name();
                match name.as_ref() {
                    b"Story" => id = attr(&e, "Self").unwrap_or_default(),
                    b"ParagraphStyleRange" => {
                        if st.align.is_none() {
                            st.align = attr(&e, "Justification").map(|j| match j.as_str() {
                                "CenterAlign" | "CenterJustified" => TextAlign::Center,
                                "RightAlign" | "RightJustified" => TextAlign::Right,
                                _ => TextAlign::Left,
                            });
                        }
                    }
                    b"CharacterStyleRange" => {
                        let mut run = TextRun { start: st.text.len(), end: st.text.len(), ..Default::default() };
                        run.size = attr(&e, "PointSize").and_then(|v| v.parse::<f32>().ok()).map(|v| v * K);
                        run.color = attr(&e, "FillColor").and_then(|c| colors.get(&c).copied());
                        if let Some(fs) = attr(&e, "FontStyle") {
                            let (w, it) = weight_of(&fs);
                            run.weight = Some(w);
                            run.italic = Some(it);
                        }
                        cur = Some(run);
                    }
                    b"AppliedFont" => in_font = true,
                    b"Content" => in_content = true,
                    b"Br" => st.text.push('\n'),
                    _ => {}
                }
            }
            Event::Text(t) => {
                let text = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                if in_content {
                    st.text.push_str(&text.replace('\u{2028}', "\n"));
                } else if in_font
                    && let Some(run) = &mut cur
                {
                    run.font = Some(text.trim().to_string());
                }
            }
            Event::End(e) => match e.name().as_ref() {
                b"Content" => in_content = false,
                b"AppliedFont" => in_font = false,
                b"CharacterStyleRange" => {
                    if let Some(mut run) = cur.take() {
                        run.end = st.text.len();
                        if first {
                            // The first range sets the whole story; later ones are runs.
                            st.size = run.size;
                            st.font = run.font.clone();
                            st.color = run.color;
                            st.weight = run.weight;
                            first = false;
                        } else if run.end > run.start && (run.size != st.size || run.color != st.color || run.weight != st.weight || run.font != st.font) {
                            st.runs.push(run);
                        }
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    // Trailing paragraph breaks don't draw anything.
    while st.text.ends_with('\n') {
        st.text.pop();
    }
    let n = st.text.len();
    st.runs.retain(|r| r.start < n);
    st.runs.iter_mut().for_each(|r| r.end = r.end.min(n));
    (id, st)
}

/// An item's outline, in spread coordinates.
fn path_of(e_xml: &[(Vec<f32>, Option<Vec<f32>>, Option<Vec<f32>>)], m: &M, closed: bool) -> SubPath {
    let nodes = e_xml
        .iter()
        .map(|(a, l, r)| {
            let p = apply(m, [a[0], a[1]]);
            let h = |v: &Option<Vec<f32>>| v.as_ref().filter(|v| v.len() == 2).map(|v| apply(m, [v[0], v[1]])).filter(|q| (q[0] - p[0]).abs() > 1e-3 || (q[1] - p[1]).abs() > 1e-3);
            Node { x: p[0], y: p[1], handle_in: h(l), handle_out: h(r) }
        })
        .collect();
    SubPath { nodes, closed }
}

/// What a spread holds, before it is sorted onto pages.
enum Item {
    Frame { id: String, story: String, next: Option<String>, bounds: [f32; 4] },
    Shape { name: String, subpaths: Vec<SubPath>, fill: Option<Color>, stroke: Option<(Color, f32)>, oval: bool },
    Picture { path: String, bounds: [f32; 4] },
}

impl Item {
    fn center(&self) -> [f32; 2] {
        match self {
            Item::Frame { bounds: b, .. } | Item::Picture { bounds: b, .. } => [b[0] + b[2] / 2.0, b[1] + b[3] / 2.0],
            Item::Shape { subpaths, .. } => {
                let pts: Vec<&Node> = subpaths.iter().flat_map(|s| s.nodes.iter()).collect();
                let n = pts.len().max(1) as f32;
                [pts.iter().map(|p| p.x).sum::<f32>() / n, pts.iter().map(|p| p.y).sum::<f32>() / n]
            }
        }
    }
}

fn bounds(sub: &SubPath) -> [f32; 4] {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for n in &sub.nodes {
        x0 = x0.min(n.x);
        y0 = y0.min(n.y);
        x1 = x1.max(n.x);
        y1 = y1.max(n.y);
    }
    if x0 > x1 { [0.0; 4] } else { [x0, y0, x1 - x0, y1 - y0] }
}

struct PageBox {
    name: String,
    /// Top-left in spread coordinates, size in points.
    origin: [f32; 2],
    size: [f32; 2],
}

fn spread(xml: &str, colors: &HashMap<String, Color>) -> (Vec<PageBox>, Vec<Item>) {
    let mut r = quick_xml::Reader::from_str(xml);
    let mut pages = vec![];
    let mut items = vec![];
    // Open items: their kind, transform (to spread), attributes and points.
    struct Open {
        tag: String,
        m: M,
        attrs: HashMap<String, String>,
        points: Vec<(Vec<f32>, Option<Vec<f32>>, Option<Vec<f32>>)>,
        open_path: bool,
        subpaths: Vec<SubPath>,
        link: Option<String>,
    }
    let mut stack: Vec<Open> = vec![];
    let mut group: Vec<M> = vec![ID];
    let tags: [&[u8]; 6] = [b"TextFrame", b"Rectangle", b"Oval", b"Polygon", b"GraphicLine", b"Group"];
    while let Ok(ev) = r.read_event() {
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                let name = e.name().as_ref().to_vec();
                if name == b"Page" {
                    let m = mul(group.last().unwrap_or(&ID), &mat(attr(e, "ItemTransform")));
                    let b = nums(&attr(e, "GeometricBounds").unwrap_or_default());
                    if b.len() == 4 {
                        let o = apply(&m, [b[1], b[0]]);
                        pages.push(PageBox { name: attr(e, "Name").unwrap_or_default(), origin: o, size: [b[3] - b[1], b[2] - b[0]] });
                    }
                } else if name == b"Spread" {
                    group = vec![mat(attr(e, "ItemTransform"))];
                } else if tags.contains(&name.as_slice()) {
                    let m = mul(group.last().unwrap_or(&ID), &mat(attr(e, "ItemTransform")));
                    if name == b"Group" {
                        if !empty {
                            group.push(m);
                        }
                        continue;
                    }
                    let attrs = e.attributes().flatten().map(|a| (String::from_utf8_lossy(a.key.as_ref()).into_owned(), a.unescape_value().map(|v| v.into_owned()).unwrap_or_default())).collect();
                    stack.push(Open { tag: String::from_utf8_lossy(&name).into_owned(), m, attrs, points: vec![], open_path: false, subpaths: vec![], link: None });
                    if empty {
                        // No geometry: nothing to draw.
                        stack.pop();
                    }
                } else if name == b"GeometryPathType" {
                    if let Some(o) = stack.last_mut() {
                        o.open_path = attr(e, "PathOpen").as_deref() == Some("true");
                        o.points.clear();
                    }
                } else if name == b"PathPointType" {
                    if let Some(o) = stack.last_mut() {
                        let a = nums(&attr(e, "Anchor").unwrap_or_default());
                        if a.len() == 2 {
                            o.points.push((a, attr(e, "LeftDirection").map(|s| nums(&s)), attr(e, "RightDirection").map(|s| nums(&s))));
                        }
                    }
                } else if name == b"Link" {
                    if let Some(o) = stack.last_mut() {
                        o.link = attr(e, "LinkResourceURI");
                    }
                }
            }
            Event::End(ref e) => {
                let name = e.name().as_ref().to_vec();
                if name == b"Group" {
                    if group.len() > 1 {
                        group.pop();
                    }
                } else if name == b"GeometryPathType" {
                    if let Some(o) = stack.last_mut() {
                        let sub = path_of(&o.points, &o.m, !o.open_path);
                        if sub.nodes.len() >= 2 {
                            o.subpaths.push(sub);
                        }
                    }
                } else if tags.contains(&name.as_slice()) {
                    let Some(o) = stack.pop() else { continue };
                    let Some(first) = o.subpaths.first() else { continue };
                    let b = bounds(first);
                    match o.tag.as_str() {
                        "TextFrame" => items.push(Item::Frame {
                            id: o.attrs.get("Self").cloned().unwrap_or_default(),
                            story: o.attrs.get("ParentStory").cloned().unwrap_or_default(),
                            next: o.attrs.get("NextTextFrame").cloned().filter(|n| n != "n"),
                            bounds: b,
                        }),
                        _ => {
                            if let Some(link) = &o.link {
                                items.push(Item::Picture { path: link.clone(), bounds: b });
                                continue;
                            }
                            let fill = o.attrs.get("FillColor").and_then(|c| colors.get(c).copied()).filter(|c| c.a > 0.0);
                            let w = o.attrs.get("StrokeWeight").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
                            let stroke = o.attrs.get("StrokeColor").and_then(|c| colors.get(c).copied()).filter(|c| c.a > 0.0).map(|c| (c, w));
                            if fill.is_none() && stroke.is_none() {
                                continue;
                            }
                            items.push(Item::Shape { name: o.attrs.get("Name").cloned().filter(|n| !n.is_empty() && n != "$ID/").unwrap_or_else(|| o.tag.clone()), subpaths: o.subpaths, fill, stroke, oval: o.tag == "Oval" });
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    (pages, items)
}

/// Reads an IDML package.
pub fn read(bytes: &[u8], name: &str) -> Result<Document, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not an IDML package: {e}"))?;
    let design = read_zip(&mut zip, "designmap.xml").ok_or("no designmap.xml in it")?;
    let mut spreads = vec![];
    let mut stories = vec![];
    let mut r = quick_xml::Reader::from_str(&design);
    while let Ok(ev) = r.read_event() {
        match ev {
            Event::Start(e) | Event::Empty(e) => match e.name().as_ref() {
                b"idPkg:Spread" => spreads.extend(attr(&e, "src")),
                b"idPkg:Story" => stories.extend(attr(&e, "src")),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    let colors = read_zip(&mut zip, "Resources/Graphic.xml").map(|x| colors(&x)).unwrap_or_default();
    let stories: HashMap<String, Story> = stories.iter().filter_map(|s| read_zip(&mut zip, s)).map(|x| story(&x, &colors)).collect();

    let mut doc = Document::empty(name, 1, 1);
    doc.dpi = 300.0;
    doc.pages.clear();
    let mut frame_ids: HashMap<String, String> = HashMap::new();
    let mut nexts: Vec<(String, String)> = vec![];
    for src in &spreads {
        let Some(xml) = read_zip(&mut zip, src) else { continue };
        let (pages, items) = spread(&xml, &colors);
        let first_new = doc.pages.len();
        for p in &pages {
            let id = doc.new_page_id();
            let n = doc.pages.len() + 1;
            let mut page = Page::new(id, if p.name.is_empty() { format!("Page {n}") } else { format!("Page {}", p.name) }, ((p.size[0] * K).round() as u32).max(1), ((p.size[1] * K).round() as u32).max(1));
            page.x = p.origin[0] * K;
            page.y = p.origin[1] * K;
            let paper = doc.new_id();
            page.layers.push(Layer::new(paper, "Paper", Content::Fill { color: Color::WHITE }));
            doc.pages.push(page);
        }
        if pages.is_empty() {
            continue;
        }
        for item in items {
            let c = item.center();
            // The page the item's middle is on (else the nearest).
            let pi = pages
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    let d = |p: &PageBox| {
                        let inside = c[0] >= p.origin[0] && c[0] <= p.origin[0] + p.size[0] && c[1] >= p.origin[1] && c[1] <= p.origin[1] + p.size[1];
                        if inside { 0.0 } else { (c[0] - p.origin[0] - p.size[0] / 2.0).abs() + (c[1] - p.origin[1] - p.size[1] / 2.0).abs() }
                    };
                    d(a).total_cmp(&d(b))
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            let o = pages[pi].origin;
            let local = |x: f32, y: f32| ((x - o[0]) * K, (y - o[1]) * K);
            let layer = match item {
                Item::Frame { id, story, next, bounds: b } => {
                    let st = stories.get(&story).cloned().unwrap_or_default();
                    let (x, y) = local(b[0], b[1]);
                    let lid = doc.new_id();
                    frame_ids.insert(id.clone(), lid.clone());
                    if let Some(n) = next {
                        nexts.push((id, n));
                    }
                    let (font, weight) = (st.font.clone().map(|f| nori_render::text::resolve_family(&f)).unwrap_or_else(|| "Manrope".into()), st.weight.unwrap_or(400));
                    let t = TextLayer {
                        text: st.text.clone(),
                        font,
                        size: st.size.unwrap_or(12.0 * K),
                        weight,
                        color: st.color.unwrap_or(Color::BLACK),
                        x,
                        y,
                        align: st.align.unwrap_or_default(),
                        frame: Some([b[2] * K, b[3] * K]),
                        runs: st.runs.iter().cloned().map(|mut r| {
                            r.font = r.font.map(|f| nori_render::text::resolve_family(&f));
                            r
                        }).collect(),
                        ..Default::default()
                    };
                    let nm: String = st.text.lines().next().unwrap_or("Text").chars().take(24).collect();
                    Layer::new(lid, if nm.trim().is_empty() { "Text frame".to_string() } else { nm }, Content::Text { text: t })
                }
                Item::Shape { name, subpaths, fill, stroke, oval: _ } => {
                    let subpaths = subpaths
                        .into_iter()
                        .map(|s| SubPath {
                            closed: s.closed,
                            nodes: s
                                .nodes
                                .into_iter()
                                .map(|n| {
                                    let (x, y) = local(n.x, n.y);
                                    let h = |h: Option<[f32; 2]>| h.map(|h| { let (a, b) = local(h[0], h[1]); [a, b] });
                                    Node { x, y, handle_in: h(n.handle_in), handle_out: h(n.handle_out) }
                                })
                                .collect(),
                        })
                        .collect();
                    let mut shape = Shape::new(Geometry::Path { subpaths }, fill.map(Paint::solid).unwrap_or(Paint::None), stroke.map(|(c, w)| Stroke { paint: Paint::solid(c), width: w * K, ..Default::default() }));
                    shape.fill_rule = FillRule::NonZero;
                    let lid = doc.new_id();
                    Layer::new(lid, name, Content::Vector { shape })
                }
                Item::Picture { path, bounds: b } => {
                    let file = path.strip_prefix("file:").unwrap_or(&path).replace("%20", " ");
                    let (x, y) = local(b[0], b[1]);
                    let (w, h) = (((b[2] * K).round() as u32).max(1), ((b[3] * K).round() as u32).max(1));
                    let lid = doc.new_id();
                    let picture = std::fs::read(&file).ok().and_then(|bytes| crate::decode_picture(&bytes).ok());
                    match picture {
                        Some((pw, ph, rgba)) => {
                            let px = nori_render::transform::resample_rgba(&rgba, pw, ph, w, h, nori_render::transform::Filter::Bicubic);
                            Layer::new(lid, std::path::Path::new(&file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Picture".into()), Content::Raster { x: x.round() as i32, y: y.round() as i32, pixels: Raster::from_rgba(w, h, &px) })
                        }
                        None => {
                            // The picture isn't here: a grey placeholder box where it was.
                            let g = Geometry::Rect { x, y, w: b[2] * K, h: b[3] * K, radius: 0.0 };
                            Layer::new(lid, format!("Missing: {}", std::path::Path::new(&file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), Content::Vector { shape: Shape::new(g, Paint::solid(Color::rgb(0.85, 0.85, 0.85)), Some(Stroke { paint: Paint::solid(Color::rgb(0.6, 0.6, 0.6)), width: K, ..Default::default() })) })
                        }
                    }
                }
            };
            // Items come in drawing order: each new one goes on top.
            doc.pages[first_new + pi].layers.insert(0, layer);
        }
    }
    if doc.pages.is_empty() {
        return Err("the package has no pages".into());
    }
    // Threads: a frame's next one, and the words only in the first.
    for (from, to) in nexts {
        if let (Some(a), Some(b)) = (frame_ids.get(&from).cloned(), frame_ids.get(&to).cloned()) {
            if let Some(Layer { content: Content::Text { text }, .. }) = doc.layer_mut(&a) {
                text.next = Some(b.clone());
            }
            if let Some(Layer { content: Content::Text { text }, .. }) = doc.layer_mut(&b) {
                text.text.clear();
                text.runs.clear();
            }
        }
    }
    doc.active_page = doc.pages[0].id.clone();
    doc.active = doc.pages[0].layers.first().map(|l| l.id.clone());
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut out);
        for (name, text) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(text.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn reads_pages_frames_threads_and_shapes() {
        let path = |x0: f32, y0: f32, x1: f32, y1: f32| {
            format!(
                "<Properties><PathGeometry><GeometryPathType PathOpen=\"false\"><PathPointArray><PathPointType Anchor=\"{x0} {y0}\" LeftDirection=\"{x0} {y0}\" RightDirection=\"{x0} {y0}\"/><PathPointType Anchor=\"{x0} {y1}\"/><PathPointType Anchor=\"{x1} {y1}\"/><PathPointType Anchor=\"{x1} {y0}\"/></PathPointArray></GeometryPathType></PathGeometry></Properties>"
            )
        };
        let spread = format!(
            "<idPkg:Spread><Spread Self=\"s1\" ItemTransform=\"1 0 0 1 0 0\">\
             <Page Self=\"p1\" Name=\"1\" GeometricBounds=\"0 0 200 100\" ItemTransform=\"1 0 0 1 -100 -100\"/>\
             <Page Self=\"p2\" Name=\"2\" GeometricBounds=\"0 0 200 100\" ItemTransform=\"1 0 0 1 0 -100\"/>\
             <TextFrame Self=\"t1\" ParentStory=\"u5\" NextTextFrame=\"t2\" ItemTransform=\"1 0 0 1 0 0\">{}</TextFrame>\
             <TextFrame Self=\"t2\" ParentStory=\"u5\" PreviousTextFrame=\"t1\" NextTextFrame=\"n\" ItemTransform=\"1 0 0 1 0 0\">{}</TextFrame>\
             <Rectangle Self=\"r1\" FillColor=\"Color/Red\" StrokeColor=\"Swatch/None\" ItemTransform=\"1 0 0 1 0 0\">{}</Rectangle>\
             </Spread></idPkg:Spread>",
            path(-90.0, -90.0, -10.0, 0.0),
            path(10.0, -90.0, 90.0, 0.0),
            path(10.0, 20.0, 50.0, 60.0)
        );
        let story = "<idPkg:Story><Story Self=\"u5\"><ParagraphStyleRange Justification=\"CenterAlign\"><CharacterStyleRange PointSize=\"10\" FontStyle=\"Bold\"><Content>Hello </Content></CharacterStyleRange><CharacterStyleRange PointSize=\"10\" FillColor=\"Color/Red\"><Content>world</Content><Br/></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>";
        let graphic = "<idPkg:Graphic><Color Self=\"Color/Red\" Space=\"RGB\" ColorValue=\"255 0 0\" Name=\"Red\"/></idPkg:Graphic>";
        let design = "<Document><idPkg:Graphic src=\"Resources/Graphic.xml\"/><idPkg:Spread src=\"Spreads/Spread_s1.xml\"/><idPkg:Story src=\"Stories/Story_u5.xml\"/></Document>";
        let bytes = package(&[("designmap.xml", design), ("Spreads/Spread_s1.xml", &spread), ("Stories/Story_u5.xml", story), ("Resources/Graphic.xml", graphic)]);
        let d = read(&bytes, "t").unwrap();
        assert_eq!(d.pages.len(), 2);
        assert_eq!((d.pages[0].width, d.pages[0].height), ((100.0 * K).round() as u32, (200.0 * K).round() as u32));
        let frames: Vec<&Layer> = d.all().into_iter().filter(|l| matches!(l.content, Content::Text { .. })).collect();
        assert_eq!(frames.len(), 2);
        let Content::Text { text: a } = &d.pages[0].layers[0].content else { panic!("page 1's top is the frame") };
        assert_eq!(a.text, "Hello world");
        assert_eq!(a.weight, 700);
        assert_eq!(a.align, TextAlign::Center);
        assert_eq!(a.runs.len(), 1);
        assert_eq!(a.runs[0].color, Some(Color::rgb(1.0, 0.0, 0.0)));
        assert!(a.next.is_some());
        assert!((a.x - 10.0 * K).abs() < 0.01 && (a.y - 10.0 * K).abs() < 0.01);
        // Page 2 has the second frame and the red box.
        assert!(d.pages[1].layers.iter().any(|l| matches!(&l.content, Content::Vector { shape } if matches!(shape.fill, Paint::Solid { color } if color == Color::rgb(1.0, 0.0, 0.0)))));
        assert_eq!(d.thread_of(&frames[0].id).len(), 2);
    }
}
