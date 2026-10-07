//! Photoshop documents (`.psd`): layers with their pixels, positions, opacity, blend modes,
//! visibility, clipping and groups (through the `psd` crate). Photoshop, Affinity Photo,
//! Pixelmator Pro, Photopea, GIMP and Krita all write PSD. Text, smart objects and adjustment
//! layers come in as the pixels Photoshop saved for them.

use nori_core::{BlendMode, Content, Document, Layer, Raster};

/// The crate keeps its blend mode type private: it is read by name.
fn blend(m: impl std::fmt::Debug) -> BlendMode {
    match format!("{m:?}").as_str() {
        "PassThrough" => BlendMode::PassThrough,
        "Darken" | "DarkerColor" => BlendMode::Darken,
        "Multiply" => BlendMode::Multiply,
        "ColorBurn" => BlendMode::ColorBurn,
        "LinearBurn" => BlendMode::LinearBurn,
        "Lighten" | "LighterColor" => BlendMode::Lighten,
        "Screen" => BlendMode::Screen,
        "ColorDodge" => BlendMode::ColorDodge,
        "LinearDodge" => BlendMode::LinearDodge,
        "Overlay" => BlendMode::Overlay,
        "SoftLight" => BlendMode::SoftLight,
        "HardLight" | "HardMix" => BlendMode::HardLight,
        "VividLight" => BlendMode::VividLight,
        "LinearLight" => BlendMode::LinearLight,
        "PinLight" => BlendMode::PinLight,
        "Difference" => BlendMode::Difference,
        "Exclusion" => BlendMode::Exclusion,
        "Subtract" => BlendMode::Subtract,
        "Divide" => BlendMode::Divide,
        "Hue" => BlendMode::Hue,
        "Saturation" => BlendMode::Saturation,
        "Color" => BlendMode::Color,
        "Luminosity" => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

/// Reads a PSD into a document (one page).
pub fn read(bytes: &[u8], name: &str) -> Result<Document, String> {
    let psd = psd::Psd::from_bytes(bytes).map_err(|e| format!("not a PSD nori can read: {e}"))?;
    let (w, h) = (psd.width(), psd.height());
    let mut doc = Document::empty(name, w, h);
    let layers = psd.layers();
    if layers.is_empty() {
        // A flat PSD: its composite image.
        let id = doc.new_id();
        doc.pages[0].layers.push(Layer::new(id.clone(), "Background", Content::Raster { x: 0, y: 0, pixels: Raster::from_rgba(w, h, &psd.rgba()) }));
        doc.active = Some(id);
        return Ok(doc);
    }
    // Nodes: every layer (by index, bottom first in the file) and every group, with the index
    // of the topmost layer inside it to put siblings in order.
    struct Node {
        layer: Layer,
        parent: Option<u32>,
        order: usize,
    }
    let mut groups: std::collections::HashMap<u32, Node> = std::collections::HashMap::new();
    for gid in psd.group_ids_in_order() {
        let Some(g) = psd.groups().get(gid) else { continue };
        let id = doc.new_id();
        let mut l = Layer::new(id, g.name(), Content::Group { children: vec![], expanded: true });
        l.visible = g.visible();
        l.opacity = g.opacity() as f32 / 255.0;
        l.blend = blend(g.blend_mode());
        groups.insert(*gid, Node { layer: l, parent: g.parent_id(), order: 0 });
    }
    let mut leaves: Vec<Node> = vec![];
    for (i, pl) in layers.iter().enumerate() {
        let id = doc.new_id();
        let (x0, y0) = (pl.layer_left().clamp(0, w as i32), pl.layer_top().clamp(0, h as i32));
        let (x1, y1) = (pl.layer_right().saturating_add(1).clamp(0, w as i32), pl.layer_bottom().saturating_add(1).clamp(0, h as i32));
        let (lw, lh) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);
        // The crate gives the layer drawn over the whole canvas; keep only its box.
        let full = pl.rgba();
        let mut px = vec![0u8; (lw * lh * 4) as usize];
        if full.len() == (w * h * 4) as usize {
            for row in 0..lh.min((y1 - y0).max(0) as u32) {
                let src = (((y0 as u32 + row) * w + x0 as u32) * 4) as usize;
                let n = ((x1 - x0).max(0) as u32 * 4) as usize;
                px[(row * lw * 4) as usize..(row * lw * 4) as usize + n].copy_from_slice(&full[src..src + n]);
            }
        }
        let mut l = Layer::new(id, pl.name(), Content::Raster { x: x0, y: y0, pixels: Raster::from_rgba(lw, lh, &px) });
        l.visible = pl.visible();
        l.opacity = pl.opacity() as f32 / 255.0;
        l.blend = blend(pl.blend_mode());
        if l.blend == BlendMode::PassThrough {
            l.blend = BlendMode::Normal;
        }
        l.clipped = pl.is_clipping_mask();
        leaves.push(Node { layer: l, parent: pl.parent_id(), order: i });
    }
    // A group sits where its topmost layer is.
    for leaf in &leaves {
        let mut p = leaf.parent;
        while let Some(gid) = p {
            let Some(g) = groups.get_mut(&gid) else { break };
            g.order = g.order.max(leaf.order);
            p = g.parent;
        }
    }
    // Groups go into their parents deepest first.
    let nodes: Vec<Node> = leaves;
    let mut gids: Vec<u32> = groups.keys().copied().collect();
    let depth = |gid: u32, groups: &std::collections::HashMap<u32, Node>| {
        let mut d = 0;
        let mut p = groups.get(&gid).and_then(|g| g.parent);
        while let Some(x) = p {
            d += 1;
            p = groups.get(&x).and_then(|g| g.parent);
            if d > 64 {
                break;
            }
        }
        d
    };
    gids.sort_by_key(|g| std::cmp::Reverse(depth(*g, &groups)));
    let mut kids: std::collections::HashMap<u32, Vec<Node>> = std::collections::HashMap::new();
    let mut top: Vec<Node> = vec![];
    for n in nodes {
        match n.parent.filter(|p| groups.contains_key(p)) {
            Some(p) => kids.entry(p).or_default().push(n),
            None => top.push(n),
        }
    }
    for gid in gids {
        let Some(mut g) = groups.remove(&gid) else { continue };
        let mut children = kids.remove(&gid).unwrap_or_default();
        children.sort_by_key(|n| std::cmp::Reverse(n.order));
        if let Content::Group { children: c, .. } = &mut g.layer.content {
            *c = children.into_iter().map(|n| n.layer).collect();
        }
        match g.parent.filter(|p| groups.contains_key(p)) {
            Some(p) => kids.entry(p).or_default().push(g),
            None => top.push(g),
        }
    }
    top.sort_by_key(|n| std::cmp::Reverse(n.order));
    doc.pages[0].layers = top.into_iter().map(|n| n.layer).collect();
    doc.active = doc.pages[0].layers.first().map(|l| l.id.clone());
    Ok(doc)
}

/// Writes an RGB/8 PSD with editable pixel layers and a composite preview. Text, vectors,
/// masks and groups are rendered per layer; compositing dependencies use `plan::split`.
pub fn write(doc: &Document, page: usize) -> Result<Vec<u8>, String> {
    let page = doc.pages.get(page).ok_or("No such page")?;
    let (w, h) = (page.width, page.height);
    if w > 30_000 || h > 30_000 || u64::from(w) * u64::from(h) > 64_000_000 { return Err("PSD export supports at most 64 million pixels, 30000 per side.".into()); }
    fn u32b(out: &mut Vec<u8>, v: u32) { out.extend(v.to_be_bytes()); }
    fn code(b: BlendMode) -> &'static [u8; 4] { match b {
        BlendMode::Multiply => b"mul ", BlendMode::Screen => b"scrn", BlendMode::Overlay => b"over", BlendMode::Darken => b"dark", BlendMode::Lighten => b"lite",
        BlendMode::ColorBurn => b"idiv", BlendMode::LinearBurn => b"lbrn", BlendMode::ColorDodge => b"div ", BlendMode::LinearDodge => b"lddg",
        BlendMode::SoftLight => b"sLit", BlendMode::HardLight => b"hLit", BlendMode::VividLight => b"vLit", BlendMode::LinearLight => b"lLit", BlendMode::PinLight => b"pLit",
        BlendMode::Difference => b"diff", BlendMode::Exclusion => b"smud", BlendMode::Subtract => b"fsub", BlendMode::Divide => b"fdiv",
        BlendMode::Hue => b"hue ", BlendMode::Saturation => b"sat ", BlendMode::Color => b"colr", BlendMode::Luminosity => b"lum ", _ => b"norm"
    } }
    let (flat, above) = crate::plan::split(doc, page);
    let mut layers = above;
    if let Some((pixels, x, y)) = flat { layers.push(Layer::new("composite", "Composited layers", Content::Raster { x, y, pixels })); }
    if layers.is_empty() { layers.push(Layer::new("empty", "Empty", Content::Raster { x: 0, y: 0, pixels: Raster::transparent(1, 1) })); }
    if layers.len() > 4096 { return Err("Too many layers for PSD export.".into()); }
    let mut records = Vec::new();
    let mut channels = Vec::new();
    let mut count = 0i16;
    // PSD records are top to bottom; the decoder exposes them in reverse order.
    for l in &layers {
        let (x, y, lw, lh, rgba) = if let Content::Raster { x, y, pixels } = &l.content {
            if l.mask.is_none() { (*x, *y, pixels.width(), pixels.height(), pixels.to_vec()) }
            else { layer_pixels(doc, page, l) }
        } else { layer_pixels(doc, page, l) };
        let pixels = u64::from(lw) * u64::from(lh);
        if pixels > 64_000_000 || channels.len() as u64 + pixels * 4 > 512_000_000 { return Err("PSD layers exceed the 512 MB export limit.".into()); }
        records.extend(y.to_be_bytes()); records.extend(x.to_be_bytes());
        records.extend((y + lh as i32).to_be_bytes()); records.extend((x + lw as i32).to_be_bytes());
        records.extend(4u16.to_be_bytes());
        for (channel, component) in [(0i16, 0usize), (1, 1), (2, 2), (-1, 3)] {
            records.extend(channel.to_be_bytes()); u32b(&mut records, (pixels + 2) as u32);
            channels.extend(0u16.to_be_bytes());
            channels.extend(rgba.as_chunks::<4>().0.iter().map(|p| p[component]));
        }
        records.extend(b"8BIM"); records.extend(code(l.blend));
        records.extend([(l.opacity.clamp(0.0, 1.0) * 255.0).round() as u8, 0, if l.visible { 0 } else { 2 }, 0]);
        let mut extra = vec![0u8; 8]; // No layer mask or blend-range blocks (masks are baked).
        let name: Vec<u8> = l.name.chars().take(255).map(|c| if c.is_ascii() { c as u8 } else { b'?' }).collect();
        extra.push(name.len() as u8); extra.extend(name);
        while !extra.len().is_multiple_of(4) { extra.push(0); }
        let unicode: Vec<_> = l.name.encode_utf16().collect();
        extra.extend(b"8BIMluni"); u32b(&mut extra, 4 + unicode.len() as u32 * 2); u32b(&mut extra, unicode.len() as u32);
        for c in unicode { extra.extend(c.to_be_bytes()); }
        if !extra.len().is_multiple_of(2) { extra.push(0); }
        u32b(&mut records, extra.len() as u32); records.extend(extra);
        count += 1;
    }
    let mut info = Vec::new(); info.extend((-count).to_be_bytes()); info.extend(records); info.extend(channels);
    if info.len() % 2 != 0 { info.push(0); }
    let mut out = Vec::new(); out.extend(b"8BPS"); out.extend(1u16.to_be_bytes()); out.extend([0; 6]);
    out.extend(4u16.to_be_bytes()); u32b(&mut out, h); u32b(&mut out, w); out.extend(8u16.to_be_bytes()); out.extend(3u16.to_be_bytes());
    u32b(&mut out, 0); u32b(&mut out, 0);
    u32b(&mut out, info.len() as u32 + 8); u32b(&mut out, info.len() as u32); out.extend(info); u32b(&mut out, 0);
    out.extend(0u16.to_be_bytes());
    let preview = nori_render::flatten_page(doc, page);
    for c in 0..4 { out.extend(preview.as_chunks::<4>().0.iter().map(|p| p[c])); }
    Ok(out)
}

