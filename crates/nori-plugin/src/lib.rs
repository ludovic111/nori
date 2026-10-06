//! nori plugin SDK.
//!
//! A nori plugin is a Rust type implementing [`Filter`]: it declares an [`Info`] block and its
//! [`Param`]s, and changes pixels in [`Filter::process`]. Built as a `cdylib` with
//! [`export_plugins!`], nori loads it at runtime through the frozen C ABI in [`ffi`]:
//!
//! ```ignore
//! use nori_plugin::prelude::*;
//!
//! struct Duotone;
//! impl Filter for Duotone {
//!     const INFO: Info = Info::filter("com.example.duotone", "Duotone", "Example", "color", "Maps dark to one colour, light to another.");
//!     fn params() -> Vec<Param> { vec![Param::number("mix", "Mix", 0.0, 100.0, 100.0, "%")] }
//!     fn new() -> Self { Duotone }
//!     fn process(&mut self, tile: &mut Tile, values: &[f64]) {
//!         let mix = (values[0] / 100.0) as f32;
//!         for px in tile.pixels.iter_mut() { /* … */ }
//!     }
//! }
//! export_plugins!(Duotone);
//! ```
//!
//! Pixels are **premultiplied RGBA, `f32` in 0..1**, rows of `tile.width`. `tile.x`/`tile.y`
//! say where the tile sits on the page (so patterns line up across tiles), and a filter that
//! reads its neighbours (a blur) says how far with [`Filter::margin`]: the host hands it that
//! much more around the area it changes. nori runs filters off the interface thread; a filter
//! that panics is switched off and the picture is left as it was.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod ffi;

/// The ABI this SDK speaks (checked by the host before anything is called).
pub const ABI_VERSION: u32 = 1;
/// The symbol nori looks up in a plugin library.
pub const ENTRY_SYMBOL: &str = "nori_plugin_entry";

/// What kind of plugin it is. Only filters for now (pixels in, pixels out).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Filter,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Filter => "filter",
        }
    }
}

/// A plugin's identity.
#[derive(Clone, Copy, Debug)]
pub struct Info {
    /// Reverse-DNS, unique: `com.example.duotone`.
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    pub version: &'static str,
    /// Where it shows in the Filter menu: blur, sharpen, noise, stylize, color, distort, other.
    pub group: &'static str,
    pub description: &'static str,
    pub kind: Kind,
}

impl Info {
    pub const fn filter(id: &'static str, name: &'static str, vendor: &'static str, group: &'static str, description: &'static str) -> Self {
        Self { id, name, vendor, version: "0.1.0", group, description, kind: Kind::Filter }
    }

    pub const fn version(mut self, v: &'static str) -> Self {
        self.version = v;
        self
    }
}

/// A parameter: a number in a range, a whole number, a switch or a choice.
#[derive(Clone, Debug)]
pub struct Param {
    pub name: &'static str,
    pub label: &'static str,
    /// `number`, `integer`, `boolean` or `choice`.
    pub kind: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub unit: &'static str,
    /// For choices, the options (the value is the option's index).
    pub options: Vec<&'static str>,
}

impl Param {
    pub fn number(name: &'static str, label: &'static str, min: f64, max: f64, default: f64, unit: &'static str) -> Self {
        Self { name, label, kind: "number", min, max, default, unit, options: vec![] }
    }

    pub fn integer(name: &'static str, label: &'static str, min: f64, max: f64, default: f64, unit: &'static str) -> Self {
        Self { name, label, kind: "integer", min, max, default, unit, options: vec![] }
    }

    pub fn switch(name: &'static str, label: &'static str, default: bool) -> Self {
        Self { name, label, kind: "boolean", min: 0.0, max: 1.0, default: if default { 1.0 } else { 0.0 }, unit: "", options: vec![] }
    }

    pub fn choice(name: &'static str, label: &'static str, options: &[&'static str], default: usize) -> Self {
        Self { name, label, kind: "choice", min: 0.0, max: options.len().saturating_sub(1) as f64, default: default as f64, unit: "", options: options.to_vec() }
    }
}

/// Pixels to work on: premultiplied RGBA `f32`, rows of `width`.
pub struct Tile<'a> {
    pub pixels: &'a mut [[f32; 4]],
    pub width: u32,
    pub height: u32,
    /// Where the tile's top-left pixel is on the page.
    pub x: i32,
    pub y: i32,
}

impl Tile<'_> {
    /// The pixel at (x, y) in the tile.
    pub fn at(&self, x: u32, y: u32) -> [f32; 4] {
        self.pixels[(y * self.width + x) as usize]
    }
}

/// A filter plugin.
pub trait Filter: Send + 'static {
    const INFO: Info;
    fn params() -> Vec<Param>;
    fn new() -> Self
    where
        Self: Sized;
    /// How many pixels around the area it changes the filter reads (0: none).
    fn margin(&self, _values: &[f64]) -> u32 {
        0
    }
    /// Changes the tile's pixels. `values` are the parameters, in order, already in range.
    fn process(&mut self, tile: &mut Tile, values: &[f64]);
}

/// Colour helpers for filters.
pub mod color {
    /// Premultiplied → straight (r, g, b) and alpha.
    pub fn unpremultiply(p: [f32; 4]) -> ([f32; 3], f32) {
        if p[3] <= 0.0 { ([0.0; 3], 0.0) } else { ([p[0] / p[3], p[1] / p[3], p[2] / p[3]], p[3]) }
    }

    /// Straight → premultiplied.
    pub fn premultiply(c: [f32; 3], a: f32) -> [f32; 4] {
        [c[0] * a, c[1] * a, c[2] * a, a]
    }

