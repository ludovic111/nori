//! Compositing: the layers, bottom to top, onto a premultiplied f32 buffer, one canvas tile at
//! a time (tiles in parallel). The window, the export and every command that looks at the
//! picture use this.

use std::collections::HashMap;
use std::sync::Arc;

use nori_core::{BlendMode, Content, Document, Layer, Page, Rect, TILE};
use rayon::prelude::*;

use crate::adjust::{Compiled, compile};
use crate::blend::{blend, over};
use crate::text::TextImage;

/// What compositing needs computed once per document state: compiled adjustments, and text and
/// vector layers drawn.
#[derive(Default)]
pub struct Prepared {
    adjust: HashMap<String, Compiled>,
    text: HashMap<String, Arc<TextImage>>,
    /// Text on master pages that says `{page}`, drawn for each page number it shows on.
    paged: HashMap<(String, usize), Arc<TextImage>>,
}

pub fn prepare(doc: &Document) -> Prepared {
    let mut p = Prepared::default();
    for l in doc.all() {
        match &l.content {
            Content::Adjustment { adjustment } => {
                p.adjust.insert(l.id.clone(), compile(adjustment));
            }
            Content::Text { .. } if !p.text.contains_key(&l.id) => {
                for (id, img) in crate::text::render_thread(doc, &l.id) {
                    p.text.insert(id, img);
                }
            }
            Content::Vector { shape } => {
                p.text.insert(l.id.clone(), crate::shapes::render(shape));
            }
            _ => {}
        }
    }
    for (i, page) in doc.pages.iter().enumerate() {
        let [_, master, _] = doc.page_stack(page);
        for l in master.iter().flat_map(|l| l.walk()) {
            if let Content::Text { text } = &l.content
                && crate::text::has_page_tokens(text)
            {
                for (id, img) in crate::text::render_thread_on(doc, &l.id, Some(i + 1)) {
                    p.paged.insert((id, i + 1), img);
                }
            }
        }
    }
    p
}

impl Prepared {
    pub fn text(&self, id: &str) -> Option<&Arc<TextImage>> {
        self.text.get(id)
    }

    /// A layer's picture as drawn on page number `page` (master text says that page's number).
    pub fn text_on(&self, id: &str, page: usize) -> Option<&Arc<TextImage>> {
        if page > 0
            && let Some(img) = self.paged.get(&(id.to_string(), page))
        {
            return Some(img);
        }
        self.text.get(id)
    }
}

/// A premultiplied pixel.
pub type Px = [f32; 4];

#[inline]
fn u8f(v: u8) -> f32 {
    v as f32 * (1.0 / 255.0)
}

/// The mask value of a layer over `r` (none: everything shows).
fn mask_of(l: &Layer, r: Rect) -> Option<Vec<u8>> {
    l.mask.as_ref().filter(|m| m.enabled).map(|m| m.mask.read_rect(r))
}

/// The active page inside `r` (page pixels), premultiplied, rows of `r.w`.
pub fn composite_rect(doc: &Document, prep: &Prepared, r: Rect) -> Vec<Px> {
    composite_page(doc, doc.page(), prep, r)
}

/// A page inside `r`: its paper, its master's layers, then its own (`Document::page_stack`),
/// premultiplied, rows of `r.w`.
pub fn composite_page(doc: &Document, page: &Page, prep: &Prepared, r: Rect) -> Vec<Px> {
    let mut buf = vec![[0f32; 4]; (r.w * r.h) as usize];
    let n = doc.page_number(&page.id);
    for list in doc.page_stack(page) {
        composite_list(list, prep, r, &mut buf, n);
    }
    buf
}

/// `page`: the number of the page being drawn (what master text says for `{page}`).
fn composite_list(layers: &[Layer], prep: &Prepared, r: Rect, buf: &mut [Px], page: usize) {
    // The coverage of the last layer that wasn't clipped: layers clipped to it show only there.
    let mut base: Option<Vec<f32>> = None;
    for l in layers.iter().rev() {
        if !l.clipped {
            base = None;
        }
        let clip = if l.clipped { base.as_deref() } else { None };
        let coverage = draw_layer(l, prep, r, buf, clip, page);
        if !l.clipped {
            base = Some(coverage.unwrap_or_else(|| vec![0.0; buf.len()]));
        }
    }
}

