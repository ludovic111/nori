//! What a page export can keep as vectors: SVG and PDF can't say "adjust everything under me",
//! nor every blend mode, so the layers under the topmost such layer are drawn into one picture
//! and the ones above stay vectors, text and images.

use nori_core::{BlendMode, Content, Document, Layer, Page, Raster};

/// Blend modes PDF and SVG both have.
pub fn portable_blend(b: BlendMode) -> bool {
    matches!(
        b,
        BlendMode::Normal
            | BlendMode::PassThrough
            | BlendMode::Multiply
            | BlendMode::Screen
            | BlendMode::Overlay
            | BlendMode::Darken
            | BlendMode::Lighten
            | BlendMode::ColorDodge
            | BlendMode::ColorBurn
            | BlendMode::HardLight
            | BlendMode::SoftLight
            | BlendMode::Difference
            | BlendMode::Exclusion
            | BlendMode::Hue
            | BlendMode::Saturation
            | BlendMode::Color
            | BlendMode::Luminosity
    )
}

/// Whether a layer (and everything in it) can be written as it is.
pub fn portable(l: &Layer) -> bool {
    portable_blend(l.blend)
        && !l.clipped
        && l.mask.is_none()
        && match &l.content {
            Content::Adjustment { .. } => false,
            Content::Group { children, .. } => children.iter().all(portable),
            _ => true,
        }
}

/// The page's layers (its master's under its own), split into a picture of everything up to
/// and including the topmost layer that can't be written as it is, and the layers above it
/// (top first).
pub fn split(doc: &Document, page: &Page) -> (Option<(Raster, i32, i32)>, Vec<Layer>) {
    let mut all: Vec<Layer> = page.layers.clone();
    if let Some(m) = page.master.as_deref().and_then(|id| doc.master_index(id)).map(|i| &doc.masters[i]) {
        all.extend(m.layers.iter().cloned());
    }
    match all.iter().position(|l| !portable(l)) {
        None => (None, all),
        Some(k) => {
            let mut below = page.clone();
            below.master = None;
            below.layers = all[k..].to_vec();
            let rgba = nori_render::flatten_page(doc, &below);
            (Some((Raster::from_rgba(page.width, page.height, &rgba), 0, 0)), all[..k].to_vec())
        }
    }
}