    /// Rec. 709 luma.
    pub fn luma(c: [f32; 3]) -> f32 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    /// `#rrggbb` → (r, g, b) in 0..1.
    pub fn hex(s: &str) -> [f32; 3] {
        let h = s.trim_start_matches('#');
        let v = u32::from_str_radix(h, 16).unwrap_or(0);
        [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
    }
}

/// Everything a plugin usually needs.
pub mod prelude {
    pub use crate::color::{hex, luma, premultiply, unpremultiply};
    pub use crate::{Filter, Info, Kind, Param, Tile, export_plugins};
}

/// Exports filters from a `cdylib` (one `nori_plugin_entry` symbol).
#[macro_export]
macro_rules! export_plugins {
    ($($plugin:ty),+ $(,)?) => {
        static __NORI_TABLES: &[$crate::ffi::FilterVTable] = &[$($crate::ffi::vtable::<$plugin>()),+];
        static __NORI_ENTRY: $crate::ffi::Entry = $crate::ffi::Entry {
            abi_version: $crate::ABI_VERSION,
            plugin_count: <[&str]>::len(&[$(stringify!($plugin)),+]) as u32,
            plugin: __nori_plugin_at,
        };
        unsafe extern "C" fn __nori_plugin_at(index: u32) -> *const $crate::ffi::FilterVTable {
            match __NORI_TABLES.get(index as usize) {
                Some(t) => t as *const _,
                None => ::std::ptr::null(),
            }
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn nori_plugin_entry() -> *const $crate::ffi::Entry {
            &__NORI_ENTRY
        }
    };
}

/// Markdown for an agent writing a plugin (`plugin.guide` returns it). A test keeps it in step
/// with this crate.
pub const GUIDE: &str = include_str!("../GUIDE.md");

#[cfg(test)]
mod tests {
    use super::*;

    struct Invert;
    impl Filter for Invert {
        const INFO: Info = Info::filter("dev.nori.test.invert", "Invert", "nori", "color", "Turns colours around.");
        fn params() -> Vec<Param> {
            vec![Param::number("amount", "Amount", 0.0, 100.0, 100.0, "%"), Param::choice("mode", "Mode", &["rgb", "luma"], 0)]
        }
        fn new() -> Self {
            Invert
        }
        fn process(&mut self, tile: &mut Tile, values: &[f64]) {
            let k = (values[0] / 100.0) as f32;
            for p in tile.pixels.iter_mut() {
                for c in 0..3 {
                    p[c] += ((p[3] - p[c]) - p[c]) * k;
                }
            }
        }
    }

    struct Boom;
    impl Filter for Boom {
        const INFO: Info = Info::filter("dev.nori.test.boom", "Boom", "nori", "other", "Panics.");
        fn params() -> Vec<Param> {
            vec![]
        }
        fn new() -> Self {
            Boom
        }
        fn process(&mut self, _: &mut Tile, _: &[f64]) {
            panic!("boom");
        }
    }

    export_plugins!(Invert, Boom);

    #[test]
    fn the_abi_round_trips() {
        let entry = unsafe { &*nori_plugin_entry() };
        assert_eq!(entry.abi_version, ABI_VERSION);
        assert_eq!(entry.plugin_count, 2);
        let t = unsafe { &*(entry.plugin)(0) };
        let manifest = unsafe { ffi::manifest_json(t) }.unwrap();
        assert!(manifest.contains("\"id\":\"dev.nori.test.invert\""), "{manifest}");
        assert!(manifest.contains("\"options\":[\"rgb\",\"luma\"]"));
        assert!(manifest.contains(&format!("\"abi\":{ABI_VERSION}")));
        let inst = unsafe { (t.create)() };
        assert!(!inst.is_null());
        let mut px = vec![[0.25f32, 0.5, 1.0, 1.0]; 4];
        let mut raw = ffi::RawTile { pixels: px.as_mut_ptr() as *mut f32, width: 2, height: 2, x: 0, y: 0 };
        let values = [100.0f64, 0.0];
        assert_eq!(unsafe { (t.process)(inst, &mut raw, values.as_ptr(), 2) }, 0);
        assert_eq!(px[0], [0.75, 0.5, 0.0, 1.0]);
        unsafe { (t.destroy)(inst) };
        assert!(unsafe { (entry.plugin)(9) }.is_null());
    }

    #[test]
    fn a_panic_is_contained() {
        let entry = unsafe { &*nori_plugin_entry() };
        let t = unsafe { &*(entry.plugin)(1) };
        let inst = unsafe { (t.create)() };
        let mut px = vec![[0.5f32; 4]; 1];
        let mut raw = ffi::RawTile { pixels: px.as_mut_ptr() as *mut f32, width: 1, height: 1, x: 0, y: 0 };
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let code = unsafe { (t.process)(inst, &mut raw, std::ptr::null(), 0) };
        std::panic::set_hook(prev);
        assert_eq!(code, ffi::PANICKED);
        // Poisoned: later calls do nothing.
        assert_eq!(unsafe { (t.process)(inst, &mut raw, std::ptr::null(), 0) }, ffi::PANICKED);
        unsafe { (t.destroy)(inst) };
    }

    #[test]
    fn the_guide_names_what_the_sdk_has() {
        for word in ["Filter", "Info::filter", "Param::number", "Param::choice", "export_plugins!", "premultiplied", "margin", "plugin.toml", ENTRY_SYMBOL, "cdylib", "plugin.build", "plugin.publishLocal"] {
            assert!(GUIDE.contains(word), "GUIDE.md doesn't mention {word}");
        }
    }
}