pub(crate) fn layer_pixels(doc: &Document, page: &nori_core::Page, layer: &Layer) -> (i32, i32, u32, u32, Vec<u8>) {
    let mut d = doc.clone(); let mut p = page.clone(); let mut l = layer.clone();
    l.visible = true; l.opacity = 1.0; l.blend = BlendMode::Normal; l.clipped = false;
    p.layers = vec![l]; p.master = None;
    d.pages = vec![p]; d.active_page = d.pages[0].id.clone();
    (0, 0, page.width, page.height, nori_render::flatten_page(&d, &d.pages[0]))
}

#[cfg(test)]
mod export_tests {
    use super::*;
    #[test]
    fn exported_psd_keeps_layer_order_opacity_and_pixels() {
        let mut d = Document::new("Layered", 8, 8, Some(nori_core::Color::WHITE));
        let id = d.new_id();
        let mut l = Layer::new(id, "Red", Content::Raster { x: 2, y: 3, pixels: Raster::from_rgba(2, 2, &[255, 0, 0, 255].repeat(4)) });
        l.opacity = 0.5; d.insert_above(l, None);
        let bytes = write(&d, 0).unwrap();
        let restored = read(&bytes, "Roundtrip").unwrap();
        assert_eq!(restored.pages[0].layers.len(), 2);
        assert_eq!(restored.pages[0].layers[0].name, "Red");
        assert!((restored.pages[0].layers[0].opacity - 0.5).abs() < 0.005);
        assert_eq!(restored.pages[0].layers[0].raster().unwrap().0, 2);
        assert_eq!(restored.pages[0].layers[0].raster().unwrap().2.to_vec(), [255, 0, 0, 255].repeat(4));
    }
}
