//! OpenRaster (`.ora`), the layered format Krita, GIMP and MyPaint read and write: a zip with
//! `stack.xml` (the layer tree, first child on top) and a PNG per layer.

use std::io::{Cursor, Read, Write};

use nori_core::{BlendMode, Content, Document, Layer, Raster};
use quick_xml::events::{BytesStart, Event};
use zip::write::SimpleFileOptions;

fn op_to_blend(op: &str) -> BlendMode {
    match op.trim_start_matches("svg:").trim_start_matches("krita:") {
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "overlay" => BlendMode::Overlay,
        "darken" => BlendMode::Darken,
        "lighten" => BlendMode::Lighten,
        "color-dodge" => BlendMode::ColorDodge,
        "color-burn" => BlendMode::ColorBurn,
        "hard-light" => BlendMode::HardLight,
        "soft-light" => BlendMode::SoftLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "hue" => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color" => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        "plus" | "linear-dodge" => BlendMode::LinearDodge,
        _ => BlendMode::Normal,
    }
}

fn blend_to_op(b: BlendMode) -> &'static str {
    match b {
        BlendMode::Multiply => "svg:multiply",
        BlendMode::Screen => "svg:screen",
        BlendMode::Overlay => "svg:overlay",
        BlendMode::Darken => "svg:darken",
        BlendMode::Lighten => "svg:lighten",
        BlendMode::ColorDodge => "svg:color-dodge",
        BlendMode::ColorBurn => "svg:color-burn",
        BlendMode::HardLight => "svg:hard-light",
        BlendMode::SoftLight => "svg:soft-light",
        BlendMode::Difference => "svg:difference",
        BlendMode::Exclusion => "svg:exclusion",
        BlendMode::Hue => "svg:hue",
        BlendMode::Saturation => "svg:saturation",
        BlendMode::Color => "svg:color",
        BlendMode::Luminosity => "svg:luminosity",
        BlendMode::LinearDodge => "svg:plus",
        _ => "svg:src-over",
    }
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == name.as_bytes()).and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

