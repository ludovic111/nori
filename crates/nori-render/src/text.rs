//! Text, set with cosmic-text (shaping, fallback, system fonts plus the bundled Manrope and IBM
//! Plex Mono) and filled with tiny-skia, so the canvas, the export and every client draw it the
//! same.
//!
//! Point text is one block from its top-left corner. A text frame wraps its words at the frame's
//! width and stops at its height; what doesn't fit flows into the next frame of its thread (on
//! this page or another), as in a layout program. Runs set parts of the words differently
//! (a character style, a weight, a colour).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use cosmic_text::{Attrs, Buffer, CacheKey, CacheKeyFlags, Command, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap, fontdb};
use nori_core::{Color, Content, Document, Rect, TextAlign, TextLayer};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

/// The families nori always has (bundled, OFL).
pub const BUNDLED_FAMILIES: [&str; 2] = ["Manrope", "IBM Plex Mono"];

pub static BUNDLED_FONTS: &[&[u8]] = &[
    include_bytes!("../fonts/manrope/Manrope-Regular.ttf"),
    include_bytes!("../fonts/manrope/Manrope-Medium.ttf"),
    include_bytes!("../fonts/manrope/Manrope-SemiBold.ttf"),
    include_bytes!("../fonts/manrope/Manrope-Bold.ttf"),
    include_bytes!("../fonts/manrope/Manrope-ExtraBold.ttf"),
    include_bytes!("../fonts/ibmplexmono/IBMPlexMono-Regular.ttf"),
    include_bytes!("../fonts/ibmplexmono/IBMPlexMono-Italic.ttf"),
    include_bytes!("../fonts/ibmplexmono/IBMPlexMono-Medium.ttf"),
    include_bytes!("../fonts/ibmplexmono/IBMPlexMono-MediumItalic.ttf"),
    include_bytes!("../fonts/ibmplexmono/IBMPlexMono-SemiBold.ttf"),
    include_bytes!("../fonts/ibmplexmono/IBMPlexMono-SemiBoldItalic.ttf"),
    include_bytes!("../fonts/ibmplexmono/IBMPlexMono-Bold.ttf"),
    include_bytes!("../fonts/ibmplexmono/IBMPlexMono-BoldItalic.ttf"),
];

/// A text layer drawn: straight RGBA rows covering `rect` on its page.
#[derive(Debug, Default)]
pub struct TextImage {
    pub rect: Rect,
    pub rgba: Vec<u8>,
    /// The words didn't all fit (the last frame of a thread overflows).
    pub overflow: bool,
}

impl TextImage {
    /// The pixel at page (x, y), straight RGBA 0..255.
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> [u8; 4] {
        if !self.rect.contains(x, y) || self.rgba.is_empty() {
            return [0; 4];
        }
        let o = (((y - self.rect.y) as u32 * self.rect.w + (x - self.rect.x) as u32) * 4) as usize;
        [self.rgba[o], self.rgba[o + 1], self.rgba[o + 2], self.rgba[o + 3]]
    }
}

struct Fonts {
    system: FontSystem,
    swash: SwashCache,
    families: HashMap<String, String>,
}

fn fonts() -> MutexGuard<'static, Fonts> {
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            for f in BUNDLED_FONTS {
                db.load_font_source(fontdb::Source::Binary(Arc::new(*f)));
            }
            if std::env::var_os("NORI_NO_SYSTEM_FONTS").is_none() {
                db.load_system_fonts();
            }
            db.set_sans_serif_family("Manrope");
            db.set_monospace_family("IBM Plex Mono");
            let families = db.faces().flat_map(|f| f.families.iter().map(|(n, _)| (n.to_lowercase(), n.clone()))).collect();
            Mutex::new(Fonts { system: FontSystem::new_with_locale_and_db("en-US".into(), db), swash: SwashCache::new(), families })
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Family names for a font picker: the bundled ones first, then the system's.
pub fn families() -> Vec<String> {
    let f = fonts();
    let mut sys: Vec<String> = f.families.values().filter(|n| !n.starts_with('.') && !BUNDLED_FAMILIES.contains(&n.as_str())).cloned().collect();
    sys.sort_by_key(|n| n.to_lowercase());
    sys.dedup();
    BUNDLED_FAMILIES.iter().map(|s| s.to_string()).chain(sys).collect()
}

