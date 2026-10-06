//! nori-io: files in and out.
//!
//! * Open: `.nori`; pictures (PNG, JPEG, WebP, TIFF, BMP, GIF); Photoshop `.psd` with its
//!   layers; OpenRaster `.ora` (Krita, GIMP, MyPaint) with its layers; SVG as vector layers.
//! * Place: a picture or an SVG into the open document, as a layer.
//! * Export: PNG, JPEG, WebP, TIFF, BMP (the page drawn, at any scale), OpenRaster (layers),
//!   SVG (vectors), PDF (every page, vectors and text kept as vectors).
//! * Assets: GIMP brushes (`.gbr`), Adobe swatches (`.ase`), `.cube` LUTs (in nori-core).
//! * [`APPS`]: the editors people come from, and which of their files nori really opens.

pub mod assets;
pub mod ora;
pub mod pdf;
pub mod plan;
pub mod psd;
pub mod svg;

use std::path::Path;

use nori_core::{Color, Content, Document, Layer, Raster};
use serde::Serialize;

/// A file format nori knows.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub open: bool,
    pub export: bool,
    /// What survives: "layers", "vectors", "pixels"…
    pub keeps: &'static str,
}

pub static FORMATS: &[Format] = &[
    Format { id: "nori", name: "nori document", extensions: &["nori"], open: true, export: true, keeps: "everything: pages, layers, vectors, text, styles, masks, history-free" },
    Format { id: "png", name: "PNG", extensions: &["png"], open: true, export: true, keeps: "pixels with transparency" },
    Format { id: "jpeg", name: "JPEG", extensions: &["jpg", "jpeg"], open: true, export: true, keeps: "pixels (no transparency; quality setting)" },
    Format { id: "webp", name: "WebP", extensions: &["webp"], open: true, export: true, keeps: "pixels with transparency (lossless)" },
    Format { id: "tiff", name: "TIFF", extensions: &["tif", "tiff"], open: true, export: true, keeps: "pixels with transparency" },
    Format { id: "bmp", name: "BMP", extensions: &["bmp"], open: true, export: true, keeps: "pixels" },
    Format { id: "gif", name: "GIF", extensions: &["gif"], open: true, export: false, keeps: "pixels (first frame)" },
    Format { id: "psd", name: "Photoshop document", extensions: &["psd"], open: true, export: false, keeps: "layers (pixels, positions, opacity, blend modes, groups, clipping); text and smart objects as their pixels" },
    Format { id: "ora", name: "OpenRaster", extensions: &["ora"], open: true, export: true, keeps: "layers (pixels, positions, opacity, blend modes, groups)" },
    Format { id: "svg", name: "SVG", extensions: &["svg", "svgz"], open: true, export: true, keeps: "vectors (paths, fills, strokes, gradients, groups); text as outlines" },
    Format { id: "pdf", name: "PDF", extensions: &["pdf"], open: false, export: true, keeps: "every page: vectors, text as outlines, pixels as images" },
];

pub fn format_of(path: &Path) -> Option<&'static Format> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    FORMATS.iter().find(|f| f.extensions.contains(&ext.as_str()))
}

pub fn format(id: &str) -> Option<&'static Format> {
    let id = id.trim().trim_start_matches('.').to_ascii_lowercase();
    FORMATS.iter().find(|f| f.id == id || f.extensions.contains(&id.as_str()))
}

/// An editor people come from: its real logo (`logo` id in the window's logos) and which of its
/// files nori opens.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
    pub maker: &'static str,
    /// `photo`, `vector` or `layout`.
    pub kind: &'static str,
    /// Format ids nori opens from it.
    pub formats: &'static [&'static str],
    /// How to bring work over, in one sentence.
    pub how: &'static str,
}

