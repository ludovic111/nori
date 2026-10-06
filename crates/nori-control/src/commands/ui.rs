//! `ui.*`: what the window shows. Everything but `ui.state` is carried out by the window itself
//! (see `Session::ui_call`).

use std::sync::Arc;

use serde_json::{Value, json};

use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub const TOOLS: &[&str] = &["move", "select", "ellipseSelect", "lasso", "wand", "crop", "eyedropper", "brush", "eraser", "fill", "gradient", "pen", "direct", "shape", "text", "frame", "hand", "zoom"];
pub const PANELS: &[&str] = &["agent", "plugins", "settings", "export", "shortcuts", "whatsNew", "onboarding", "pages", "layers", "home", "account"];

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "ui.state" => Ok(json!(s.ui_state())),
        "ui.setTool" => {
            let t = a.str("tool")?;
            if !TOOLS.contains(&t) {
                let hint = crate::registry::closest(t, TOOLS).map(|c| format!(" Did you mean `{c}`?")).unwrap_or_default();
                return Err(format!("No tool `{t}`.{hint} Tools: {}.", TOOLS.join(", ")));
            }
            s.ui_call(cx.spec.name, Value::Object(a.0)).await
        }
        "ui.showPanel" => {
            let p = a.str("panel")?;
            if !PANELS.contains(&p) {
                return Err(format!("No panel `{p}`. Panels: {}.", PANELS.join(", ")));
            }
            s.ui_call(cx.spec.name, Value::Object(a.0)).await
        }
        _ => s.ui_call(cx.spec.name, Value::Object(a.0)).await,
    }
}
