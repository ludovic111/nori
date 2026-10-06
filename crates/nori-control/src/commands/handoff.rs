//! `handoff.*`: nori and the other lsuite apps. A picture goes to kimchi as a PNG imported into
//! its open project, through kimchi's own bridge (found in `~/.lsuite/apps/kimchi.json`).

use std::sync::Arc;

use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "handoff.apps" => {
            let apps: Vec<serde_json::Value> = crate::discovery::installed_apps()
                .into_iter()
                .map(|e| json!({ "app": e.app, "version": e.version, "kind": e.kind, "running": e.running.is_some(), "cli": e.cli, "mcp": e.mcp }))
                .collect();
            Ok(json!({ "apps": apps }))
        }
        "handoff.toKimchi" => {
            let kimchi = crate::discovery::find("kimchi").ok_or("kimchi isn't installed here (no ~/.lsuite/apps/kimchi.json). Install it from lsuite.xyz/kimchi.")?;
            let running = kimchi.running.clone().ok_or("kimchi isn't running: start it and open a project, then send again.")?;
            let control = running.control_file.clone().ok_or("kimchi didn't say how to reach it (no control file).")?;
            // The picture, as a PNG in nori's hand-off folder.
            let doc = s.document()?;
            let page = match a.opt_str("page") {
                Some(k) => match doc.resolve_page(k)? {
                    nori_core::PageRef::Page(i) => Some(i),
                    _ => return Err("Send a page, not a master.".into()),
                },
                None => None,
            };
            let dir = s.data_dir.join("handoff");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let stem: String = doc.name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' }).collect();
            let path = dir.join(format!("{stem}-{}.png", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
            let opts = nori_io::ExportOptions { format: Some("png".into()), scale: a.opt_f64("scale").unwrap_or(1.0) as f32, page, ..Default::default() };
            let p2 = path.clone();
            let exported = tokio::task::spawn_blocking(move || nori_io::export(&doc, &p2, &opts)).await.map_err(|e| e.to_string())??;
            let mut client = crate::bridge::Client::connect(&control, "cli").await.map_err(|e| format!("Couldn't reach kimchi: {e}"))?;
            let place = a.bool_or("place", true);
            let result = client.call("media.import", json!({ "paths": [path.display().to_string()], "place": place })).await.map_err(|e| format!("kimchi said: {e}"))?;
            if let (Some(dur), true) = (a.opt_f64("duration"), place)
                && let Some(clip) = result["clips"].as_array().and_then(|c| c.first()).and_then(|c| c.as_str().or_else(|| c["id"].as_str()))
            {
                let _ = client.call("clip.trim", json!({ "clipId": clip, "edge": "end", "time": result["start"].as_f64().unwrap_or(0.0) + dur })).await;
            }
            Ok(json!({ "sent": path.display().to_string(), "width": exported.width, "height": exported.height, "kimchi": result }))
        }
        _ => Err(super::unhandled(cx)),
    }
}
