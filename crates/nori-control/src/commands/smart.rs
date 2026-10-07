//! Embedded, editable sources with explicit replacement and lossless repeated resize.
use super::util;
use crate::{
    registry::{Args, Ctx},
    session::{CmdResult, Session},
};
use base64::Engine;
use nori_core::{BlendMode, Content, Document, Raster};
use serde_json::json;
use std::{io::Write, sync::Arc};
const B64: base64::engine::general_purpose::GeneralPurpose = base64::engine::general_purpose::STANDARD;
fn encode(d: &Document) -> CmdResult<Arc<String>> {
    Ok(Arc::new(B64.encode(nori_core::file::to_bytes(d, None)?)))
}
fn decode(s: &str) -> CmdResult<Document> {
    if s.len() > 256 * 1024 * 1024 {
        return Err("The embedded source is too large.".into());
    }
    nori_core::file::from_bytes(&B64.decode(s).map_err(|e| e.to_string())?)
}
fn render(d: &Document, w: u32, h: u32) -> CmdResult<Raster> {
    if w == 0
        || h == 0
        || w > nori_core::MAX_SIDE
        || h > nori_core::MAX_SIDE
        || u64::from(w) * u64::from(h) > 64_000_000
        || u64::from(d.width()) * u64::from(d.height()) > 64_000_000
    {
        return Err("Smart object dimensions must fit within 64 million pixels and 30,000 pixels per side.".into());
    }
    let p = Raster::from_rgba(d.width(), d.height(), &nori_render::flatten(d));
    Ok(if (w, h) == (d.width(), d.height()) { p } else { nori_render::transform::resize_raster(&p, w, h, nori_render::transform::Filter::Bicubic) })
}
pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    let (doc, rev, doc_id) = s.snapshot()?;
    let id = util::layer_id(&doc, &a)?;
    let layer = doc.layer(&id).ok_or("No layer")?.clone();
    if cx.spec.name == "layer.extractSmartObject" {
        let source = decode(layer.smart_source.as_deref().ok_or("Not a smart object")?)?;
        let path = util::path(&a, "path")?;
        if path.extension().and_then(|x| x.to_str()) != Some("nori") {
            return Err("Save the source with a .nori extension.".into());
        }
        let bytes = nori_core::file::to_bytes(&source, None)?;
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| e.to_string())?;
        if let Err(e) = out.write_all(&bytes) {
            drop(out);
            let _ = std::fs::remove_file(&path);
            return Err(e.to_string());
        }
        return Ok(json!({"path":path,"layerId":id}));
    }
    util::unlocked(&layer)?;
    let name = cx.spec.name;
    let layer_id = id.clone();
    let (source, pixels) = tokio::task::spawn_blocking(move || -> CmdResult<_> {
        let source = match name {
            "layer.makeSmartObject" => {
                if layer.smart_source.is_some() {
                    return Err("Already a smart object.".into());
                }
                if layer.clipped || matches!(layer.content, Content::Adjustment { .. }) {
                    return Err("Group the adjustment and its picture before converting to a smart object.".into());
                }
                let mut src = doc.clone();
                let mut page = doc.page_of(&id).ok_or("No page")?.clone();
                let mut l = layer.clone();
                l.opacity = 1.;
                l.blend = BlendMode::Normal;
                l.visible = true;
                l.mask = None;
                l.clipped = false;
                page.layers = vec![l];
                page.master = None;
                src.active_page = page.id.clone();
                src.pages = vec![page];
                src.masters.clear();
                src.selection = None;
                encode(&src)?
            }
            "layer.replaceSmartObject" => {
                if layer.smart_source.is_none() {
                    return Err("Not a smart object.".into());
                }
                encode(&nori_io::open(&util::path(&a, "path")?)?)?
            }
            _ => layer.smart_source.clone().ok_or("Not a smart object")?,
        };
        let src = decode(&source)?;
        let (w, h) = if name == "layer.resizeSmartObject" {
            (a.opt_u32("width").ok_or("Invalid width")?, a.opt_u32("height").ok_or("Invalid height")?)
        } else if name == "layer.replaceSmartObject" {
            let (_, _, p) = layer.raster().ok_or("No smart preview")?;
            (p.width(), p.height())
        } else {
            (src.width(), src.height())
        };
        let pixels = render(&src, w, h)?;
        Ok((source, pixels))
    })
    .await
    .map_err(|e| e.to_string())??;
    // Snapshot revision prevents applying an asynchronous render to another tab or edit.
    let id = layer_id;
    s.edit_at(doc_id, rev, cx.label(), cx.source, move |d| {
        let l = d.layer_mut(&id).ok_or("No layer")?;
        let (x, y) = if name == "layer.makeSmartObject" { (0, 0) } else { l.raster().map(|(x, y, _)| (x, y)).ok_or("No preview")? };
        l.smart_source = Some(source);
        l.content = Content::Raster { x, y, pixels };
        Ok(json!({"layerId":id,"smartObject":true}))
    })
}
