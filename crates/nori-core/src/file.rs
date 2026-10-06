//! The `.nori` file: a zip holding `document.json` and a PNG for each layer's pixels (see
//! `docs/FILE_FORMAT.md`).
//!
//! ```text
//! mimetype            application/x-nori (stored, first, like OpenRaster's)
//! document.json       the Document: canvas, layers (top first), settings
//! layers/<id>.png     RGBA pixels of each raster layer (its own size; placed at its x, y)
//! masks/<id>.png      each layer mask, grey, canvas size
//! selection.png       the selection, grey, canvas size (when there is one)
//! luts/<id>.cube      each Color Lookup adjustment's table
//! preview.png         the whole picture, at most 512 px, for file browsers and the home screen
//! ```

use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

use image::ImageEncoder;
use zip::write::SimpleFileOptions;

use crate::document::{Document, FORMAT};
use crate::layer::{Adjustment, Content};
use crate::raster::{Mask, Raster};

pub const MIMETYPE: &str = "application/x-nori";
pub const EXTENSION: &str = "nori";

/// Encodes straight RGBA rows as a PNG (fast compression: these are working files).
pub fn encode_png_rgba(width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive)
        .write_image(data, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn encode_png_grey(width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive)
        .write_image(data, width, height, image::ExtendedColorType::L8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// A picture to put in the file as `preview.png`: straight RGBA rows.
pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Writes `doc` to `path` (a temporary file first, then renamed over it).
pub fn save(doc: &Document, path: &Path, preview: Option<&Preview>) -> Result<(), String> {
    let bytes = to_bytes(doc, preview)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "document.nori".into());
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Couldn't write {}: {e}", path.display())
    })
}

/// The file's bytes.
pub fn to_bytes(doc: &Document, preview: Option<&Preview>) -> Result<Vec<u8>, String> {
    // Every picture is encoded first, in parallel (PNG is the slow part).
    let mut jobs: Vec<(String, Box<dyn Fn() -> Result<Vec<u8>, String> + Send + Sync + '_>)> = vec![];
    for l in doc.all() {
        if let Content::Raster { pixels, .. } = &l.content {
            jobs.push((format!("layers/{}.png", l.id), Box::new(move || encode_png_rgba(pixels.width(), pixels.height(), &pixels.to_vec()))));
        }
        if let Some(m) = &l.mask {
            let mask = &m.mask;
            jobs.push((format!("masks/{}.png", l.id), Box::new(move || encode_png_grey(mask.width(), mask.height(), &mask.to_vec()))));
        }
        if let Content::Adjustment { adjustment: Adjustment::Lut { table, size, name, .. } } = &l.content {
            let (table, size, name) = (table.clone(), *size, name.clone());
            jobs.push((format!("luts/{}.cube", l.id), Box::new(move || Ok(write_cube(&name, size, &table).into_bytes()))));
        }
    }
    if let Some(sel) = &doc.selection {
        jobs.push(("selection.png".into(), Box::new(move || encode_png_grey(sel.width(), sel.height(), &sel.to_vec()))));
    }
    if let Some(p) = preview {
        jobs.push(("preview.png".into(), Box::new(move || encode_png_rgba(p.width, p.height, &p.rgba))));
    }
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<parking::Slot> = (0..jobs.len()).map(|_| parking::Slot::default()).collect();
    std::thread::scope(|s| {
        for _ in 0..threads.min(jobs.len().max(1)) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((_, job)) = jobs.get(i) else { break };
                    results[i].set(job());
                }
            });
        }
    });

    let mut out = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let z = |e: zip::result::ZipError| e.to_string();
    zip.start_file("mimetype", stored).map_err(z)?;
    zip.write_all(MIMETYPE.as_bytes()).map_err(|e| e.to_string())?;
    let mut json_doc = doc.clone();
    json_doc.format = FORMAT;
    zip.start_file("document.json", deflated).map_err(z)?;
    zip.write_all(&serde_json::to_vec_pretty(&json_doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    for ((name, _), slot) in jobs.iter().zip(results) {
        let data = slot.take().ok_or_else(|| format!("{name} wasn't written"))??;
        let opts = if name.ends_with(".cube") { deflated } else { stored };
        zip.start_file(name.as_str(), opts).map_err(z)?;
        zip.write_all(&data).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(z)?;
    Ok(out.into_inner())
}

mod parking {
    /// A result one worker thread fills in.
    #[derive(Default)]
    pub struct Slot(std::sync::Mutex<Option<Result<Vec<u8>, String>>>);

    impl Slot {
        pub fn set(&self, v: Result<Vec<u8>, String>) {
            *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(v);
        }

        pub fn take(self) -> Option<Result<Vec<u8>, String>> {
            self.0.into_inner().unwrap_or_else(|e| e.into_inner())
        }
    }
}

/// Reads a `.nori` file.
pub fn open(path: &Path) -> Result<Document, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Couldn't open {}: {e}", path.display()))?;
    from_reader(file).map_err(|e| format!("{} isn't a readable nori file: {e}", path.display()))
}

pub fn from_bytes(bytes: &[u8]) -> Result<Document, String> {
    from_reader(Cursor::new(bytes))
}

fn read_entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<Option<Vec<u8>>, String> {
    let mut f = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut buf = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(Some(buf))
}

fn decode_rgba(bytes: &[u8], what: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|e| format!("{what}: {e}"))?.to_rgba8();
    Ok((img.width(), img.height(), img.into_raw()))
}