fn family_in(f: &Fonts, name: &str) -> String {
    let lower = name.trim().to_lowercase();
    if let Some(n) = f.families.get(&lower) {
        return n.clone();
    }
    if ["mono", "courier", "menlo", "consol", "code"].iter().any(|k| lower.contains(k)) { "IBM Plex Mono".into() } else { "Manrope".into() }
}

/// The family a name resolves to (itself if installed, else a bundled one of the same kind).
pub fn resolve_family(name: &str) -> String {
    family_in(&fonts(), name)
}

fn outline(commands: &[Command], x: f32, y: f32, pb: &mut PathBuilder) {
    for c in commands {
        match *c {
            Command::MoveTo(v) => pb.move_to(x + v.x, y - v.y),
            Command::LineTo(v) => pb.line_to(x + v.x, y - v.y),
            Command::QuadTo(c1, v) => pb.quad_to(x + c1.x, y - c1.y, x + v.x, y - v.y),
            Command::CurveTo(c1, c2, v) => pb.cubic_to(x + c1.x, y - c1.y, x + c2.x, y - c2.y, x + v.x, y - v.y),
            Command::Close => pb.close(),
        }
    }
}

/// One span of a story, resolved: where it is and how it is set.
#[derive(Clone, Debug, PartialEq)]
struct Span {
    start: usize,
    end: usize,
    family: String,
    size: f32,
    weight: u16,
    italic: bool,
    color: Color,
}

