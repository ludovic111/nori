//! PDF export (through `krilla`): every page of the document, at its size in points (pixels ×
//! 72 / dpi), vector layers as PDF paths with their fills, strokes and gradients, text as
//! searchable glyphs with embedded fonts, pixels as images, opacity and blend modes as PDF's own. What
//! PDF can't say (adjustment layers, masks, clipping) is drawn into a picture with what is under
//! it (`plan::split`).

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, LinearGradient, RadialGradient, SpreadMethod, Stop as KStop, Stroke, StrokeDash};
use nori_core::vector::{Cap, Join, Paint, Seg};
use nori_core::{BlendMode, Color, Content, Document, Layer, Page, Raster};

fn n(v: f32) -> NormalizedF32 {
    NormalizedF32::new(v.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE)
}

fn rgb_of(c: &Color) -> rgb::Color {
    let [r, g, b, _] = c.to_u8();
    rgb::Color::new(r, g, b)
}

fn paint(p: &Paint) -> Option<(krilla::paint::Paint, f32)> {
    let stops = |s: &[nori_core::vector::Stop]| -> Vec<KStop> { s.iter().map(|st| KStop { offset: n(st.offset), color: rgb_of(&st.color).into(), opacity: n(st.color.a) }).collect() };
    Some(match p {
        Paint::None => return None,
        Paint::Solid { color } => (rgb_of(color).into(), color.a),
        Paint::Linear { x1, y1, x2, y2, stops: s } => (
            LinearGradient { x1: *x1, y1: *y1, x2: *x2, y2: *y2, transform: Transform::default(), spread_method: SpreadMethod::Pad, stops: stops(s), anti_alias: true }.into(),
            1.0,
        ),
        Paint::Radial { cx, cy, r, stops: s } => (
            RadialGradient { fx: *cx, fy: *cy, fr: 0.0, cx: *cx, cy: *cy, cr: *r, transform: Transform::default(), spread_method: SpreadMethod::Pad, stops: stops(s), anti_alias: true }.into(),
            1.0,
        ),
    })
}

fn path_of(segs: &[Seg]) -> Option<krilla::geom::Path> {
    let mut pb = PathBuilder::new();
    for s in segs {
        match *s {
            Seg::Move(p) => pb.move_to(p[0], p[1]),
            Seg::Line(p) => pb.line_to(p[0], p[1]),
            Seg::Cubic(a, b, p) => pb.cubic_to(a[0], a[1], b[0], b[1], p[0], p[1]),
            Seg::Close => pb.close(),
        }
    }
    pb.finish()
}

fn blend(b: BlendMode) -> krilla::blend::BlendMode {
    use krilla::blend::BlendMode as K;
    match b {
        BlendMode::Multiply => K::Multiply,
        BlendMode::Screen => K::Screen,
        BlendMode::Overlay => K::Overlay,
        BlendMode::Darken => K::Darken,
        BlendMode::Lighten => K::Lighten,
        BlendMode::ColorDodge => K::ColorDodge,
        BlendMode::ColorBurn => K::ColorBurn,
        BlendMode::HardLight => K::HardLight,
        BlendMode::SoftLight => K::SoftLight,
        BlendMode::Difference => K::Difference,
        BlendMode::Exclusion => K::Exclusion,
        BlendMode::Hue => K::Hue,
        BlendMode::Saturation => K::Saturation,
        BlendMode::Color => K::Color,
        BlendMode::Luminosity => K::Luminosity,
        _ => K::Normal,
    }
}

fn image(s: &mut krilla::surface::Surface, pixels: &Raster, x: i32, y: i32) {
    let img = krilla::image::Image::from_rgba8(pixels.to_vec(), pixels.width(), pixels.height());
    if let Some(size) = Size::from_wh(pixels.width() as f32, pixels.height() as f32) {
        s.push_transform(&Transform::from_translate(x as f32, y as f32));
        s.draw_image(img, size);
        s.pop();
    }
}