pub static APPS: &[App] = &[
    App { id: "photoshop", name: "Photoshop", maker: "Adobe", kind: "photo", formats: &["psd"], how: "Open the .psd: layers, groups, blend modes and opacity come with it." },
    App { id: "gimp", name: "GIMP", maker: "The GIMP team", kind: "photo", formats: &["ora", "psd"], how: "In GIMP, File › Export As… OpenRaster (.ora) or Photoshop (.psd), then open it here with its layers." },
    App { id: "affinityphoto", name: "Affinity Photo", maker: "Serif (Canva)", kind: "photo", formats: &["psd"], how: "In Affinity Photo, File › Export › PSD, then open it here with its layers." },
    App { id: "pixelmator", name: "Pixelmator Pro", maker: "Pixelmator Team (Apple)", kind: "photo", formats: &["psd"], how: "In Pixelmator Pro, File › Export › Photoshop, then open it here with its layers." },
    App { id: "krita", name: "Krita", maker: "KDE", kind: "photo", formats: &["ora", "psd"], how: "In Krita, File › Export… OpenRaster (.ora), then open it here with its layers." },
    App { id: "photopea", name: "Photopea", maker: "Ivan Kutskir", kind: "photo", formats: &["psd"], how: "In Photopea, File › Save as PSD, then open it here with its layers." },
    App { id: "illustrator", name: "Illustrator", maker: "Adobe", kind: "vector", formats: &["svg"], how: "In Illustrator, File › Export › Export As… SVG, then open it here: paths, fills, strokes and gradients stay editable." },
    App { id: "inkscape", name: "Inkscape", maker: "Inkscape Project", kind: "vector", formats: &["svg"], how: "Open Inkscape's .svg here: paths, groups, fills, strokes and gradients stay editable." },
    App { id: "figma", name: "Figma", maker: "Figma", kind: "vector", formats: &["svg"], how: "In Figma, select a frame, Export › SVG, then open it here as vector layers." },
    App { id: "affinitydesigner", name: "Affinity Designer", maker: "Serif (Canva)", kind: "vector", formats: &["svg", "psd"], how: "In Affinity Designer, File › Export › SVG, then open it here as vector layers." },
    App { id: "canva", name: "Canva", maker: "Canva", kind: "layout", formats: &["svg"], how: "In Canva, Share › Download › SVG, then open it here as vector layers." },
    App { id: "affinitypublisher", name: "Affinity Publisher", maker: "Serif (Canva)", kind: "layout", formats: &["svg"], how: "In Affinity Publisher, File › Export › SVG (one page at a time), then open it here or place it on a page." },
    App { id: "scribus", name: "Scribus", maker: "The Scribus team", kind: "layout", formats: &["svg"], how: "In Scribus, File › Export › Save as SVG, then open it here or place it on a page." },
];

pub fn app(id: &str) -> Option<&'static App> {
    let id: String = id.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    APPS.iter().find(|a| a.id == id || a.name.to_ascii_lowercase().replace(' ', "") == id)
}

fn name_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into())
}

/// Decodes a picture file into straight RGBA (EXIF orientation applied for JPEGs).
pub fn decode_picture(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    decoder.no_limits();
    let img = decoder.decode().map_err(|e| e.to_string())?;
    // Turned as the camera says.
    let orientation = exif_orientation(bytes);
    let img = match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    };
    let rgba = img.to_rgba8();
    if rgba.width() > nori_core::MAX_SIDE || rgba.height() > nori_core::MAX_SIDE {
        return Err(format!("a {}×{} picture is larger than nori's {} px", rgba.width(), rgba.height(), nori_core::MAX_SIDE));
    }
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// The EXIF orientation of a JPEG (1 when there is none).
fn exif_orientation(b: &[u8]) -> u16 {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return 1;
    }
    let mut i = 2;
    while i + 4 < b.len() {
        if b[i] != 0xFF {
            return 1;
        }
        let marker = b[i + 1];
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if marker == 0xE1 && b.get(i + 4..i + 10) == Some(b"Exif\0\0") {
            let t = i + 10;
            let le = b.get(t..t + 2) == Some(b"II");
            let r16 = |o: usize| -> Option<u16> { let s = b.get(t + o..t + o + 2)?; Some(if le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) }) };
            let r32 = |o: usize| -> Option<u32> { let s = b.get(t + o..t + o + 4)?; Some(if le { u32::from_le_bytes([s[0], s[1], s[2], s[3]]) } else { u32::from_be_bytes([s[0], s[1], s[2], s[3]]) }) };
            let Some(ifd) = r32(4) else { return 1 };
            let n = r16(ifd as usize).unwrap_or(0);
            for e in 0..n as usize {
                let o = ifd as usize + 2 + e * 12;
                if r16(o) == Some(0x0112) {
                    return r16(o + 8).unwrap_or(1);
                }
            }
            return 1;
        }
        if marker == 0xDA {
            return 1;
        }
        i += 2 + len;
    }
    1
}