fn decode_grey(bytes: &[u8], what: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|e| format!("{what}: {e}"))?.to_luma8();
    Ok((img.width(), img.height(), img.into_raw()))
}

pub fn from_reader<R: Read + Seek>(reader: R) -> Result<Document, String> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    let json = read_entry(&mut zip, "document.json")?.ok_or("there is no document.json in it")?;
    let mut doc: Document = serde_json::from_slice(&json).map_err(|e| format!("document.json: {e}"))?;
    if doc.format > FORMAT {
        return Err(format!("it was written by a newer nori (format {}); update nori to open it", doc.format));
    }
    if doc.pages.is_empty() {
        return Err("it has no pages".into());
    }
    for p in doc.pages.iter().chain(doc.masters.iter()) {
        if p.width == 0 || p.height == 0 || p.width > crate::MAX_SIDE || p.height > crate::MAX_SIDE {
            return Err(format!("a page of {}×{} is out of range", p.width, p.height));
        }
    }
    let ids: Vec<String> = doc.all().iter().map(|l| l.id.clone()).collect();
    for id in ids {
        let raster = read_entry(&mut zip, &format!("layers/{id}.png"))?;
        let mask = read_entry(&mut zip, &format!("masks/{id}.png"))?;
        let cube = read_entry(&mut zip, &format!("luts/{id}.cube"))?;
        let (cw, ch) = doc.page_of(&id).map(|p| (p.width, p.height)).unwrap_or((0, 0));
        let layer = doc.layer_mut(&id).ok_or("layer vanished")?;
        if let Content::Raster { pixels, .. } = &mut layer.content {
            if let Some(png) = raster {
                let (w, h, data) = decode_rgba(&png, &format!("layers/{id}.png"))?;
                if w != pixels.width() || h != pixels.height() {
                    return Err(format!("layers/{id}.png is {w}×{h}, document.json says {}×{}", pixels.width(), pixels.height()));
                }
                *pixels = Raster::from_rgba(w, h, &data);
            }
        }
        if let (Some(m), Some(png)) = (&mut layer.mask, mask) {
            let (w, h, data) = decode_grey(&png, &format!("masks/{id}.png"))?;
            if w != cw || h != ch {
                return Err(format!("masks/{id}.png is {w}×{h}, the canvas is {cw}×{ch}"));
            }
            let fill = m.mask.fill();
            m.mask = Mask::from_vec(w, h, fill, &data);
        }
        if let (Content::Adjustment { adjustment: Adjustment::Lut { table, size, .. } }, Some(text)) = (&mut layer.content, cube) {
            let (s, t) = parse_cube(&String::from_utf8_lossy(&text))?;
            *size = s;
            *table = Arc::new(t);
        }
    }
    let (pw, ph) = (doc.width(), doc.height());
    if let Some(sel) = &mut doc.selection
        && let Some(png) = read_entry(&mut zip, "selection.png")?
    {
        let (w, h, data) = decode_grey(&png, "selection.png")?;
        if w == pw && h == ph {
            let fill = sel.fill();
            *sel = Mask::from_vec(w, h, fill, &data);
        }
    }
    doc.format = FORMAT;
    Ok(doc)
}

/// The preview picture stored in a file, if it has one (RGBA, its size).
pub fn read_preview(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let png = read_entry(&mut zip, "preview.png").ok()??;
    decode_rgba(&png, "preview.png").ok()
}

