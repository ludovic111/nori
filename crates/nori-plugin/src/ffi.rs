//! The C ABI between nori and a plugin library. Frozen: ABI 1's layouts below never change
//! (a compile-time check fails if they move); a later ABI adds a new symbol and table.
//!
//! A library exports `nori_plugin_entry() -> *const Entry`. The host checks
//! `Entry::abi_version` before calling anything, then reads each table's manifest (JSON) and
//! creates instances. Every call is guarded: a panic inside the plugin poisons that instance
//! (it answers [`PANICKED`] from then on) instead of unwinding into the host.

use std::ffi::c_void;
use std::panic::AssertUnwindSafe;

use crate::{ABI_VERSION, Filter, Tile};

/// `process` finished.
pub const OK: i32 = 0;
/// A bad argument (null pointers, a tile larger than its buffer).
pub const BAD_ARGUMENT: i32 = 1;
/// The plugin panicked (now or before): the host should switch it off.
pub const PANICKED: i32 = 2;

/// Pixels handed to `process`: `width × height` premultiplied RGBA `f32` (4 floats a pixel,
/// rows of `width`), at page position (`x`, `y`).
#[repr(C)]
#[derive(Debug)]
pub struct RawTile {
    pub pixels: *mut f32,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

/// One filter type.
#[repr(C)]
pub struct FilterVTable {
    /// Writes the manifest JSON into a buffer the host frees with `free_bytes`. Returns 0.
    pub manifest: unsafe extern "C" fn(out: *mut *mut u8, len: *mut usize) -> i32,
    pub create: unsafe extern "C" fn() -> *mut c_void,
    pub destroy: unsafe extern "C" fn(instance: *mut c_void),
    /// How many pixels around the changed area the filter reads.
    pub margin: unsafe extern "C" fn(instance: *mut c_void, values: *const f64, count: u32) -> u32,
    pub process: unsafe extern "C" fn(instance: *mut c_void, tile: *mut RawTile, values: *const f64, count: u32) -> i32,
    pub free_bytes: unsafe extern "C" fn(ptr: *mut u8, len: usize),
}
unsafe impl Sync for FilterVTable {}
unsafe impl Send for FilterVTable {}

/// The root of a plugin library.
#[repr(C)]
pub struct Entry {
    pub abi_version: u32,
    pub plugin_count: u32,
    pub plugin: unsafe extern "C" fn(index: u32) -> *const FilterVTable,
}
unsafe impl Sync for Entry {}
unsafe impl Send for Entry {}

/// The exported entry function's type.
pub type EntryFn = unsafe extern "C" fn() -> *const Entry;

// ABI 1 is frozen: a change that moves any of these fails to compile.
const _: () = {
    use std::mem::{offset_of, size_of};
    let p = size_of::<usize>();
    assert!(size_of::<FilterVTable>() == 6 * p);
    assert!(offset_of!(FilterVTable, process) == 4 * p);
    assert!(offset_of!(FilterVTable, free_bytes) == 5 * p);
    assert!(offset_of!(Entry, plugin_count) == 4);
    assert!(offset_of!(Entry, plugin) == 8);
    assert!(offset_of!(RawTile, width) == p);
    assert!(offset_of!(RawTile, y) == p + 12);
};

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn num(v: f64) -> String {
    if v.is_finite() { format!("{v}") } else { "0".into() }
}

/// The manifest of a filter type, as JSON (no serde: plugins stay light).
pub fn manifest_of<P: Filter>() -> String {
    let i = P::INFO;
    let params: Vec<String> = P::params()
        .iter()
        .map(|p| {
            format!(
                "{{\"name\":{},\"label\":{},\"kind\":{},\"min\":{},\"max\":{},\"default\":{},\"unit\":{},\"options\":[{}]}}",
                esc(p.name),
                esc(p.label),
                esc(p.kind),
                num(p.min),
                num(p.max),
                num(p.default),
                esc(p.unit),
                p.options.iter().map(|o| esc(o)).collect::<Vec<_>>().join(",")
            )
        })
        .collect();
    format!(
        "{{\"abi\":{ABI_VERSION},\"id\":{},\"name\":{},\"vendor\":{},\"version\":{},\"group\":{},\"description\":{},\"kind\":{},\"params\":[{}]}}",
        esc(i.id),
        esc(i.name),
        esc(i.vendor),
        esc(i.version),
        esc(i.group),
        esc(i.description),
        esc(i.kind.id()),
        params.join(",")
    )
}

unsafe fn give(bytes: Vec<u8>, out: *mut *mut u8, len: *mut usize) -> i32 {
    if out.is_null() || len.is_null() {
        return BAD_ARGUMENT;
    }
    let boxed = bytes.into_boxed_slice();
    // SAFETY: both pointers were checked for null; the host owns them for this call.
    unsafe {
        *len = boxed.len();
        *out = Box::into_raw(boxed) as *mut u8;
    }
    OK
}

unsafe extern "C" fn manifest<P: Filter>(out: *mut *mut u8, len: *mut usize) -> i32 {
    match std::panic::catch_unwind(manifest_of::<P>) {
        // SAFETY: forwarded from the host's call.
        Ok(json) => unsafe { give(json.into_bytes(), out, len) },
        Err(_) => PANICKED,
    }
}

struct Guarded<P> {
    plugin: P,
    poisoned: bool,
}

unsafe extern "C" fn create<P: Filter>() -> *mut c_void {
    match std::panic::catch_unwind(|| Box::new(Guarded { plugin: P::new(), poisoned: false })) {
        Ok(b) => Box::into_raw(b) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn destroy<P: Filter>(instance: *mut c_void) {
    if !instance.is_null() {
        // SAFETY: made by `create::<P>`.
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(instance as *mut Guarded<P>) })));
    }
}