/// The words with `{page}` / `{pages}` filled in, and the spans covering every byte.
fn story(doc: Option<&Document>, t: &TextLayer, page_no: usize) -> (String, Vec<Span>) {
    let pages = doc.map_or(1, |d| d.pages.len());
    // Runs refer to byte ranges of the text as written; tokens change lengths, so runs are
    // mapped through the replacement.
    let mut text = String::with_capacity(t.text.len());
    let mut map: Vec<usize> = Vec::with_capacity(t.text.len() + 1);
    let mut i = 0;
    let src = t.text.as_str();
    while i < src.len() {
        let rest = &src[i..];
        let (token, value) = if rest.starts_with("{page}") {
            (6, page_no.to_string())
        } else if rest.starts_with("{pages}") {
            (7, pages.to_string())
        } else {
            (0, String::new())
        };
        if token > 0 {
            for k in 0..token {
                map.push(text.len() + if k == 0 { 0 } else { value.len() });
            }
            text.push_str(&value);
            i += token;
        } else {
            let ch = rest.chars().next().expect("not empty");
            for _ in 0..ch.len_utf8() {
                map.push(text.len());
            }
            text.push(ch);
            i += ch.len_utf8();
        }
    }
    map.push(text.len());
    let base = Span { start: 0, end: text.len(), family: t.font.clone(), size: t.size, weight: t.weight, italic: t.italic, color: t.color };
    // Cut the text at every run edge; each piece takes the runs covering it.
    let mut cuts: Vec<usize> = vec![0, text.len()];
    let runs: Vec<(usize, usize, &nori_core::TextRun)> = t
        .runs
        .iter()
        .map(|r| (map[r.start.min(src.len())], map[r.end.min(src.len())], r))
        .filter(|(s, e, _)| e > s)
        .collect();
    for (s, e, _) in &runs {
        cuts.push(*s);
        cuts.push(*e);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut spans = vec![];
    for w in cuts.windows(2) {
        let (s, e) = (w[0], w[1]);
        if e <= s || !text.is_char_boundary(s) || !text.is_char_boundary(e) {
            continue;
        }
        let mut sp = Span { start: s, end: e, ..base.clone() };
        for (rs, re, r) in &runs {
            if *rs <= s && e <= *re {
                if let Some(cs) = r.style.as_deref().and_then(|n| doc.and_then(|d| d.character_style(n))) {
                    if let Some(v) = &cs.font {
                        sp.family = v.clone();
                    }
                    if let Some(v) = cs.size {
                        sp.size = v;
                    }
                    if let Some(v) = cs.weight {
                        sp.weight = v;
                    }
                    if let Some(v) = cs.italic {
                        sp.italic = v;
                    }
                    if let Some(v) = cs.color {
                        sp.color = v;
                    }
                }
                if let Some(v) = &r.font {
                    sp.family = v.clone();
                }
                if let Some(v) = r.size {
                    sp.size = v;
                }
                if let Some(v) = r.weight {
                    sp.weight = v;
                }
                if let Some(v) = r.italic {
                    sp.italic = v;
                }
                if let Some(v) = r.color {
                    sp.color = v;
                }
            }
        }
        spans.push(sp);
    }
    if spans.is_empty() {
        spans.push(base);
    }
    (text, spans)
}

/// A shaped glyph with the exact font and position used on the canvas, for searchable PDF.
#[derive(Clone)]
pub struct PdfGlyph {
    pub font: Arc<Vec<u8>>,
    pub font_index: u32,
    pub id: u16,
    pub text: String,
    pub size: f32,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    pub color: Color,
}

/// One frame's worth of glyphs: outline paths by colour, and where its words ended.
struct Set {
    glyphs: Vec<PdfGlyph>,
    paths: Vec<(Color, tiny_skia::Path)>,
    block: Rect,
    /// Byte offset in the story where the next frame continues (the story's length if all fit).
    end: usize,
}

/// Sets `text[from..]` (spans in story bytes) in a frame of `t`'s settings.
fn set_frame(f: &mut Fonts, t: &TextLayer, text: &str, spans: &[Span], from: usize) -> Set {
    set_frame_impl(f, t, text, spans, from, false)
}

fn set_frame_impl(f: &mut Fonts, t: &TextLayer, text: &str, spans: &[Span], from: usize, collect: bool) -> Set {
    let line_mult = t.line_height.clamp(0.5, 5.0);
    let base_size = t.size.clamp(1.0, 4000.0);
    let part = &text[from..];
    let frame = t.frame.filter(|[w, h]| *w > 1.0 && *h > 1.0);
    let mut buffer = Buffer::new_empty(Metrics::new(base_size, base_size * line_mult));
    buffer.set_wrap(if frame.is_some() { Wrap::WordOrGlyph } else { Wrap::None });
    buffer.set_size(frame.map(|[w, _]| w), None);
    let families: Vec<String> = spans.iter().map(|s| family_in(f, &s.family)).collect();
    let pieces: Vec<(&str, Attrs)> = spans
        .iter()
        .zip(families.iter())
        .filter(|(s, _)| s.end > from)
        .map(|(s, fam)| {
            let (a, b) = (s.start.max(from) - from, s.end - from);
            let [r, g, bl, al] = s.color.to_u8();
            let size = s.size.clamp(1.0, 4000.0);
            let attrs = Attrs::new()
                .family(Family::Name(fam))
                .weight(Weight(s.weight.clamp(100, 900)))
                .style(if s.italic { Style::Italic } else { Style::Normal })
                .color(cosmic_text::Color::rgba(r, g, bl, al))
                .metrics(Metrics::new(size, size * line_mult))
                .letter_spacing(t.letter_spacing / size)
                .cache_key_flags(CacheKeyFlags::DISABLE_HINTING);
            (&part[a..b], attrs)
        })
        .collect();
    let default = Attrs::new().family(Family::Name(families.first().map_or("Manrope", String::as_str))).metrics(Metrics::new(base_size, base_size * line_mult));
    buffer.set_rich_text(pieces, &default, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut f.system, false);

    // Paragraph starts in `part` (cosmic's line_i is the paragraph, glyph offsets are in it).
    let mut para_start = vec![0usize];
    for (i, c) in part.char_indices() {
        if c == '\n' {
            para_start.push(i + 1);
        }
    }
    struct Line {
        top: f32,
        height: f32,
        baseline: f32,
        width: f32,
        para: usize,
        glyphs: Vec<cosmic_text::LayoutGlyph>,
    }
    let mut lines: Vec<Line> = vec![];
    for run in buffer.layout_runs() {
        let spacing = run.line_i as f32 * t.paragraph_spacing;
        lines.push(Line { top: run.line_top + spacing, height: run.line_height, baseline: run.line_y + spacing, width: run.line_w, para: run.line_i, glyphs: run.glyphs.to_vec() });
    }
    // In a frame, only whole lines that fit.
    let mut end = text.len();
    if let Some([_, h]) = frame
        && let Some(cut) = lines.iter().position(|l| l.top + l.height > h + 0.5)
    {
        let first = &lines[cut];
        let at = first.glyphs.first().map_or(0, |g| g.start);
        end = from + para_start.get(first.para).copied().unwrap_or(0) + at;
        lines.truncate(cut);
    }
    let block_w = frame.map(|[w, _]| w).unwrap_or_else(|| lines.iter().map(|l| l.width).fold(1.0, f32::max));
    let block_h = frame.map(|[_, h]| h).unwrap_or_else(|| lines.last().map_or(base_size * line_mult, |l| l.top + l.height));
    let mut glyphs = Vec::new();
    let mut faces = HashMap::new();
    let mut by_color: Vec<(Color, PathBuilder)> = vec![];
    for line in &lines {
        let dx = match t.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (block_w - line.width) / 2.0,
            TextAlign::Right => block_w - line.width,
        };
        for g in &line.glyphs {
            let (key, _, _) = CacheKey::new(g.font_id, g.glyph_id, g.font_size, (0.0, 0.0), g.font_weight, g.cache_key_flags);
            let color = g.color_opt.map(|c| Color::from_u8([c.r(), c.g(), c.b(), c.a()])).unwrap_or(t.color);
            if collect {
                let face = faces.entry(g.font_id).or_insert_with(|| f.system.db().with_face_data(g.font_id, |data, index| (Arc::new(data.to_vec()), index)));
                if let Some((font, font_index)) = face {
                    let start = from + para_start[line.para] + g.start;
                    let end = from + para_start[line.para] + g.end;
                    glyphs.push(PdfGlyph { font: font.clone(), font_index: *font_index, id: g.glyph_id, text: text.get(start..end).unwrap_or("").to_string(), size: g.font_size,
                        x: t.x + dx + g.x + g.font_size * g.x_offset, y: t.y + line.baseline - g.font_size * g.y_offset, advance: g.w, color });
                }
            }
            let Fonts { system, swash, .. } = &mut *f;
            if let Some(cmds) = swash.get_outline_commands(system, key) {
                let i = match by_color.iter().position(|(c, _)| *c == color) {
                    Some(i) => i,
                    None => {
                        by_color.push((color, PathBuilder::new()));
                        by_color.len() - 1
                    }
                };
                outline(cmds, t.x + dx + g.x + g.font_size * g.x_offset, t.y + line.baseline - g.font_size * g.y_offset, &mut by_color[i].1);
            }
        }
    }
    let paths = by_color.into_iter().filter_map(|(c, pb)| pb.finish().map(|p| (c, p))).collect();
    let block = Rect::new(t.x.floor() as i32, t.y.floor() as i32, block_w.ceil() as u32 + 1, block_h.ceil().max(1.0) as u32 + 1);
    Set { glyphs, paths, block, end }
}