/// Reads an Adobe / Resolve `.cube` 3D LUT: its size and `size³` RGB triples (red fastest).
pub fn parse_cube(text: &str) -> Result<(u32, Vec<[f32; 3]>), String> {
    let mut size = 0u32;
    let (mut min, mut max) = ([0f32; 3], [1f32; 3]);
    let mut table = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let first = parts.next().unwrap_or("");
        match first.to_ascii_uppercase().as_str() {
            "TITLE" => {}
            "LUT_3D_SIZE" => size = parts.next().and_then(|s| s.parse().ok()).ok_or("LUT_3D_SIZE needs a number")?,
            "LUT_1D_SIZE" => return Err("1D LUTs (LUT_1D_SIZE) aren't supported; use a 3D .cube".into()),
            "DOMAIN_MIN" => {
                let v: Vec<f32> = parts.filter_map(|s| s.parse().ok()).collect();
                if v.len() == 3 {
                    min = [v[0], v[1], v[2]];
                }
            }
            "DOMAIN_MAX" => {
                let v: Vec<f32> = parts.filter_map(|s| s.parse().ok()).collect();
                if v.len() == 3 {
                    max = [v[0], v[1], v[2]];
                }
            }
            _ => {
                let r: f32 = first.parse().map_err(|_| format!("unexpected line `{line}`"))?;
                let g: f32 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| format!("unexpected line `{line}`"))?;
                let b: f32 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| format!("unexpected line `{line}`"))?;
                let n = |v: f32, i: usize| (v - min[i]) / (max[i] - min[i]).max(1e-6);
                table.push([n(r, 0), n(g, 1), n(b, 2)]);
            }
        }
    }
    if !(2..=256).contains(&size) {
        return Err(format!("LUT_3D_SIZE {size} is out of range (2–256)"));
    }
    let want = (size * size * size) as usize;
    if table.len() != want {
        return Err(format!("a {size}³ LUT needs {want} entries, the file has {}", table.len()));
    }
    Ok((size, table))
}

/// Writes a `.cube` file.
pub fn write_cube(title: &str, size: u32, table: &[[f32; 3]]) -> String {
    let mut s = format!("TITLE \"{}\"\nLUT_3D_SIZE {size}\n", title.replace('"', "'"));
    for [r, g, b] in table {
        s.push_str(&format!("{r:.6} {g:.6} {b:.6}\n"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::layer::{Layer, LayerMask, TextLayer};

    #[test]
    fn round_trips_layers_masks_selection_and_luts() {
        let mut d = Document::new("Trip", 300, 200, Some(Color::WHITE));
        let id = d.new_id();
        let mut px = Raster::transparent(50, 40);
        px.set(3, 4, [255, 0, 0, 200]);
        let mut l = Layer::new(id.clone(), "Red dot", Content::Raster { x: 10, y: 20, pixels: px });
        let mut m = Mask::filled(300, 200, 255);
        m.set(5, 5, [0]);
        l.mask = Some(LayerMask { mask: m, enabled: true });
        d.insert_above(l, None);
        let tid = d.new_id();
        d.insert_above(Layer::new(tid, "Title", Content::Text { text: TextLayer { text: "Hello".into(), ..Default::default() } }), None);
        let lid = d.new_id();
        let table: Vec<[f32; 3]> = (0..8).map(|i| [(i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32]).collect();
        d.insert_above(
            Layer::new(lid.clone(), "Look", Content::Adjustment { adjustment: Adjustment::Lut { name: "id".into(), size: 2, amount: 1.0, table: Arc::new(table.clone()) } }),
            None,
        );
        let mut sel = Mask::filled(300, 200, 0);
        sel.set(1, 1, [255]);
        d.selection = Some(sel);

        let bytes = to_bytes(&d, Some(&Preview { width: 2, height: 1, rgba: vec![0; 8] })).unwrap();
        let back = from_bytes(&bytes).unwrap();
        assert_eq!(back.all().len(), 4);
        let l = back.layer(&id).unwrap();
        assert_eq!(l.raster().unwrap().2.get(3, 4), [255, 0, 0, 200]);
        assert_eq!(l.raster_bounds().unwrap().x, 10);
        assert_eq!(l.mask.as_ref().unwrap().mask.get(5, 5), [0]);
        assert_eq!(l.mask.as_ref().unwrap().mask.get(6, 5), [255]);
        assert_eq!(back.selection.as_ref().unwrap().get(1, 1), [255]);
        assert_eq!(back.selection.as_ref().unwrap().get(2, 1), [0]);
        match &back.layer(&lid).unwrap().content {
            Content::Adjustment { adjustment: Adjustment::Lut { table: t, size, .. } } => {
                assert_eq!(*size, 2);
                assert_eq!(t.as_slice(), table.as_slice());
            }
            _ => panic!("not a LUT"),
        }
        let bg = back.page().layers.last().unwrap();
        assert_eq!(bg.raster().unwrap().2.get(299, 199), [255, 255, 255, 255]);
    }

    #[test]
    fn cube_files_parse_and_refuse_bad_ones() {
        let text = "TITLE \"x\"\n# c\nLUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
        let (s, t) = parse_cube(text).unwrap();
        assert_eq!(s, 2);
        assert_eq!(t[1], [1.0, 0.0, 0.0]);
        assert!(parse_cube("LUT_3D_SIZE 2\n0 0 0\n").unwrap_err().contains("needs 8"));
        assert!(parse_cube("LUT_1D_SIZE 2\n").is_err());
        let again = write_cube("x", s, &t);
        assert_eq!(parse_cube(&again).unwrap().1, t);
    }

    #[test]
    fn refuses_what_isnt_a_nori_file() {
        assert!(from_bytes(b"not a zip").is_err());
    }
}