/// Draws one layer onto `buf`; returns its coverage (for layers clipped to it) when it has one.
fn draw_layer(l: &Layer, prep: &Prepared, r: Rect, buf: &mut [Px], clip: Option<&[f32]>, page: usize) -> Option<Vec<f32>> {
    if !l.visible || l.opacity <= 0.0 {
        return None;
    }
    let mask = mask_of(l, r);
    let k_at = |i: usize| -> f32 {
        let mut k = l.opacity;
        if let Some(m) = &mask {
            k *= u8f(m[i]);
        }
        if let Some(c) = clip {
            k *= c[i];
        }
        k
    };
    match &l.content {
        Content::Raster { x, y, pixels } => {
            let lr = Rect::new(*x, *y, pixels.width(), pixels.height()).intersect(&r);
            if lr.is_empty() {
                return Some(vec![0.0; buf.len()]);
            }
            let px = pixels.read_rect(r.translate(-x, -y));
            let mut cov = vec![0f32; buf.len()];
            for (i, d) in buf.iter_mut().enumerate() {
                let a = px[i * 4 + 3];
                if a == 0 {
                    continue;
                }
                let s = [u8f(px[i * 4]), u8f(px[i * 4 + 1]), u8f(px[i * 4 + 2])];
                cov[i] = u8f(a) * mask.as_ref().map_or(1.0, |m| u8f(m[i]));
                over(d, s, u8f(a) * k_at(i), l.blend);
            }
            Some(cov)
        }
        Content::Fill { color } => {
            let s = [color.r, color.g, color.b];
            for (i, d) in buf.iter_mut().enumerate() {
                over(d, s, color.a * k_at(i), l.blend);
            }
            Some(match &mask {
                Some(m) => m.iter().map(|v| u8f(*v) * color.a).collect(),
                None => vec![color.a; buf.len()],
            })
        }
        Content::Text { .. } | Content::Vector { .. } => {
            let img = prep.text_on(&l.id, page)?;
            if img.rect.intersect(&r).is_empty() {
                return Some(vec![0.0; buf.len()]);
            }
            let mut cov = vec![0f32; buf.len()];
            for row in 0..r.h as i32 {
                for col in 0..r.w as i32 {
                    let p = img.get(r.x + col, r.y + row);
                    if p[3] == 0 {
                        continue;
                    }
                    let i = (row as u32 * r.w + col as u32) as usize;
                    cov[i] = u8f(p[3]) * mask.as_ref().map_or(1.0, |m| u8f(m[i]));
                    over(&mut buf[i], [u8f(p[0]), u8f(p[1]), u8f(p[2])], u8f(p[3]) * k_at(i), l.blend);
                }
            }
            Some(cov)
        }
        Content::Adjustment { .. } => {
            let Some(c) = prep.adjust.get(&l.id) else { return None };
            for (i, d) in buf.iter_mut().enumerate() {
                let a = d[3];
                if a <= 0.0 {
                    continue;
                }
                let k = k_at(i);
                if k <= 0.0 {
                    continue;
                }
                let inv = 1.0 / a;
                let b = [d[0] * inv, d[1] * inv, d[2] * inv];
                let adj = c.apply(b);
                let mixed = blend(l.blend, b, adj);
                for ch in 0..3 {
                    d[ch] = (b[ch] + (mixed[ch] - b[ch]) * k).clamp(0.0, 1.0) * a;
                }
            }
            None
        }
        Content::Group { children, .. } => {
            if l.blend == BlendMode::PassThrough {
                let before = buf.to_vec();
                composite_list(children, prep, r, buf, page);
                if l.opacity < 1.0 || mask.is_some() || clip.is_some() {
                    for (i, d) in buf.iter_mut().enumerate() {
                        let k = k_at(i);
                        for ch in 0..4 {
                            d[ch] = before[i][ch] + (d[ch] - before[i][ch]) * k;
                        }
                    }
                }
                None
            } else {
                let mut inner = vec![[0f32; 4]; buf.len()];
                composite_list(children, prep, r, &mut inner, page);
                let mut cov = vec![0f32; buf.len()];
                for (i, d) in buf.iter_mut().enumerate() {
                    let a = inner[i][3];
                    if a <= 0.0 {
                        continue;
                    }
                    let inv = 1.0 / a;
                    cov[i] = a;
                    over(d, [inner[i][0] * inv, inner[i][1] * inv, inner[i][2] * inv], a * k_at(i), l.blend);
                }
                Some(cov)
            }
        }
    }
}