/// Opens any file nori reads as a document.
pub fn open(path: &Path) -> Result<Document, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let name = name_of(path);
    let fmt = format_of(path).map(|f| f.id).unwrap_or("");
    let res = match fmt {
        "nori" => nori_core::file::from_bytes(&bytes),
        "psd" => psd::read(&bytes, &name),
        "ora" => ora::read(&bytes, &name),
        "svg" => svg::read(&bytes, &name),
        "pdf" => Err("nori writes PDF but doesn't open it yet.".to_string()),
        _ => {
            // Sniff: a nori file (zip with document.json), else a picture.
            if bytes.starts_with(b"PK") {
                nori_core::file::from_bytes(&bytes).or_else(|_| ora::read(&bytes, &name))
            } else if bytes.starts_with(b"8BPS") {
                psd::read(&bytes, &name)
            } else {
                decode_picture(&bytes).map(|(w, h, rgba)| Document::from_rgba(name.clone(), w, h, &rgba))
            }
        }
    };
    res.map_err(|e| format!("Couldn't open {}: {e}", path.display()))
}

/// Puts a file on the active page as a new layer above the active one: a picture as pixels
/// (centred), an SVG as a group of vector layers, a nori/PSD/ORA file as a group of its
/// layers. Returns the new layer's id.
pub fn place(doc: &mut Document, path: &Path, at: Option<(i32, i32)>) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let name = name_of(path);
    match format_of(path).map(|f| f.id) {
        Some("svg") => svg::place(doc, &bytes, &name),
        Some("psd" | "ora" | "nori") => {
            let other = open(path)?;
            let mut layers = other.pages.into_iter().next().map(|p| p.layers).unwrap_or_default();
            fn renumber(doc: &mut Document, list: &mut [Layer]) {
                for l in list {
                    l.id = doc.new_id();
                    if let Some(c) = l.children_mut() {
                        renumber(doc, c);
                    }
                }
            }
            renumber(doc, &mut layers);
            let id = doc.new_id();
            let mut g = Layer::new(id.clone(), name, Content::Group { children: layers, expanded: false });
            g.blend = nori_core::BlendMode::PassThrough;
            let above = doc.active_id();
            doc.insert_above(g, above.as_deref());
            Ok(id)
        }
        _ => {
            let (w, h, rgba) = decode_picture(&bytes)?;
            let (x, y) = at.unwrap_or(((doc.width() as i32 - w as i32) / 2, (doc.height() as i32 - h as i32) / 2));
            let id = doc.new_id();
            let above = doc.active_id();
            doc.insert_above(Layer::new(id.clone(), name, Content::Raster { x, y, pixels: Raster::from_rgba(w, h, &rgba) }), above.as_deref());
            Ok(id)
        }
    }
}

