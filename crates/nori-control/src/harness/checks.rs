//! Objective checks an agent runs on its work (HARNESS.md part 4): numbers, not impressions.
//!
//! * **Text contrast**: every visible text layer drawn alone and the page drawn without it; the
//!   WCAG contrast ratio between the text's colour and what is behind each of its glyphs'
//!   pixels (median, and the worst tenth), against 4.5:1, or 3:1 for large text.
//! * **Bounds**: layers entirely off the page (and its bleed), text running past the page edge
//!   or outside the margins, artwork that crosses the trim without reaching the bleed.
//! * **Resolution**: each picture's pixels per inch at its printed size, and how much real detail
//!   it holds (an enlarged picture has fewer details than pixels).
//! * **Overflow and leftovers**: text that doesn't fit its frame, empty layers.
//! * **Print colour**: bright RGB colours a CMYK press can't reach, on print documents.

use nori_core::{Content, Document, Layer, Page, PageRef, Rect};
use serde::Serialize;

use super::context::{mm, trim_num};
use crate::commands::util;

/// How far the checks go: `Quick` (geometry and overflow, for the live context) or `Full`
/// (also renders for contrast and resolution).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    Quick,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub severity: Severity,
    /// `overflow`, `offPage`, `pastEdge`, `outsideMargins`, `shortOfBleed`, `noBleed`, `contrast`,
    /// `lowResolution`, `enlarged`, `empty`, `printColour`, `hidden`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contrast {
    pub layer_id: String,
    pub name: String,
    pub text: String,
    /// Type size in pixels and in points at the document's dpi.
    pub size_px: f32,
    pub size_pt: f32,
    /// Large text (18 pt, or 14 pt bold) needs 3:1 instead of 4.5:1.
    pub large: bool,
    /// Median contrast ratio over the glyphs' pixels.
    pub ratio: f32,
    /// The worst tenth of the glyphs' pixels (busy backgrounds).
    pub worst: f32,
    pub required: f32,
    pub ok: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    pub layer_id: String,
    pub name: String,
    /// The picture's pixels on the page.
    pub pixels: [u32; 2],
    /// Its printed size at the document's dpi.
    pub print_mm: [f32; 2],
    /// Pixels per inch at that size (the document's dpi).
    pub ppi: f32,
    /// How much it was enlarged, judged from its detail (1: sharp to the pixel).
    pub enlarged: f32,
    /// Real detail per inch: `ppi / enlarged`.
    pub effective_ppi: f32,
    pub ok: bool,
}

/// The checks of one page.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub page: String,
    /// From 1 (0 for a master page).
    pub number: usize,
    pub width: u32,
    pub height: u32,
    pub dpi: f32,
    /// The trim size in millimetres, and whether the document is for print (200 dpi or more).
    pub print_mm: [f32; 2],
    pub print: bool,
    pub ok: bool,
    pub errors: usize,
    pub warnings: usize,
    pub problems: Vec<Problem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contrast: Vec<Contrast>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Resolution>,
}

/// Print documents: 200 dpi or more.
const PRINT_DPI: f32 = 200.0;
/// The largest area a contrast check renders, in pixels (bigger text is sampled at its top).
const CONTRAST_AREA: u32 = 4_000_000;