/// Premultiplied f32 → straight RGBA8.
pub fn to_rgba8(buf: &[Px]) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len() * 4);
    for p in buf {
        let a = p[3].clamp(0.0, 1.0);
        if a <= 0.0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let inv = 1.0 / a;
        let q = |v: f32| ((v * inv).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        out.extend_from_slice(&[q(p[0]), q(p[1]), q(p[2]), (a * 255.0 + 0.5) as u8]);
    }
    out
}

/// Every tile of a `width`×`height` page (row by row).
pub fn page_tiles(width: u32, height: u32) -> Vec<Rect> {
    let (gx, gy) = (width.div_ceil(TILE), height.div_ceil(TILE));
    (0..gy)
        .flat_map(|ty| (0..gx).map(move |tx| (tx, ty)))
        .map(|(tx, ty)| {
            let (x, y) = (tx * TILE, ty * TILE);
            Rect::new(x as i32, y as i32, TILE.min(width - x), TILE.min(height - y))
        })
        .collect()
}

/// The active page as straight RGBA rows (tiles composited in parallel).
pub fn flatten(doc: &Document) -> Vec<u8> {
    let prep = prepare(doc);
    flatten_with(doc, &prep, doc.bounds())
}

/// A page as straight RGBA rows.
pub fn flatten_page(doc: &Document, page: &Page) -> Vec<u8> {
    let prep = prepare(doc);
    flatten_page_with(doc, page, &prep, page.bounds())
}

/// The active page inside `area` as straight RGBA rows.
pub fn flatten_with(doc: &Document, prep: &Prepared, area: Rect) -> Vec<u8> {
    flatten_page_with(doc, doc.page(), prep, area)
}

/// A page inside `area` as straight RGBA rows.
pub fn flatten_page_with(doc: &Document, page: &Page, prep: &Prepared, area: Rect) -> Vec<u8> {
    let tiles: Vec<Rect> = page_tiles(page.width, page.height).into_iter().map(|t| t.intersect(&area)).filter(|t| !t.is_empty()).collect();
    let parts: Vec<(Rect, Vec<u8>)> = tiles.par_iter().map(|t| (*t, to_rgba8(&composite_page(doc, page, prep, *t)))).collect();
    let mut out = vec![0u8; (area.w * area.h * 4) as usize];
    for (t, data) in parts {
        for row in 0..t.h {
            let src = (row * t.w * 4) as usize;
            let dst = (((t.y - area.y) as u32 + row) * area.w + (t.x - area.x) as u32) as usize * 4;
            out[dst..dst + (t.w * 4) as usize].copy_from_slice(&data[src..src + (t.w * 4) as usize]);
        }
    }
    out
}

/// The colour of the picture at one canvas pixel (straight RGBA8), as the eyedropper sees it.
pub fn pick(doc: &Document, x: i32, y: i32) -> [u8; 4] {
    let prep = prepare(doc);
    to_rgba8(&composite_rect(doc, &prep, Rect::new(x, y, 1, 1))).try_into().unwrap_or([0; 4])
}

/// One layer drawn on its own over its page (for thumbnails and `layer.look`): straight RGBA
/// rows of the page's size scaled to fit `max` pixels.
pub fn layer_image(doc: &Document, id: &str, max: u32) -> Option<(u32, u32, Vec<u8>)> {
    let mut l = doc.layer(id)?.clone();
    let page = doc.page_of(id)?.clone();
    l.visible = true;
    l.clipped = false;
    let mut alone = page.clone();
    alone.master = None;
    // An adjustment alone shows what it does to the picture.
    if !matches!(l.content, Content::Adjustment { .. }) {
        alone.layers = vec![l];
    }
    let full = flatten_page(doc, &alone);
    Some(crate::transform::fit_rgba(&full, page.width, page.height, max))
}