/// How to export.
#[derive(Clone, Debug)]
pub struct ExportOptions {
    /// A format id (`png`, `jpeg`, `webp`, `tiff`, `bmp`, `ora`, `svg`, `pdf`, `nori`); none:
    /// from the file's extension.
    pub format: Option<String>,
    /// JPEG quality, 1–100.
    pub quality: u8,
    /// Size multiplier for pictures (2: twice the pixels; vectors and text stay sharp).
    pub scale: f32,
    /// The page (index) for single-page formats; none: the active page.
    pub page: Option<usize>,
    /// For PDF: which pages (none: all).
    pub pages: Option<Vec<usize>>,
    /// Under transparent pixels for formats without transparency (JPEG, BMP); white by default.
    pub matte: Color,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { format: None, quality: 90, scale: 1.0, page: None, pages: None, matte: Color::WHITE }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub path: String,
    pub format: &'static str,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub pages: usize,
}

/// The page drawn as straight RGBA at `scale`.
pub fn render_page(doc: &Document, page: usize, scale: f32) -> (u32, u32, Vec<u8>) {
    let page = page.min(doc.pages.len().saturating_sub(1));
    if (scale - 1.0).abs() < 1e-4 {
        let p = &doc.pages[page];
        return (p.width, p.height, nori_render::flatten_page(doc, p));
    }
    let scaled = nori_render::transform::scale_document(doc, scale, nori_render::transform::Filter::Bicubic);
    let p = &scaled.pages[page];
    (p.width, p.height, nori_render::flatten_page(&scaled, p))
}