/// Checks one page (or master page) of `d`.
pub fn check(d: &Document, r: PageRef, depth: Depth) -> Result<Report, String> {
    let page = d.page_at(r).ok_or("no page")?;
    let number = match r {
        PageRef::Page(i) => i + 1,
        PageRef::Master(_) => 0,
    };
    let print = d.dpi >= PRINT_DPI;
    let mut problems = vec![];
    // Drawing every shape and text is for the full checks; the quick ones only lay out text
    // (cached by its content), so the live context stays cheap.
    let prep = (depth == Depth::Full).then(|| nori_render::prepare(d));
    let overflows = |id: &str| match &prep {
        Some(p) => p.text(id).is_some_and(|t| t.overflow),
        None => nori_render::text::render_thread(d, id).into_iter().any(|(i, t)| i == id && t.overflow),
    };

    // Every visible layer, with its group's visibility.
    let mut shown: Vec<&Layer> = vec![];
    fn visible<'a>(list: &'a [Layer], out: &mut Vec<&'a Layer>) {
        for l in list {
            if !l.visible {
                continue;
            }
            out.push(l);
            visible(l.children(), out);
        }
    }
    visible(&page.layers, &mut shown);

    let trim = page.bounds();
    let bleed = page.bleed.max(0.0).round() as i32;
    let with_bleed = trim.inflate(bleed);
    let m = &page.margins;
    let live = Rect::new(m.left.round() as i32, m.top.round() as i32, (page.width as f32 - m.left - m.right).max(0.0) as u32, (page.height as f32 - m.top - m.bottom).max(0.0) as u32);
    let has_margins = m.top > 0.0 || m.right > 0.0 || m.bottom > 0.0 || m.left > 0.0;
    let mut runs_off_edge = false;

    for l in &shown {
        let label = format!("`{}` ({})", l.name, l.id);
        if let Content::Text { text } = &l.content {
            if overflows(&l.id) {
                problems.push(problem(Severity::Error, "overflow", l, format!("{label} doesn't fit its frame: make the frame bigger, the type smaller, or thread it on (text_thread).")));
            }
            if text.text.trim().is_empty() && text.next.is_none() && !is_threaded_into(d, &l.id) {
                problems.push(problem(Severity::Warning, "empty", l, format!("{label} is an empty text layer.")));
            }
        }
        if let Content::Raster { pixels, .. } = &l.content
            && pixels.used_tiles() == 0
        {
            problems.push(problem(Severity::Warning, "empty", l, format!("{label} has no pixels.")));
            continue;
        }
        if matches!(l.content, Content::Fill { .. } | Content::Adjustment { .. } | Content::Group { .. }) {
            continue;
        }
        let Some(b) = util::bounds(d, l).filter(|b| !b.is_empty()) else { continue };
        if b.intersect(&with_bleed).is_empty() {
            problems.push(problem(Severity::Error, "offPage", l, format!("{label} is entirely outside the page{} (at {}, {}, {}×{}).", if bleed > 0 { " and its bleed" } else { "" }, b.x, b.y, b.w, b.h)));
            continue;
        }
        let past = sides_past(b, trim);
        if matches!(l.content, Content::Text { .. }) {
            if !past.is_empty() {
                problems.push(problem(Severity::Error, "pastEdge", l, format!("{label} runs past the page edge ({}): it will be cut off.", past.join(", "))));
            } else if has_margins && !inside(b, live, 2) {
                problems.push(problem(Severity::Warning, "outsideMargins", l, format!("{label} goes outside the margins ({}).", sides_past(b, live).join(", "))));
            }
        } else if b.x <= trim.x || b.y <= trim.y || b.right() >= trim.right() || b.bottom() >= trim.bottom() {
            // Artwork that reaches the trim must run on into the bleed.
            runs_off_edge = true;
            if bleed > 0 {
                let short = sides_short_of_bleed(b, trim, bleed);
                if !short.is_empty() {
                    problems.push(problem(Severity::Warning, "shortOfBleed", l, format!("{label} reaches the trim but stops short of the {bleed} px bleed ({}): extend it to the bleed edge.", short.join(", "))));
                }
            }
        }
    }
    if print && runs_off_edge && bleed == 0 && number > 0 {
        problems.push(Problem {
            severity: Severity::Warning,
            kind: "noBleed",
            layer_id: None,
            message: format!("Artwork runs off the edge of this print page but it has no bleed: page_update bleed={} (3 mm) and extend it to the bleed edge.", (3.0 / 25.4 * d.dpi).round()),
        });
    }

    let mut contrast = vec![];
    let mut images = vec![];
    if let Some(prep) = &prep {
        for l in &shown {
            match &l.content {
                Content::Text { text } if !text.text.trim().is_empty() || is_threaded_into(d, &l.id) => {
                    if let Some(c) = text_contrast(d, page, prep, l, text) {
                        if !c.ok {
                            problems.push(problem(
                                Severity::Error,
                                "contrast",
                                l,
                                format!("`{}` ({}) has a contrast of {:.1}:1 against what is behind it; {} text needs {}:1. Darken or lighten it, or put a scrim behind it.", l.name, l.id, c.ratio, if c.large { "large" } else { "body" }, trim_num(c.required as f64)),
                            ));
                        } else if c.worst < c.required {
                            problems.push(problem(Severity::Warning, "contrast", l, format!("Part of `{}` ({}) falls on a busy area: its worst tenth is {:.1}:1 (needs {}:1).", l.name, l.id, c.worst, trim_num(c.required as f64))));
                        }
                        contrast.push(c);
                    }
                }
                Content::Raster { pixels, .. } => {
                    if let Some(res) = resolution(d, page, l, pixels, print) {
                        if !res.ok {
                            let (sev, kind) = if print && res.effective_ppi < 150.0 { (Severity::Error, "lowResolution") } else { (Severity::Warning, if print { "lowResolution" } else { "enlarged" }) };
                            let msg = if print {
                                format!("`{}` ({}) holds about {} ppi of detail at its printed size ({}×{} mm); print wants 300 (at least 200 for posters). Use a bigger original or print it smaller.", l.name, l.id, res.effective_ppi.round(), res.print_mm[0], res.print_mm[1])
                            } else {
                                format!("`{}` ({}) looks enlarged about {}×: it will look soft. Use a bigger original or show it smaller.", l.name, l.id, trim_num(res.enlarged as f64))
                            };
                            problems.push(problem(sev, kind, l, msg));
                        }
                        images.push(res);
                    }
                }
                _ => {}
            }
        }
        if print {
            for (l, c) in print_colours(&shown) {
                problems.push(problem(Severity::Info, "printColour", l, format!("`{}` ({}) uses {}, a bright RGB colour that prints duller in CMYK: pick a slightly muted one if colour matters.", l.name, l.id, c)));
            }
        }
    }
    for l in page.all() {
        if !l.visible {
            problems.push(problem(Severity::Info, "hidden", l, format!("`{}` ({}) is hidden.", l.name, l.id)));
        }
    }
    let errors = problems.iter().filter(|p| p.severity == Severity::Error).count();
    let warnings = problems.iter().filter(|p| p.severity == Severity::Warning).count();
    let to_mm = |px: u32| (px as f32 / d.dpi.max(1.0) * 25.4 * 10.0).round() / 10.0;
    Ok(Report {
        page: page.id.clone(),
        number,
        width: page.width,
        height: page.height,
        dpi: d.dpi,
        print_mm: [to_mm(page.width), to_mm(page.height)],
        print,
        ok: errors == 0,
        errors,
        warnings,
        problems,
        contrast,
        images,
    })
}