/// Whether a layer or any layer in it is an adjustment (which changes everything under it).
pub fn has_adjustment(l: &Layer) -> bool {
    l.walk().iter().any(|x| matches!(x.content, Content::Adjustment { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_core::{Adjustment, Color, LayerMask, Mask, Raster};

    fn doc_with(top: Layer) -> Document {
        let mut d = Document::new("t", 4, 4, Some(Color::rgb(0.0, 0.0, 1.0)));
        d.insert_above(top, None);
        d
    }

    #[test]
    fn normal_multiply_and_opacity() {
        let red = Layer::new("L9", "red", Content::Fill { color: Color::rgb(1.0, 0.0, 0.0) });
        let out = flatten(&doc_with(red.clone()));
        assert_eq!(&out[0..4], &[255, 0, 0, 255]);
        let mut half = red.clone();
        half.opacity = 0.5;
        let out = flatten(&doc_with(half));
        assert_eq!(&out[0..4], &[128, 0, 128, 255]);
        let mut mul = red;
        mul.blend = BlendMode::Multiply;
        let out = flatten(&doc_with(mul));
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn masks_adjustments_groups_and_clipping() {
        // A mask hides the left half.
        let mut red = Layer::new("L9", "red", Content::Fill { color: Color::rgb(1.0, 0.0, 0.0) });
        let mut m = Mask::filled(4, 4, 255);
        for y in 0..4 {
            m.set(0, y, [0]);
            m.set(1, y, [0]);
        }
        red.mask = Some(LayerMask { mask: m, enabled: true });
        let out = flatten(&doc_with(red));
        assert_eq!(&out[0..4], &[0, 0, 255, 255]);
        assert_eq!(&out[12..16], &[255, 0, 0, 255]);

        // An invert adjustment turns blue to yellow.
        let inv = Layer::new("L9", "inv", Content::Adjustment { adjustment: Adjustment::Invert });
        assert_eq!(&flatten(&doc_with(inv.clone()))[0..4], &[255, 255, 0, 255]);

        // Inside a group (isolated), it only reaches the group's own layers: nothing below.
        let g = Layer::new("L10", "g", Content::Group { children: vec![inv.clone()], expanded: true });
        assert_eq!(&flatten(&doc_with(g))[0..4], &[0, 0, 255, 255]);
        // Pass-through groups let it through.
        let mut g = Layer::new("L10", "g", Content::Group { children: vec![inv], expanded: true });
        g.blend = BlendMode::PassThrough;
        assert_eq!(&flatten(&doc_with(g))[0..4], &[255, 255, 0, 255]);

        // A layer clipped to a layer with one pixel shows only there.
        let mut d = Document::new("t", 4, 4, None);
        let mut dot = Raster::transparent(4, 4);
        dot.set(2, 2, [0, 0, 0, 255]);
        d.page_mut().layers[0].content = Content::Raster { x: 0, y: 0, pixels: dot };
        let mut green = Layer::new("L5", "green", Content::Fill { color: Color::rgb(0.0, 1.0, 0.0) });
        green.clipped = true;
        d.insert_above(green, None);
        let out = flatten(&d);
        assert_eq!(&out[(2 * 4 + 2) * 4..(2 * 4 + 2) * 4 + 4], &[0, 255, 0, 255]);
        assert_eq!(&out[0..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn big_canvases_composite_across_tiles() {
        let d = Document::new("big", 700, 300, Some(Color::rgb(0.5, 0.5, 0.5)));
        let out = flatten(&d);
        assert_eq!(out.len(), 700 * 300 * 4);
        assert!(out.chunks_exact(4).all(|p| p == [128, 128, 128, 255]));
        assert_eq!(pick(&d, 650, 290), [128, 128, 128, 255]);
    }

    #[test]
    fn masters_sit_on_the_paper_and_number_each_page() {
        let mut d = Document::new("t", 160, 80, Some(Color::WHITE));
        let mut p2 = d.pages[0].clone();
        p2.id = "P2".into();
        d.pages.push(p2);
        let mut m = nori_core::Page::new("M1", "A-Master", 160, 80);
        let t = nori_core::TextLayer { text: "{page}".into(), size: 60.0, x: 10.0, y: 5.0, color: Color::rgb(0.0, 0.0, 0.0), ..Default::default() };
        m.layers.push(Layer::new("L50", "Folio", Content::Text { text: t }));
        d.masters.push(m);
        for p in &mut d.pages {
            p.master = Some("M1".into());
        }
        let one = flatten_page(&d, &d.pages[0]);
        let two = flatten_page(&d, &d.pages[1]);
        let ink = |px: &[u8]| px.chunks_exact(4).filter(|p| p[0] < 100).count();
        assert!(ink(&one) > 50, "the white paper hides the master");
        assert!(ink(&two) > 50);
        assert_ne!(one, two, "both pages say the same number");
    }
}
