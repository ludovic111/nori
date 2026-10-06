//! nori-core: the document model of nori, lsuite's image editor.
//!
//! * [`Document`]: pages (or artboards) and master pages, each a stack of [`Layer`]s
//!   (pixels, fills, adjustments, vector shapes and paths, text and text frames threaded
//!   across pages, groups) with opacity, blend modes and masks; paragraph and character
//!   styles; the active page and layer; the selection.
//! * [`raster`]: pixels in shared tiles, so copies are cheap and edits copy only what they
//!   touch.
//! * [`Editor`]: the one undo history every client edits through (batches, coalescing,
//!   checkpoints).
//! * [`file`]: the `.nori` file (a zip of `document.json` and PNGs).
//!
//! Drawing the document (compositing, filters, brushes, text) is `nori-render`'s job.

pub mod color;
pub mod document;
pub mod file;
pub mod history;
pub mod layer;
pub mod raster;
pub mod selection;
pub mod vector;

pub use color::Color;
pub use document::{CharacterStyle, Document, FORMAT, Guide, Loc, Margins, Page, PageRef, ParagraphStyle, Styles, Units};
pub use history::{EditError, Editor, StepInfo};
pub use layer::{Adjustment, BlendMode, Content, Layer, LayerMask, TextAlign, TextLayer, TextRun};
pub use vector::{Geometry, Paint, Shape};
pub use raster::{Mask, Raster, Rect, TILE};

/// The app's name, in one place (for a rename: this, the crate names, the file extension in
/// [`file::EXTENSION`] and the binaries).
pub const APP: &str = "nori";

/// The largest canvas side, in pixels (as other editors allow).
pub const MAX_SIDE: u32 = 30_000;

/// The closest candidate by edit distance, if it is close enough to be a typo.
pub fn closest<'a>(word: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let w = word.to_lowercase();
    candidates
        .iter()
        .map(|c| (levenshtein(&w, &c.to_lowercase()), *c))
        .filter(|(d, c)| *d <= (c.len().max(3) / 3).max(2))
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let cur = row[j + 1];
            row[j + 1] = if ca == b[j] { prev } else { 1 + prev.min(row[j]).min(row[j + 1]) };
            prev = cur;
        }
    }
    row[b.len()]
}
