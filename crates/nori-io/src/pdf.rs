//! PDF export (through `krilla`): every page of the document, at its size in points (pixels ×
//! 72 / dpi), vector layers as PDF paths with their fills, strokes and gradients, text as
//! outlines (sharp at any zoom), pixels as images, opacity and blend modes as PDF's own. What
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

fn skia_path(p: &tiny_skia::Path) -> Option<krilla::geom::Path> {
    use tiny_skia::PathSegment as S;
    let mut pb = PathBuilder::new();
    for s in p.segments() {
        match s {
            S::MoveTo(p) => pb.move_to(p.x, p.y),
            S::LineTo(p) => pb.line_to(p.x, p.y),
            S::QuadTo(c, p) => pb.quad_to(c.x, c.y, p.x, p.y),
            S::CubicTo(a, b, p) => pb.cubic_to(a.x, a.y, b.x, b.y, p.x, p.y),
            S::Close => pb.close(),
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
                for (c, p) in nori_render::text::outlines_on(doc, &l.id, Some(doc.page_number(&page.id))) {
                    if let Some(path) = skia_path(&p) {
                        s.set_fill(Some(Fill { paint: rgb_of(&c).into(), opacity: n(c.a), rule: FillRule::NonZero }));
                        s.set_stroke(None);
                        s.draw_path(&path);
                    }
                }
            }
            Content::Fill { color } => {
                let mut pb = PathBuilder::new();
                if let Some(r) = krilla::geom::Rect::from_xywh(0.0, 0.0, page.width as f32, page.height as f32) {
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
        let settings = krilla::page::PageSettings::from_wh(page.width as f32 * k, page.height as f32 * k).ok_or("a page has no size")?;
        let mut p = pdf.start_page_with(settings);
        let mut s = p.surface();
        s.push_transform(&Transform::from_row(k, 0.0, 0.0, k, 0.0, 0.0));
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
    }
}