fn problem(severity: Severity, kind: &'static str, l: &Layer, message: String) -> Problem {
    Problem { severity, kind, layer_id: Some(l.id.clone()), message }
}

/// Whether another frame threads its words into `id` (a frame with no words of its own).
fn is_threaded_into(d: &Document, id: &str) -> bool {
    d.pages.iter().chain(&d.masters).flat_map(|p| p.all()).any(|l| matches!(&l.content, Content::Text { text } if text.next.as_deref() == Some(id)))
}

fn inside(b: Rect, area: Rect, tolerance: i32) -> bool {
    b.x >= area.x - tolerance && b.y >= area.y - tolerance && b.right() <= area.right() + tolerance && b.bottom() <= area.bottom() + tolerance
}

/// "left by 40 px", … for each side of `b` past `area`.
fn sides_past(b: Rect, area: Rect) -> Vec<String> {
    let mut out = vec![];
    if b.x < area.x {
        out.push(format!("left by {} px", area.x - b.x));
    }
    if b.y < area.y {
        out.push(format!("top by {} px", area.y - b.y));
    }
    if b.right() > area.right() {
        out.push(format!("right by {} px", b.right() - area.right()));
    }
    if b.bottom() > area.bottom() {
        out.push(format!("bottom by {} px", b.bottom() - area.bottom()));
    }
    out
}

/// The sides where `b` reaches the trim but ends before the bleed's edge.
fn sides_short_of_bleed(b: Rect, trim: Rect, bleed: i32) -> Vec<String> {
    let mut out = vec![];
    let tol = 1;
    if b.x <= trim.x && b.x > trim.x - bleed + tol {
        out.push(format!("left reaches {} of {bleed} px", trim.x - b.x));
    }
    if b.y <= trim.y && b.y > trim.y - bleed + tol {
        out.push(format!("top reaches {} of {bleed} px", trim.y - b.y));
    }
    if b.right() >= trim.right() && b.right() < trim.right() + bleed - tol {
        out.push(format!("right reaches {} of {bleed} px", b.right() - trim.right()));
    }
    if b.bottom() >= trim.bottom() && b.bottom() < trim.bottom() + bleed - tol {
        out.push(format!("bottom reaches {} of {bleed} px", b.bottom() - trim.bottom()));
    }
    out
}

// ---- contrast ---------------------------------------------------------------------------------

/// WCAG relative luminance of an sRGB colour (0–255 channels).
pub fn luminance(rgb: [f32; 3]) -> f32 {
    let lin = |c: f32| {
        let c = c / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2])
}