unsafe fn values<'a>(ptr: *const f64, count: u32) -> &'a [f64] {
    // SAFETY: the host passes `count` values or a null pointer.
    if ptr.is_null() || count == 0 { &[] } else { unsafe { std::slice::from_raw_parts(ptr, count as usize) } }
}

/// The values, padded with each parameter's default (a host passing fewer gets defaults).
fn padded<P: Filter>(given: &[f64]) -> Vec<f64> {
    P::params().iter().enumerate().map(|(i, p)| given.get(i).copied().filter(|v| v.is_finite()).unwrap_or(p.default).clamp(p.min, p.max)).collect()
}

unsafe extern "C" fn margin<P: Filter>(instance: *mut c_void, v: *const f64, count: u32) -> u32 {
    if instance.is_null() {
        return 0;
    }
    // SAFETY: made by `create::<P>`; the values are the host's.
    let g = unsafe { &mut *(instance as *mut Guarded<P>) };
    if g.poisoned {
        return 0;
    }
    let vals = padded::<P>(unsafe { values(v, count) });
    std::panic::catch_unwind(AssertUnwindSafe(|| g.plugin.margin(&vals))).unwrap_or(0).min(4096)
}

unsafe extern "C" fn process<P: Filter>(instance: *mut c_void, tile: *mut RawTile, v: *const f64, count: u32) -> i32 {
    if instance.is_null() || tile.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: made by `create::<P>`; the tile and values are the host's for this call.
    let g = unsafe { &mut *(instance as *mut Guarded<P>) };
    if g.poisoned {
        return PANICKED;
    }
    let raw = unsafe { &*tile };
    if raw.pixels.is_null() && raw.width * raw.height > 0 {
        return BAD_ARGUMENT;
    }
    let n = raw.width as usize * raw.height as usize;
    let pixels: &mut [[f32; 4]] = if n == 0 { &mut [] } else { unsafe { std::slice::from_raw_parts_mut(raw.pixels as *mut [f32; 4], n) } };
    let vals = padded::<P>(unsafe { values(v, count) });
    let mut t = Tile { pixels, width: raw.width, height: raw.height, x: raw.x, y: raw.y };
    match std::panic::catch_unwind(AssertUnwindSafe(|| g.plugin.process(&mut t, &vals))) {
        Ok(()) => {
            // Keep what comes back valid premultiplied colour.
            for p in t.pixels.iter_mut() {
                let a = if p[3].is_finite() { p[3].clamp(0.0, 1.0) } else { 0.0 };
                p[3] = a;
                for c in p.iter_mut().take(3) {
                    *c = if c.is_finite() { c.clamp(0.0, a) } else { 0.0 };
                }
            }
            OK
        }
        Err(_) => {
            g.poisoned = true;
            PANICKED
        }
    }
}

unsafe extern "C" fn free_bytes(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        // SAFETY: made by `give`.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
    }
}

/// The vtable for one filter type (usable in `static` tables).
pub const fn vtable<P: Filter>() -> FilterVTable {
    FilterVTable { manifest: manifest::<P>, create: create::<P>, destroy: destroy::<P>, margin: margin::<P>, process: process::<P>, free_bytes }
}

/// Reads a table's manifest JSON (host side).
///
/// # Safety
/// `table` must come from a library exporting this ABI.
pub unsafe fn manifest_json(table: &FilterVTable) -> Result<String, String> {
    let mut ptr: *mut u8 = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: the table is the library's; the out pointers are ours.
    let code = unsafe { (table.manifest)(&mut ptr, &mut len) };
    if code != OK || ptr.is_null() {
        return Err(format!("the plugin's manifest isn't available (code {code})"));
    }
    // SAFETY: the library gave `len` bytes at `ptr`, released with its own `free_bytes`.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    unsafe { (table.free_bytes)(ptr, len) };
    String::from_utf8(bytes).map_err(|_| "the plugin's manifest isn't text".to_string())
}
