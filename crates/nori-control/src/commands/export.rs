//! `export.*`: files out, and what nori opens from other editors.

use std::sync::Arc;

use serde_json::json;

use super::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "export.formats" => Ok(json!({ "formats": nori_io::FORMATS, "apps": nori_io::APPS })),
        "export.file" => {
            let path = util::path(&a, "path")?;
            let doc = s.document()?;
            let page = match a.opt_str("page") {
                Some(k) => match doc.resolve_page(k)? {
                    nori_core::PageRef::Page(i) => Some(i),
                    nori_core::PageRef::Master(_) => return Err("Export a page, not a master.".into()),
                },
                None => None,
            };
            let pages = a.array("pages").map(|p| p.iter().filter_map(|v| v.as_u64()).filter(|n| *n >= 1).map(|n| n as usize - 1).collect::<Vec<_>>());
            let opts = nori_io::ExportOptions {
                format: a.opt_str("format").map(str::to_string),
                quality: a.opt_i64("quality").unwrap_or(90).clamp(1, 100) as u8,
                scale: a.opt_f64("scale").unwrap_or(1.0).clamp(0.01, 16.0) as f32,
                page,
                pages,
                ..Default::default()
            };
            let id = format!("export-{}", chrono::Utc::now().timestamp_millis());
            s.emit(crate::session::Event::Progress { id: id.clone(), label: format!("Exporting {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), done: false, error: None });
            let p2 = path.clone();
            let r = tokio::task::spawn_blocking(move || nori_io::export(&doc, &p2, &opts)).await.map_err(|e| e.to_string())?;
            s.emit(crate::session::Event::Progress { id, label: "Export".into(), done: true, error: r.as_ref().err().cloned() });
            let r = r?;
            Ok(json!(r))
        }
        _ => Err(super::unhandled(cx)),
    }
}