/// The box a text layer takes on its page (point text: its words; a frame: the frame).
pub fn measure(t: &TextLayer) -> Rect {
    let (text, spans) = story(None, t, 1);
    set_frame(&mut fonts(), t, &text, &spans, 0).block
}

fn paint_set(set: &Set, t: &TextLayer, overflow: bool) -> TextImage {
    let mut bounds: Option<tiny_skia::Rect> = None;
    for (_, p) in &set.paths {
        let b = p.bounds();
        bounds = Some(match bounds {
            Some(a) => tiny_skia::Rect::from_ltrb(a.left().min(b.left()), a.top().min(b.top()), a.right().max(b.right()), a.bottom().max(b.bottom())).unwrap_or(a),
            None => b,
        });
    }
    let Some(b) = bounds else { return TextImage { rect: set.block, rgba: vec![], overflow } };
    let rect = Rect::new(b.left().floor() as i32 - 1, b.top().floor() as i32 - 1, (b.width().ceil() as u32) + 3, (b.height().ceil() as u32) + 3);
    let rect = Rect::new(rect.x, rect.y, rect.w.min(20_000), rect.h.min(20_000));
    let Some(mut pm) = Pixmap::new(rect.w.max(1), rect.h.max(1)) else { return TextImage::default() };
    for (color, path) in &set.paths {
        let mut paint = Paint::default();
        let [r, g, bl, a] = color.to_u8();
        paint.set_color_rgba8(r, g, bl, a);
        paint.anti_alias = true;
        pm.fill_path(path, &paint, FillRule::Winding, Transform::from_translate(-rect.x as f32, -rect.y as f32), None);
    }
    let _ = t;
    // tiny-skia is premultiplied; layers are straight.
    let mut rgba = pm.take();
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    TextImage { rect, rgba, overflow }
}