/// Reads an OpenRaster file.
pub fn read(bytes: &[u8], name: &str) -> Result<Document, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not an OpenRaster file: {e}"))?;
    let mut xml = String::new();
    zip.by_name("stack.xml").map_err(|_| "no stack.xml in it".to_string())?.read_to_string(&mut xml).map_err(|e| e.to_string())?;
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut doc: Option<Document> = None;
    // Open stacks: the children read so far (top first).
    let mut stacks: Vec<(Option<Layer>, Vec<Layer>)> = vec![];
    loop {
        let ev = reader.read_event().map_err(|e| format!("stack.xml: {e}"))?;
        match ev {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"image" => {
                let w: u32 = attr(&e, "w").and_then(|v| v.parse().ok()).ok_or("stack.xml: the image has no width")?;
                let h: u32 = attr(&e, "h").and_then(|v| v.parse().ok()).ok_or("stack.xml: the image has no height")?;
                if w == 0 || h == 0 || w > nori_core::MAX_SIDE || h > nori_core::MAX_SIDE {
                    return Err(format!("an image of {w}×{h} is out of range"));
                }
                doc = Some(Document::empty(name, w, h));
            }
            Event::Start(e) if e.name().as_ref() == b"stack" => {
                let d = doc.as_mut().ok_or("stack.xml: a stack before the image")?;
                // The root stack has no layer of its own.
                let group = if stacks.is_empty() {
                    None
                } else {
                    let id = d.new_id();
                    let mut g = Layer::new(id, attr(&e, "name").unwrap_or_else(|| "Group".into()), Content::Group { children: vec![], expanded: true });
                    g.opacity = attr(&e, "opacity").and_then(|v| v.parse().ok()).unwrap_or(1.0);
                    g.visible = attr(&e, "visibility").is_none_or(|v| v != "hidden");
                    g.blend = if attr(&e, "isolation").as_deref() == Some("isolate") { op_to_blend(&attr(&e, "composite-op").unwrap_or_default()) } else { BlendMode::PassThrough };
                    Some(g)
                };
                stacks.push((group, vec![]));
            }
            Event::End(e) if e.name().as_ref() == b"stack" => {
                let (group, children) = stacks.pop().ok_or("stack.xml: unbalanced stacks")?;
                match (group, stacks.last_mut()) {
                    (Some(mut g), Some((_, list))) => {
                        if let Content::Group { children: c, .. } = &mut g.content {
                            *c = children;
                        }
                        list.push(g);
                    }
                    (None, _) => {
                        let d = doc.as_mut().ok_or("stack.xml: no image")?;
                        d.pages[0].layers = children;
                    }
                    _ => {}
                }
            }
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"layer" => {
                let d = doc.as_mut().ok_or("stack.xml: a layer before the image")?;
                let src = attr(&e, "src").ok_or("stack.xml: a layer without src")?;
                let mut png = vec![];
                zip.by_name(&src).map_err(|_| format!("{src} is missing"))?.read_to_end(&mut png).map_err(|e| e.to_string())?;
                let img = image::load_from_memory(&png).map_err(|e| format!("{src}: {e}"))?.to_rgba8();
                let id = d.new_id();
                let x = attr(&e, "x").and_then(|v| v.parse().ok()).unwrap_or(0);
                let y = attr(&e, "y").and_then(|v| v.parse().ok()).unwrap_or(0);
                let mut l = Layer::new(id, attr(&e, "name").unwrap_or_else(|| "Layer".into()), Content::Raster { x, y, pixels: Raster::from_rgba(img.width(), img.height(), img.as_raw()) });
                l.opacity = attr(&e, "opacity").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.0, 1.0);
                l.visible = attr(&e, "visibility").is_none_or(|v| v != "hidden");
                l.blend = op_to_blend(&attr(&e, "composite-op").unwrap_or_default());
                if let Some((_, list)) = stacks.last_mut() {
                    list.push(l);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut d = doc.ok_or("stack.xml has no image")?;
    d.active = d.pages[0].layers.first().map(|l| l.id.clone());
    Ok(d)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Writes the active page as OpenRaster: raster layers as they are; text, vectors, fills and
/// adjustment layers as pixels (other apps don't share nori's), groups as stacks.
pub fn write(doc: &Document) -> Result<Vec<u8>, String> {
    let page = doc.page();
    let mut files: Vec<(String, Vec<u8>)> = vec![];
    let mut xml = format!("<?xml version='1.0' encoding='UTF-8'?>\n<image version=\"0.0.5\" w=\"{}\" h=\"{}\" xres=\"{}\" yres=\"{}\">\n<stack>\n", page.width, page.height, doc.dpi, doc.dpi);
    fn walk(doc: &nori_core::Document, list: &[Layer], xml: &mut String, files: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
        for l in list {
            let vis = if l.visible { "visible" } else { "hidden" };
            match &l.content {
                Content::Group { children, .. } => {
                    let iso = if l.blend == BlendMode::PassThrough { "auto" } else { "isolate" };
                    xml.push_str(&format!("<stack name=\"{}\" opacity=\"{:.3}\" visibility=\"{vis}\" isolation=\"{iso}\" composite-op=\"{}\">\n", esc(&l.name), l.opacity, blend_to_op(l.blend)));
                    walk(doc, children, xml, files)?;
                    xml.push_str("</stack>\n");
                }
                Content::Raster { x, y, pixels } if l.mask.is_none() => {
                    let src = format!("data/{}.png", l.id);
                    files.push((src.clone(), nori_core::file::encode_png_rgba(pixels.width(), pixels.height(), &pixels.to_vec())?));
                    xml.push_str(&format!("<layer name=\"{}\" src=\"{src}\" x=\"{x}\" y=\"{y}\" opacity=\"{:.3}\" visibility=\"{vis}\" composite-op=\"{}\"/>\n", esc(&l.name), l.opacity, blend_to_op(l.blend)));
                }
                _ => {
                    // Drawn on its own, at full opacity (the opacity stays an attribute).
                    let mut solo = l.clone();
                    solo.opacity = 1.0;
                    solo.visible = true;
                    solo.blend = BlendMode::Normal;
                    let mut p = doc.page().clone();
                    p.master = None;
                    p.layers = vec![solo];
                    let rgba = nori_render::flatten_page(doc, &p);
                    let src = format!("data/{}.png", l.id);
                    files.push((src.clone(), nori_core::file::encode_png_rgba(p.width, p.height, &rgba)?));
                    xml.push_str(&format!("<layer name=\"{}\" src=\"{src}\" x=\"0\" y=\"0\" opacity=\"{:.3}\" visibility=\"{vis}\" composite-op=\"{}\"/>\n", esc(&l.name), l.opacity, blend_to_op(l.blend)));
                }
            }
        }
        Ok(())
    }
    walk(doc, &page.layers, &mut xml, &mut files)?;
    xml.push_str("</stack>\n</image>\n");
    let merged = nori_render::flatten(doc);
    let (tw, th, thumb) = nori_render::transform::fit_rgba(&merged, page.width, page.height, 256);
    let mut out = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let z = |e: zip::result::ZipError| e.to_string();
    zip.start_file("mimetype", stored).map_err(z)?;
    zip.write_all(b"image/openraster").map_err(|e| e.to_string())?;
    zip.start_file("stack.xml", stored).map_err(z)?;
    zip.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
    for (name, data) in files {
        zip.start_file(name, stored).map_err(z)?;
        zip.write_all(&data).map_err(|e| e.to_string())?;
    }
    zip.start_file("mergedimage.png", stored).map_err(z)?;
    zip.write_all(&nori_core::file::encode_png_rgba(page.width, page.height, &merged)?).map_err(|e| e.to_string())?;
    zip.start_file("Thumbnails/thumbnail.png", stored).map_err(z)?;
    zip.write_all(&nori_core::file::encode_png_rgba(tw, th, &thumb)?).map_err(|e| e.to_string())?;
    zip.finish().map_err(z)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_core::Color;

    #[test]
    fn round_trips_layers_and_groups() {
        let mut d = Document::new("t", 30, 20, Some(Color::WHITE));
        let id = d.new_id();
        let mut px = Raster::transparent(5, 5);
        px.set(1, 1, [255, 0, 0, 255]);
        let mut l = Layer::new(id, "Dot", Content::Raster { x: 3, y: 4, pixels: px });
        l.blend = BlendMode::Multiply;
        l.opacity = 0.5;
        let gid = d.new_id();
        d.insert_above(Layer::new(gid, "Group", Content::Group { children: vec![l], expanded: true }), None);
        let bytes = write(&d).unwrap();
        let back = read(&bytes, "t").unwrap();
        assert_eq!((back.width(), back.height()), (30, 20));
        let layers = &back.page().layers;
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].name, "Group");
        let dot = &layers[0].children()[0];
        assert_eq!(dot.blend, BlendMode::Multiply);
        assert!((dot.opacity - 0.5).abs() < 1e-3);
        assert_eq!(dot.raster().unwrap().2.get(1, 1), [255, 0, 0, 255]);
        assert_eq!(dot.raster_bounds().unwrap().x, 3);
        assert_eq!(layers[1].name, "Background");
    }
}