fn layers(s: &mut krilla::surface::Surface, doc: &Document, page: &Page, list: &[Layer]) {
    for l in list.iter().rev() {
        if !l.visible || l.opacity <= 0.0 {
            continue;
        }
        let mut pushed = 0;
        if !matches!(l.blend, BlendMode::Normal | BlendMode::PassThrough) {
            s.push_blend_mode(blend(l.blend));
            pushed += 1;
        }
        if l.opacity < 1.0 {
            s.push_opacity(n(l.opacity));
            pushed += 1;
        }
        match &l.content {
            Content::Group { children, .. } => {
                if l.blend != BlendMode::PassThrough {
                    s.push_isolated();
                    pushed += 1;
                }
                layers(s, doc, page, children);
            }
            Content::Vector { shape } => {
                if let Some(path) = path_of(&shape.transformed()) {
                    s.set_fill(paint(&shape.fill).map(|(p, o)| Fill {
                        paint: p,
                        opacity: n(o),
                        rule: if shape.fill_rule == nori_core::vector::FillRule::EvenOdd { FillRule::EvenOdd } else { FillRule::NonZero },
                    }));
                    s.set_stroke(shape.stroke.as_ref().and_then(|st| {
                        let (p, o) = paint(&st.paint)?;
                        Some(Stroke {
                            paint: p,
                            width: st.width,
                            miter_limit: st.miter_limit.max(1.0),
                            line_cap: match st.cap {
                                Cap::Butt => LineCap::Butt,
                                Cap::Round => LineCap::Round,
                                Cap::Square => LineCap::Square,
                            },
                            line_join: match st.join {
                                Join::Miter => LineJoin::Miter,
                                Join::Round => LineJoin::Round,
                                Join::Bevel => LineJoin::Bevel,
                            },
                            opacity: n(o),
                            dash: (st.dash.len() >= 2).then(|| StrokeDash { array: st.dash.clone(), offset: 0.0 }),
                        })
                    }));
                    s.draw_path(&path);
                    s.set_stroke(None);
                }
            }
            Content::Text { .. } => {
                use krilla::text::{Font, GlyphId, KrillaGlyph};
                use krilla::geom::Point;
                let on = doc.pages.iter().position(|p| p.id == page.id);
                let glyphs = nori_render::text::pdf_glyphs_on(doc, &l.id, on);
                let mut fonts = std::collections::HashMap::new();
                for g in &glyphs {
                    let key = (std::sync::Arc::as_ptr(&g.font) as usize, g.font_index);
                    let font = fonts.entry(key).or_insert_with(|| Font::new(g.font.as_ref().clone().into(), g.font_index));
                    let Some(font) = font else { continue };
                    let glyph = KrillaGlyph { glyph_id: GlyphId::new(g.id as u32), text_range: 0..g.text.len(), x_advance: g.advance / g.size, x_offset: 0.0, y_offset: 0.0, y_advance: 0.0, location: None };
                    s.set_fill(Some(Fill { paint: rgb_of(&g.color).into(), opacity: n(g.color.a), rule: FillRule::NonZero }));
                    s.set_stroke(None);
                    s.draw_glyphs(Point::from_xy(g.x, g.y), &[glyph], font.clone(), &g.text, g.size, false);
                }
            }
            Content::Fill { color } => {
                let mut pb = PathBuilder::new();
                // A fill covers the bleed too: it is the page's paper.
                let b = page.bleed.max(0.0);
                if let Some(r) = krilla::geom::Rect::from_xywh(-b, -b, page.width as f32 + 2.0 * b, page.height as f32 + 2.0 * b) {
                    pb.push_rect(r);
                }
                if let Some(path) = pb.finish() {
                    s.set_fill(Some(Fill { paint: rgb_of(color).into(), opacity: n(color.a), rule: FillRule::NonZero }));
                    s.set_stroke(None);
                    s.draw_path(&path);
                }
            }
            Content::Raster { x, y, pixels } => image(s, pixels, *x, *y),
            Content::Adjustment { .. } => {}
        }
        for _ in 0..pushed {
            s.pop();
        }
    }
}