/// WCAG contrast ratio of two colours (1–21).
pub fn contrast_ratio(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// `layers` without the layer `id` (wherever it is in groups).
fn without(layers: &[Layer], id: &str) -> Vec<Layer> {
    layers
        .iter()
        .filter(|l| l.id != id)
        .map(|l| {
            let mut l = l.clone();
            if let Some(c) = l.children_mut() {
                *c = without(c, id);
            }
            l
        })
        .collect()
}

fn text_contrast(d: &Document, page: &Page, prep: &nori_render::Prepared, l: &Layer, text: &nori_core::TextLayer) -> Option<Contrast> {
    let mut area = util::bounds(d, l)?.intersect(&page.bounds());
    if area.is_empty() {
        return None;
    }
    if area.w * area.h > CONTRAST_AREA {
        area.h = (CONTRAST_AREA / area.w).max(1);
    }
    // The text alone, at full opacity of its own, and the page without it.
    let mut alone_layer = l.clone();
    alone_layer.visible = true;
    alone_layer.clipped = false;
    alone_layer.mask = None;
    let opacity = l.opacity.clamp(0.0, 1.0);
    alone_layer.opacity = 1.0;
    let mut alone = page.clone();
    alone.master = None;
    alone.layers = vec![alone_layer];
    let mut behind = page.clone();
    behind.layers = without(&page.layers, &l.id);
    let fg = nori_render::composite::flatten_page_with(d, &alone, prep, area);
    let bg = nori_render::composite::flatten_page_with(d, &behind, prep, area);
    let max_a = fg.chunks_exact(4).map(|p| p[3]).max().unwrap_or(0);
    if max_a < 32 {
        return None;
    }
    let core = (max_a as u32 * 3 / 4) as u8;
    let mut ratios: Vec<f32> = fg
        .chunks_exact(4)
        .zip(bg.chunks_exact(4))
        .filter(|(f, _)| f[3] >= core)
        .map(|(f, b)| {
            // What is behind, over white paper where the page is transparent.
            let ba = b[3] as f32 / 255.0;
            let back = [0, 1, 2].map(|c| b[c] as f32 * ba + 255.0 * (1.0 - ba));
            // The text as it lands with its layer's opacity.
            let front = [0, 1, 2].map(|c| f[c] as f32 * opacity + back[c] * (1.0 - opacity));
            contrast_ratio(front, back)
        })
        .collect();
    if ratios.is_empty() {
        return None;
    }
    ratios.sort_by(f32::total_cmp);
    let ratio = ratios[ratios.len() / 2];
    let worst = ratios[ratios.len() / 10];
    let size_pt = text.size * 72.0 / d.dpi.max(1.0);
    let bold = text.weight >= 700;
    // Screen documents: CSS pixels (24 px, or 18.66 px bold). Print: points (18, or 14 bold).
    let large = if d.dpi < PRINT_DPI { text.size >= 24.0 || (bold && text.size >= 18.66) } else { size_pt >= 18.0 || (bold && size_pt >= 14.0) };
    let required = if large { 3.0 } else { 4.5 };
    let round = |x: f32| (x * 100.0).round() / 100.0;
    Some(Contrast {
        layer_id: l.id.clone(),
        name: l.name.clone(),
        text: text.text.chars().take(40).collect(),
        size_px: text.size,
        size_pt: round(size_pt),
        large,
        ratio: round(ratio),
        worst: round(worst),
        required,
        ok: ratio >= required,
    })
}

// ---- resolution -------------------------------------------------------------------------------

/// The side of the window the detail estimate looks at.
const DETAIL_WINDOW: u32 = 256;
/// Below this ratio of round-trip error to local contrast, a down-and-up resample lost nothing:
/// the picture holds no detail at that scale (it was enlarged at least that much).
const DETAIL_THRESHOLD: f32 = 0.36;

fn resolution(d: &Document, page: &Page, l: &Layer, pixels: &nori_core::Raster, print: bool) -> Option<Resolution> {
    let (x, y, _) = l.raster()?;
    let content = pixels.content_bounds()?;
    let on_page = content.translate(x, y).intersect(&page.bounds());
    if on_page.w < 16 || on_page.h < 16 {
        return None;
    }
    let enlarged = enlargement(pixels, content).unwrap_or(1.0);
    let ppi = d.dpi;
    let effective = ppi / enlarged;
    let to_mm = |px: u32| (px as f32 / d.dpi.max(1.0) * 25.4 * 10.0).round() / 10.0;
    let ok = if print { effective >= 250.0 || (effective >= 180.0 && on_page.w.max(on_page.h) as f32 / d.dpi >= 16.0) } else { enlarged < 1.6 };
    Some(Resolution {
        layer_id: l.id.clone(),
        name: l.name.clone(),
        pixels: [on_page.w, on_page.h],
        print_mm: [to_mm(on_page.w), to_mm(on_page.h)],
        ppi,
        enlarged,
        effective_ppi: (effective * 10.0).round() / 10.0,
        ok,
    })
}

fn luma(rgba: &[u8]) -> Vec<f32> {
    rgba.chunks_exact(4)
        .map(|p| {
            let a = p[3] as f32 / 255.0;
            // Over mid grey, so transparent edges don't read as detail.
            let c = |v: u8| v as f32 * a + 128.0 * (1.0 - a);
            0.299 * c(p[0]) + 0.587 * c(p[1]) + 0.114 * c(p[2])
        })
        .collect()
}

/// Mean absolute difference between neighbours: how much is going on.
fn activity(l: &[f32], w: usize, h: usize) -> f32 {
    let mut sum = 0.0;
    let mut n = 0usize;
    for y in 0..h {
        for x in 0..w {
            let v = l[y * w + x];
            if x + 1 < w {
                sum += (v - l[y * w + x + 1]).abs();
                n += 1;
            }
            if y + 1 < h {
                sum += (v - l[(y + 1) * w + x]).abs();
                n += 1;
            }
        }
    }
    if n == 0 { 0.0 } else { sum / n as f32 }
}

/// How much a picture was enlarged, from its detail: the largest factor `k` for which shrinking
/// by `k` and enlarging back loses (almost) nothing. Looks at the busiest window of the picture.
/// `None` when the picture is too flat to tell.
pub fn enlargement(pixels: &nori_core::Raster, content: Rect) -> Option<f32> {
    let side = DETAIL_WINDOW.min(content.w).min(content.h);
    if side < 32 {
        return None;
    }
    // The busiest of a 3×3 grid of windows over the content.
    let mut best: Option<(f32, Vec<f32>)> = None;
    for gy in 0..3u32 {
        for gx in 0..3u32 {
            let wx = content.x + ((content.w - side) * gx / 2) as i32;
            let wy = content.y + ((content.h - side) * gy / 2) as i32;
            let l = luma(&pixels.read_rect(Rect::new(wx, wy, side, side)));
            let a = activity(&l, side as usize, side as usize);
            if best.as_ref().is_none_or(|(b, _)| a > *b) {
                best = Some((a, l));
            }
        }
    }
    let (act, l) = best?;
    if act < 1.5 {
        return None;
    }
    let n = side as usize;
    let rgba: Vec<u8> = l.iter().flat_map(|v| {
        let v = v.round().clamp(0.0, 255.0) as u8;
        [v, v, v, 255]
    }).collect();
    use nori_render::transform::{Filter, resample_rgba};
    // Halve and double back (Lanczos both ways, close to an ideal low-pass): what is lost is the
    // detail finer than two pixels. A picture enlarged twice or more has almost none of it.
    let small = (side / 2).max(4);
    let down = resample_rgba(&rgba, side, side, small, small, Filter::Lanczos);
    let up = resample_rgba(&down, small, small, side, side, Filter::Lanczos);
    let err: f32 = up.chunks_exact(4).zip(&l).map(|(p, v)| (p[0] as f32 - v).abs()).sum::<f32>() / (n * n) as f32;
    let r = err / act;
    // Calibrated on pictures enlarged 2× and 3× with bicubic (tests below).
    Some(if r >= DETAIL_THRESHOLD {
        1.0
    } else if r >= 0.22 {
        2.0
    } else if r >= 0.1 {
        3.0
    } else {
        4.0
    })
}

// ---- print colour -----------------------------------------------------------------------------

/// Bright, saturated greens, cyans, blues and violets: RGB colours outside a press's CMYK gamut.
pub fn out_of_cmyk(c: nori_core::Color) -> bool {
    let (r, g, b) = (c.r.clamp(0.0, 1.0), c.g.clamp(0.0, 1.0), c.b.clamp(0.0, 1.0));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max < 0.7 || max - min < 0.75 * max {
        return false;
    }
    let d = max - min;
    let hue = if max == r { 60.0 * (((g - b) / d).rem_euclid(6.0)) } else if max == g { 60.0 * ((b - r) / d + 2.0) } else { 60.0 * ((r - g) / d + 4.0) };
    (90.0..=290.0).contains(&hue)
}

/// The print-unfriendly colours each layer uses (once per layer).
fn print_colours<'a>(shown: &[&'a Layer]) -> Vec<(&'a Layer, String)> {
    let mut out = vec![];
    for l in shown {
        let colours: Vec<nori_core::Color> = match &l.content {
            Content::Text { text } => std::iter::once(text.color).chain(text.runs.iter().filter_map(|r| r.color)).collect(),
            Content::Fill { color } => vec![*color],
            Content::Vector { shape } => paint_colours(&shape.fill).into_iter().chain(shape.stroke.iter().flat_map(|s| paint_colours(&s.paint))).collect(),
            _ => vec![],
        };
        if let Some(c) = colours.into_iter().find(|c| out_of_cmyk(*c)) {
            out.push((*l, c.hex()));
        }
    }
    out
}

