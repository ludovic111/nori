//! `color.*`: the foreground and background colours, and imported swatches.

use std::sync::Arc;

use nori_core::Color;
use serde_json::json;

use super::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Colors, Event, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "color.get" => Ok(json!(*s.colors.read())),
        "color.set" => {
            let mut c = *s.colors.read();
            if a.bool_or("reset", false) {
                c = Colors::default();
            }
            if a.bool_or("swap", false) {
                std::mem::swap(&mut c.foreground, &mut c.background);
            }
            if let Some(v) = a.opt_str("foreground") {
                c.foreground = Color::parse(v)?;
            }
            if let Some(v) = a.opt_str("background") {
                c.background = Color::parse(v)?;
            }
            *s.colors.write() = c;
            s.emit(Event::ToolsChanged);
            Ok(json!(c))
        }
        "color.swatches" => Ok(json!({ "swatches": *s.swatches.read() })),
        "color.importSwatches" => {
            let path = util::path(&a, "path")?;
            let bytes = std::fs::read(&path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
            let list = nori_io::assets::read_ase(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            let n = list.len();
            {
                let mut sw = s.swatches.write();
                for w in list {
                    if !sw.iter().any(|x| x.name == w.name && x.color == w.color) {
                        sw.push(w);
                    }
                }
                let _ = std::fs::write(s.data_dir.join("swatches.json"), serde_json::to_vec_pretty(&*sw).unwrap_or_default());
            }
            s.emit(Event::ToolsChanged);
            Ok(json!({ "imported": n, "swatches": s.swatches.read().len() }))
        }
        _ => Err(super::unhandled(cx)),
    }
}

/// Loads the brush tips and swatches imported before (from nori's data folder).
pub fn load_assets(s: &Session) {
    if let Ok(entries) = std::fs::read_dir(s.data_dir.join("brushes")) {
        let mut tips = vec![];
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("gbr"))
                && let Ok(bytes) = std::fs::read(&p)
            {
                let stem = p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                match nori_io::assets::read_gbr(&bytes, &stem) {
                    Ok(t) => tips.push(Arc::new(t)),
                    Err(e) => tracing::warn!("{}: {e}", p.display()),
                }
            }
        }
        *s.brushes.write() = tips;
    }
    if let Ok(bytes) = std::fs::read(s.data_dir.join("swatches.json")) {
        #[derive(serde::Deserialize)]
        struct W {
            name: String,
            color: Color,
            #[serde(default)]
            group: String,
        }
        if let Ok(list) = serde_json::from_slice::<Vec<W>>(&bytes) {
            *s.swatches.write() = list.into_iter().map(|w| nori_io::assets::Swatch { name: w.name, color: w.color, group: w.group }).collect();
        }
    }
}
