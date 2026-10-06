//! `app.*`: nori itself: version, commands, settings, updates, what's new, the first-run setup.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::registry::{self, Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "app.version" => Ok(json!({
            "app": "nori",
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "dataDir": s.data_dir.display().to_string(),
            "configDir": s.config_dir.display().to_string(),
            "commands": registry::commands().len(),
            "bridgePort": s.bridge_port(),
        })),
        "app.commands" => {
            let fam = a.opt_str("family");
            let list: Vec<Value> = registry::commands().iter().filter(|c| fam.is_none_or(|f| c.family() == f)).map(registry::describe).collect();
            Ok(json!({ "commands": list }))
        }
        "app.settings" => Ok(json!(s.settings())),
        "app.setSetting" => {
            let key = a.str("key")?.split('.').map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(".");
            if cx.source.is_agent() && (key == "agent" || key.starts_with("agent.") || key.starts_with("plugins.disabled")) {
                return Err("Agent settings, permissions and plugin switches stay with the person.".into());
            }
            let value = a.get("value").cloned().unwrap_or(Value::Null);
            let mut err = None;
            let settings = s.update_settings(|st| {
                if let Err(e) = st.set(&key, value) {
                    err = Some(e);
                }
            })?;
            if let Some(e) = err {
                return Err(e);
            }
            Ok(json!({ "key": key, "value": settings.get(&key) }))
        }
        "app.checkUpdates" => crate::update::check().await,
        "app.whatsNew" => {
            let all = a.bool_or("all", false);
            let releases = if all { crate::release_notes::all() } else { crate::release_notes::find(crate::update::CURRENT).into_iter().collect() };
            Ok(json!({ "releases": releases }))
        }
        "app.onboarding" => {
            let st = s.settings();
            let apps: Vec<Value> = nori_io::APPS.iter().map(|ap| json!({ "id": ap.id, "name": ap.name, "maker": ap.maker, "kind": ap.kind, "opens": ap.formats, "how": ap.how })).collect();
            Ok(json!({
                "done": !st.onboarding.completed.is_empty(),
                "comingFrom": st.onboarding.coming_from,
                "apps": apps,
                "agent": { "provider": st.agent.provider, "providers": crate::settings::AGENT_PROVIDERS },
                "account": crate::commands::account::status(false).await,
            }))
        }
        "app.finishOnboarding" => {
            let from = a.strings("comingFrom");
            for f in &from {
                if nori_io::app(f).is_none() {
                    return Err(format!("`{f}` isn't an app nori knows (app.onboarding lists them)."));
                }
            }
            s.update_settings(|st| {
                st.onboarding.completed = env!("CARGO_PKG_VERSION").to_string();
                st.onboarding.coming_from = from.clone();
            })?;
            Ok(json!({ "done": true, "comingFrom": from }))
        }
        _ => s.ui_call(cx.spec.name, Value::Object(a.0)).await,
    }
}