/// The page number (1-based) `{page}` stands for in a layer: the page it is on, or for a layer
/// on a master page, the page `on` it is drawn on (0 when none is given).
fn page_number(doc: &Document, id: &str, on: Option<usize>) -> usize {
    match doc.path_of(id).map(|l| l.page) {
        Some(nori_core::PageRef::Page(i)) => i + 1,
        Some(nori_core::PageRef::Master(_)) => on.unwrap_or(0),
        None => 0,
    }
}

/// Whether a text layer's words change with the page it is drawn on.
pub fn has_page_tokens(t: &TextLayer) -> bool {
    t.text.contains("{page}")
}

/// Draws every frame of the thread `id` belongs to (point text: just itself). Returns each
/// frame's picture by layer id.
pub fn render_thread(doc: &Document, id: &str) -> Vec<(String, Arc<TextImage>)> {
    render_thread_on(doc, id, None)
}

/// [`render_thread`] for a thread on a master page, drawn on page number `on`.
pub fn render_thread_on(doc: &Document, id: &str, on: Option<usize>) -> Vec<(String, Arc<TextImage>)> {
    let ids = doc.thread_of(id);
    let frames: Vec<(String, TextLayer)> = ids
        .iter()
        .filter_map(|i| match &doc.layer(i)?.content {
            Content::Text { text } => Some((i.clone(), text.clone())),
            _ => None,
        })
        .collect();
    let Some((head_id, head)) = frames.first() else { return vec![] };
    // Remembered by everything that goes into it.
    let pages: Vec<usize> = frames.iter().map(|(i, _)| page_number(doc, i, on)).collect();
    let styles = serde_json::to_string(&doc.styles.character).unwrap_or_default();
    let key = format!("{}|{:?}|{}|{}", serde_json::to_string(&frames).unwrap_or_default(), pages, doc.pages.len(), styles);
    type StoryImages = HashMap<String, Vec<(String, Arc<TextImage>)>>;
    static CACHE: OnceLock<Mutex<StoryImages>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return v.clone();
    }
    let (text, spans) = story(Some(doc), head, page_number(doc, head_id, on));
    let mut out = vec![];
    let mut from = 0;
    let mut f = fonts();
    let n = frames.len();
    for (k, (fid, frame)) in frames.iter().enumerate() {
        // Each frame is set with its own geometry and the head's type settings.
        let mut t = head.clone();
        t.x = frame.x;
        t.y = frame.y;
        t.frame = frame.frame;
        // Leading spaces and line breaks don't start a frame.
        while from < text.len() && text[from..].starts_with([' ', '\n']) {
            from += 1;
        }
        let set = if from < text.len() { set_frame(&mut f, &t, &text, &spans, from) } else { Set { glyphs: vec![], paths: vec![], block: Rect::default(), end: from } };
        let overflow = k + 1 == n && set.end < text.len();
        out.push((fid.clone(), Arc::new(paint_set(&set, &t, overflow))));
        from = set.end.max(from);
    }
    drop(f);
    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > 256 {
        map.clear();
    }
    map.insert(key, out.clone());
    out
}

/// One text layer on its own (no document: no threads, no character styles, page 1).
pub fn render(t: &TextLayer) -> Arc<TextImage> {
    let (text, spans) = story(None, t, 1);
    let set = set_frame(&mut fonts(), t, &text, &spans, 0);
    Arc::new(paint_set(&set, t, set.end < text.len()))
}

/// Glyph outlines of a text layer as it is drawn on its page (for SVG and PDF, which keep text
/// as vectors): `(colour, path)` per colour.
pub fn outlines(doc: &Document, id: &str) -> Vec<(Color, tiny_skia::Path)> {
    outlines_on(doc, id, None)
}

/// [`outlines`] for a layer that may be on a master page, drawn on page number `on`.
pub fn outlines_on(doc: &Document, id: &str, on: Option<usize>) -> Vec<(Color, tiny_skia::Path)> {
    let ids = doc.thread_of(id);
    let frames: Vec<(String, TextLayer)> = ids
        .iter()
        .filter_map(|i| match &doc.layer(i)?.content {
            Content::Text { text } => Some((i.clone(), text.clone())),
            _ => None,
        })
        .collect();
    let Some((head_id, head)) = frames.first() else { return vec![] };
    let (text, spans) = story(Some(doc), head, page_number(doc, head_id, on));
    let mut from = 0;
    let mut f = fonts();
    for (fid, frame) in &frames {
        let mut t = head.clone();
        t.x = frame.x;
        t.y = frame.y;
        t.frame = frame.frame;
        while from < text.len() && text[from..].starts_with([' ', '\n']) {
            from += 1;
        }
        let set = if from < text.len() { set_frame(&mut f, &t, &text, &spans, from) } else { Set { glyphs: vec![], paths: vec![], block: Rect::default(), end: from } };
        if fid == id {
            return set.paths;
        }
        from = set.end.max(from);
    }
    vec![]
}