/// The document's pages as a PDF (`pages`: which ones, 0-based; none: all).
pub fn write(doc: &Document, pages: Option<&[usize]>) -> Result<Vec<u8>, String> {
    let mut pdf = krilla::Document::new();
    let k = 72.0 / doc.dpi.max(1.0);
    let list: Vec<usize> = match pages {
        Some(p) => p.iter().copied().filter(|i| *i < doc.pages.len()).collect(),
        None => (0..doc.pages.len()).collect(),
    };
    if list.is_empty() {
        return Err("No pages to write.".into());
    }
    for i in list {
        let page = &doc.pages[i];
        // With a bleed the sheet is bigger than the page by the bleed on every side: the trim
        // box marks the page, the bleed box the sheet, and artwork past the edge prints on.
        let b = page.bleed.max(0.0) * k;
        let (w, h) = (page.width as f32 * k, page.height as f32 * k);
        let mut settings = krilla::page::PageSettings::from_wh(w + 2.0 * b, h + 2.0 * b).ok_or("a page has no size")?;
        if b > 0.0 {
            settings = settings.with_trim_box(krilla::geom::Rect::from_xywh(b, b, w, h)).with_bleed_box(krilla::geom::Rect::from_xywh(0.0, 0.0, w + 2.0 * b, h + 2.0 * b));
        }
        let mut p = pdf.start_page_with(settings);
        let mut s = p.surface();
        s.push_transform(&Transform::from_row(k, 0.0, 0.0, k, b, b));
        let (flat, above) = crate::plan::split(doc, page);
        if let Some((pixels, x, y)) = flat {
            image(&mut s, &pixels, x, y);
        }
        layers(&mut s, doc, page, &above);
        s.pop();
        s.finish();
        p.finish();
    }
    pdf.finish().map_err(|e| format!("Couldn't write the PDF: {e:?}"))
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use nori_core::vector::{Geometry, Shape};
    use nori_core::{Page, TextLayer};

    #[test]
    fn writes_pages_with_vectors_text_and_pixels() {
        let mut d = Document::new("Booklet", 595, 842, Some(Color::WHITE));
        let id = d.new_id();
        d.insert_above(Layer::new(id, "Box", Content::Vector { shape: Shape::new(Geometry::Rect { x: 50.0, y: 50.0, w: 200.0, h: 100.0, radius: 12.0 }, Paint::solid(Color::rgb(0.9, 0.2, 0.1)), None) }), None);
        let id = d.new_id();
        d.insert_above(Layer::new(id, "Title", Content::Text { text: TextLayer { text: "Hello PDF".into(), size: 40.0, x: 60.0, y: 200.0, ..Default::default() } }), None);
        let p2 = d.new_page_id();
        d.pages.push(Page::new(p2, "Page 2", 595, 842));
        let bytes = write(&d, None).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.len() > 1000);
        assert!(String::from_utf8_lossy(&bytes).contains("/ToUnicode"), "PDF text must have Unicode mappings");
        let back = read(&bytes, "PDF roundtrip").unwrap();
        assert_eq!(back.pages.len(), 2);
        assert!(back.pages[0].layers.len() > 1);
    }

    #[test]
    fn a_bleed_makes_the_sheet_bigger_and_marks_the_trim() {
        let mut d = Document::new("Flyer", 2480, 3508, Some(Color::WHITE));
        d.dpi = 300.0;
        let id = d.new_id();
        d.insert_above(Layer::new(id, "Full bleed", Content::Vector { shape: Shape::new(Geometry::Rect { x: -35.0, y: -35.0, w: 2550.0, h: 3578.0, radius: 0.0 }, Paint::solid(Color::rgb(0.1, 0.2, 0.4)), None) }), None);
        let plain = String::from_utf8_lossy(&write(&d, None).unwrap()).into_owned();
        assert!(!plain.contains("/TrimBox"), "no bleed, no trim box");
        d.pages[0].bleed = 35.0;
        let bled = String::from_utf8_lossy(&write(&d, None).unwrap()).into_owned();
        assert!(bled.contains("/TrimBox") && bled.contains("/BleedBox"), "the trim and the bleed are marked");
        // 2480 px at 300 dpi is 595.2 pt; 35 px is 8.4 pt on each side: a 612 pt wide sheet.
        let media = bled.find("/MediaBox").map(|i| bled[i..].chars().take(60).collect::<String>()).unwrap_or_default();
        assert!(media.contains("612"), "the sheet is the page plus the bleed: {media}");
    }
}

/// Opens PDF pages, including Illustrator files saved with PDF compatibility. Geometry stays
/// editable; font glyphs become paths. Illustrator's private editing data is not interpreted.
pub fn read(bytes: &[u8], name: &str) -> Result<Document, String> {
    use hayro_svg::{RenderCache, SvgRenderSettings, hayro_interpret::InterpreterSettings, hayro_syntax::Pdf};
    if !bytes.starts_with(b"%PDF-") {
        return Err("This Illustrator file has no PDF compatibility data. Save it with Create PDF Compatible File enabled, or export SVG.".into());
    }
    let pdf = Pdf::new(bytes.to_vec()).map_err(|e| format!("Couldn't read the PDF data: {e:?}"))?;
    if pdf.pages().is_empty() || pdf.pages().len() > 500 { return Err("PDF import supports 1–500 pages.".into()); }
    let cache = RenderCache::new();
    let mut doc = Document::empty(name, 1, 1);
    doc.pages.clear(); doc.dpi = 72.0;
    for (i, page) in pdf.pages().iter().enumerate() {
        let (w, h) = page.render_dimensions();
        if !w.is_finite() || !h.is_finite() || w < 1.0 || h < 1.0 || w > nori_core::MAX_SIDE as f32 || h > nori_core::MAX_SIDE as f32 { return Err("A PDF page exceeds nori's size limit.".into()); }
        let svg = hayro_svg::convert(page, &cache, &InterpreterSettings::default(), &SvgRenderSettings::default());
        let converted = crate::svg::read(svg.as_bytes(), name)?;
        let mut p = converted.pages.into_iter().next().ok_or("The PDF page has no content")?;
        p.id = doc.new_page_id(); p.name = format!("Page {}", i + 1);
        fn ids(doc: &mut Document, layers: &mut [Layer]) {
            for l in layers { l.id = doc.new_id(); if let Content::Group { children, .. } = &mut l.content { ids(doc, children); } }
        }
        ids(&mut doc, &mut p.layers);
        doc.pages.push(p);
    }
    doc.active_page = doc.pages[0].id.clone();
    doc.active = doc.pages[0].layers.first().map(|l| l.id.clone());
    Ok(doc)
}