/// Writes the document (or a page of it) to `path`.
pub fn export(doc: &Document, path: &Path, opts: &ExportOptions) -> Result<Exported, String> {
    let fmt = match &opts.format {
        Some(f) => format(f).ok_or_else(|| format!("nori doesn't write `{f}`. Formats: {}.", FORMATS.iter().filter(|f| f.export).map(|f| f.id).collect::<Vec<_>>().join(", ")))?,
        None => format_of(path).ok_or_else(|| format!("{} has no extension nori writes; give a format.", path.display()))?,
    };
    if !fmt.export {
        return Err(format!("nori opens {} but doesn't write it.", fmt.name));
    }
    let page = opts.page.unwrap_or_else(|| doc.active_index()).min(doc.pages.len().saturating_sub(1));
    let (bytes, w, h, pages): (Vec<u8>, u32, u32, usize) = match fmt.id {
        "nori" => {
            let (pw, ph, rgba) = render_page(doc, doc.active_index(), 1.0);
            let (tw, th, thumb) = nori_render::transform::fit_rgba(&rgba, pw, ph, 512);
            (nori_core::file::to_bytes(doc, Some(&nori_core::file::Preview { width: tw, height: th, rgba: thumb }))?, pw, ph, doc.pages.len())
        }
        "svg" => {
            let p = &doc.pages[page];
            (svg::write(doc, Some(p))?.into_bytes(), p.width, p.height, 1)
        }
        "pdf" => {
            let list = opts.pages.clone();
            let n = list.as_ref().map_or(doc.pages.len(), Vec::len);
            let p = &doc.pages[page];
            (pdf::write(doc, list.as_deref())?, p.width, p.height, n)
        }
        "ora" => {
            let mut d = doc.clone();
            d.active_page = d.pages[page].id.clone();
            let p = &doc.pages[page];
            (ora::write(&d)?, p.width, p.height, 1)
        }
        id => {
            let (w, h, mut rgba) = render_page(doc, page, opts.scale);
            let mut out = Vec::new();
            let cur = std::io::Cursor::new(&mut out);
            use image::ImageEncoder;
            let flat = |rgba: &mut Vec<u8>| -> Vec<u8> {
                let m = opts.matte.to_u8();
                rgba.chunks_exact(4)
                    .flat_map(|p| {
                        let a = p[3] as u32;
                        [0, 1, 2].map(|c| ((p[c] as u32 * a + m[c] as u32 * (255 - a) + 127) / 255) as u8)
                    })
                    .collect()
            };
            match id {
                "png" => image::codecs::png::PngEncoder::new_with_quality(cur, image::codecs::png::CompressionType::Default, image::codecs::png::FilterType::Adaptive)
                    .write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)
                    .map_err(|e| e.to_string())?,
                "jpeg" => image::codecs::jpeg::JpegEncoder::new_with_quality(cur, opts.quality.clamp(1, 100)).write_image(&flat(&mut rgba), w, h, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?,
                "webp" => image::codecs::webp::WebPEncoder::new_lossless(cur).write_image(&rgba, w, h, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?,
                "tiff" => image::codecs::tiff::TiffEncoder::new(cur).write_image(&rgba, w, h, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?,
                "bmp" => image::codecs::bmp::BmpEncoder::new(&mut std::io::Cursor::new(&mut out)).write_image(&flat(&mut rgba), w, h, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?,
                other => return Err(format!("nori doesn't write `{other}`.")),
            }
            (out, w, h, 1)
        }
    };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("nori-export.tmp");
    std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Couldn't write {}: {e}", path.display())
    })?;
    Ok(Exported { path: path.display().to_string(), format: fmt.id, width: w, height: h, bytes: bytes.len() as u64, pages })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_and_reopens_every_picture_format() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Document::new("t", 40, 30, Some(Color::rgb(1.0, 0.5, 0.0)));
        let id = d.new_id();
        d.insert_above(
            Layer::new(id, "Box", Content::Vector { shape: nori_core::Shape::new(nori_core::Geometry::Rect { x: 5.0, y: 5.0, w: 10.0, h: 10.0, radius: 0.0 }, nori_core::Paint::solid(Color::BLACK), None) }),
            None,
        );
        for ext in ["png", "jpg", "webp", "tiff", "bmp", "ora", "svg", "nori"] {
            let p = dir.path().join(format!("out.{ext}"));
            let r = export(&d, &p, &ExportOptions::default()).unwrap();
            assert!(r.bytes > 0, "{ext}");
            let back = open(&p).unwrap_or_else(|e| panic!("{ext}: {e}"));
            assert_eq!((back.width(), back.height()), (40, 30), "{ext}");
            let px = nori_render::flatten(&back);
            let at = |x: u32, y: u32| px[((y * 40 + x) * 4) as usize];
            assert!(at(8, 8) < 60, "{ext}: the box is dark");
            assert!(at(30, 25) > 200, "{ext}: the background is orange");
        }
        let r = export(&d, &dir.path().join("big.png"), &ExportOptions { scale: 2.0, ..Default::default() }).unwrap();
        assert_eq!((r.width, r.height), (80, 60));
        let pdf = export(&d, &dir.path().join("out.pdf"), &ExportOptions::default()).unwrap();
        assert_eq!(pdf.format, "pdf");
        assert!(export(&d, &dir.path().join("x.gif"), &ExportOptions::default()).is_err());
        assert!(open(&dir.path().join("out.pdf")).is_err());
    }

    #[test]
    fn apps_only_claim_formats_nori_opens() {
        for a in APPS {
            assert!(!a.formats.is_empty());
            for f in a.formats {
                assert!(format(f).is_some_and(|f| f.open), "{} claims {f}", a.id);
            }
        }
        assert_eq!(app("Affinity Photo").unwrap().id, "affinityphoto");
    }

    #[test]
    fn places_pictures_as_layers() {
        let dir = tempfile::tempdir().unwrap();
        let mut src = Document::new("s", 10, 10, Some(Color::rgb(0.0, 0.0, 1.0)));
        src.name = "s".into();
        let p = dir.path().join("blue.png");
        export(&src, &p, &ExportOptions::default()).unwrap();
        let mut d = Document::new("t", 100, 100, Some(Color::WHITE));
        let id = place(&mut d, &p, None).unwrap();
        let l = d.layer(&id).unwrap();
        assert_eq!(l.raster_bounds().unwrap(), nori_core::Rect::new(45, 45, 10, 10));
        assert_eq!(l.name, "blue");
    }
}