fn paint_colours(p: &nori_core::Paint) -> Vec<nori_core::Color> {
    use nori_core::Paint;
    match p {
        Paint::None => vec![],
        Paint::Solid { color } => vec![*color],
        Paint::Linear { stops, .. } | Paint::Radial { stops, .. } => stops.iter().map(|s| s.color).collect(),
    }
}

/// One line per page for people: "Page 1 (210×297 mm at 300 dpi): 2 errors, 1 warning."
pub fn summary(r: &Report, dpi: f32) -> String {
    format!(
        "Page {} ({}×{} mm at {} dpi): {} error{}, {} warning{}.",
        r.number,
        mm(r.width, dpi),
        mm(r.height, dpi),
        trim_num(dpi as f64),
        r.errors,
        if r.errors == 1 { "" } else { "s" },
        r.warnings,
        if r.warnings == 1 { "" } else { "s" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_ratios_follow_wcag() {
        assert!((contrast_ratio([0.0; 3], [255.0; 3]) - 21.0).abs() < 0.01);
        assert!((contrast_ratio([255.0; 3], [255.0; 3]) - 1.0).abs() < 0.01);
        // #767676 on white is the classic 4.54:1.
        let r = contrast_ratio([118.0; 3], [255.0; 3]);
        assert!((r - 4.54).abs() < 0.02, "{r}");
    }

    #[test]
    fn bright_rgb_greens_and_blues_are_out_of_cmyk() {
        assert!(out_of_cmyk(nori_core::Color::rgb(0.0, 1.0, 0.2)));
        assert!(out_of_cmyk(nori_core::Color::rgb(0.1, 0.2, 1.0)));
        assert!(!out_of_cmyk(nori_core::Color::rgb(0.9, 0.1, 0.1)), "reds print");
        assert!(!out_of_cmyk(nori_core::Color::rgb(0.2, 0.4, 0.6)), "muted blues print");
        assert!(!out_of_cmyk(nori_core::Color::rgb(0.0, 0.0, 0.0)));
    }

    /// A busy picture: sharp edges and fine texture everywhere.
    fn detailed(w: u32, h: u32) -> Vec<u8> {
        let mut seed = 12345u32;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let noise = (seed >> 24) as f32 / 255.0 * 60.0;
                let stripes = if ((x / 3) + (y / 5)) % 2 == 0 { 150.0 } else { 70.0 };
                let v = (stripes + noise).clamp(0.0, 255.0) as u8;
                out.extend_from_slice(&[v, v.saturating_sub(20), v / 2, 255]);
            }
        }
        out
    }

    #[test]
    fn an_enlarged_picture_shows_how_much() {
        use nori_render::transform::{Filter, resample_rgba};
        let (w, h) = (300u32, 300u32);
        let sharp = detailed(w, h);
        let r = nori_core::Raster::from_rgba(w, h, &sharp);
        assert_eq!(enlargement(&r, r.bounds()), Some(1.0), "a sharp picture isn't enlarged");
        for k in [2u32, 3] {
            let small = detailed(w / k, h / k);
            let big = resample_rgba(&small, w / k, h / k, w, h, Filter::Bicubic);
            let r = nori_core::Raster::from_rgba(w, h, &big);
            let e = enlargement(&r, r.bounds()).unwrap();
            assert!(e >= k as f32 * 0.75, "enlarged {k}× reads as {e}×");
        }
        let flat = nori_core::Raster::from_rgba(w, h, &vec![200u8; (w * h * 4) as usize]);
        assert_eq!(enlargement(&flat, flat.bounds()), None, "a flat picture can't tell");
    }
}