/// Searchable glyphs for one frame, with thread flow and page tokens resolved.
pub fn pdf_glyphs_on(doc: &Document, id: &str, on: Option<usize>) -> Vec<PdfGlyph> {
    let ids = doc.thread_of(id);
    let frames: Vec<(String, TextLayer)> = ids
        .iter()
        .filter_map(|i| match &doc.layer(i)?.content {
            Content::Text { text } => Some((i.clone(), text.clone())),
            _ => None,
        })
        .collect();
    let Some((head_id, head)) = frames.first() else { return vec![] };
    let (text, spans) = story(Some(doc), head, page_number(doc, head_id, on));
    let mut from = 0;
    let mut f = fonts();
    for (fid, frame) in &frames {
        let mut t = head.clone();
        t.x = frame.x;
        t.y = frame.y;
        t.frame = frame.frame;
        while from < text.len() && text[from..].starts_with([' ', '\n']) {
            from += 1;
        }
        let set = if from < text.len() { set_frame_impl(&mut f, &t, &text, &spans, from, true) } else { Set { glyphs: vec![], paths: vec![], block: Rect::default(), end: from } };
        if fid == id {
            return set.glyphs;
        }
        from = set.end.max(from);
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_core::{Layer, Page, TextRun};

    #[test]
    fn draws_ink_where_the_words_are() {
        let t = TextLayer { text: "Hello".into(), size: 48.0, color: Color::rgb(1.0, 0.0, 0.0), x: 10.0, y: 20.0, ..Default::default() };
        let img = render(&t);
        assert!(img.rect.w > 60 && img.rect.h > 20, "{:?}", img.rect);
        let inked = img.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 128).count();
        assert!(inked > 200, "only {inked} pixels inked");
        assert!(img.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).all(|p| p[0] >= 250 && p[1] <= 5));
        let m = measure(&t);
        assert_eq!((m.x, m.y), (10, 20));
    }

    #[test]
    fn runs_change_colour_and_tokens_fill_in() {
        let t = TextLayer {
            text: "ab {page}/{pages}".into(),
            size: 40.0,
            color: Color::BLACK,
            runs: vec![TextRun { start: 0, end: 2, color: Some(Color::rgb(1.0, 0.0, 0.0)), ..Default::default() }],
            ..Default::default()
        };
        let (text, spans) = story(None, &t, 3);
        assert_eq!(text, "ab 3/1");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].color, Color::rgb(1.0, 0.0, 0.0));
        let img = render(&t);
        assert!(img.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 200 && p[0] > 200 && p[1] < 50));
        assert!(img.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 200 && p[0] < 50));
    }

    #[test]
    fn words_flow_from_frame_to_frame() {
        let mut d = Document::empty("t", 400, 400);
        let words = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen".to_string();
        let a = d.new_id();
        let b = d.new_id();
        let frame = |text: &str, x: f32, next: Option<String>| Content::Text {
            text: TextLayer { text: text.into(), size: 20.0, x, y: 10.0, frame: Some([120.0, 60.0]), next, ..Default::default() },
        };
        d.pages[0].layers.push(Layer::new(a.clone(), "A", frame(&words, 10.0, Some(b.clone()))));
        let p2 = d.new_page_id();
        d.pages.push(Page::new(p2, "2", 400, 400));
        d.pages[1].layers.push(Layer::new(b.clone(), "B", frame("", 200.0, None)));
        let out = render_thread(&d, &b);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, a);
        assert!(!out[0].1.rgba.is_empty() && !out[1].1.rgba.is_empty(), "both frames get words");
        assert!(out[1].1.overflow, "sixteen words don't fit in two small frames");
        assert!(out[1].1.rect.x >= 199);
        // The first frame stops at its height.
        assert!(out[0].1.rect.bottom() <= 10 + 60 + 4, "{:?}", out[0].1.rect);
    }
}
