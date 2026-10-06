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
        let (x1, y1) = (pl.layer_right().clamp(0, w as i32), pl.layer_bottom().clamp(0, h as i32));
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
