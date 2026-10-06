//! nori-render: everything that turns a nori document into pixels, and every pixel operation.
//!
//! * [`composite`]: pages composited tile by tile (in parallel) in premultiplied f32 with the
//!   W3C blend modes ([`blend`]), masks, clipping, groups (isolated or pass-through),
//!   adjustment layers ([`adjust`]), text ([`text`]) and vector shapes ([`shapes`]).
//! * [`filters`]: blurs, sharpen, noise, pixelate, inside the selection.
//! * [`brush`]: the brush engine (stamps, hardness, flow, pressure) for brush and eraser.
//! * [`fill`]: paint bucket, magic wand regions, gradients.
//! * [`transform`]: resampling, scaling and rotating layers, flips, quarter turns.

pub mod adjust;
pub mod blend;
pub mod brush;
pub mod composite;
pub mod fill;
pub mod filters;
pub mod shapes;
pub mod text;
pub mod transform;

pub use composite::{Prepared, composite_page, composite_rect, flatten, flatten_page, prepare};
