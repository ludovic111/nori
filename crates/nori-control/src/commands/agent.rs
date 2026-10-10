//! `agent.*`: the built-in agent of the Agent panel. The app installs it (`nori-agent` builds on
//! this crate, so it is reached through [`AgentHost`](crate::session::AgentHost)); the window and
//! every other client drive the same conversation and runs. Choosing the provider and storing
//! keys are settings, handled here.

use std::sync::Arc;

use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Event, Session};
use crate::settings::AGENT_PROVIDERS;

/// The keys the agent keeps in the keychain, by provider.
pub const AGENT_KEY_IDS: &[&str] = &["anthropic", "openai", "openai-compatible"];

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "agent.setProvider" => {
            let p = a.str("provider")?.trim().to_ascii_lowercase();
            if !AGENT_PROVIDERS.contains(&p.as_str()) {
                let hint = crate::registry::closest(&p, AGENT_PROVIDERS).map(|c| format!(" Did you mean `{c}`?")).unwrap_or_default();
                return Err(format!("`{p}` isn't a provider.{hint} Providers: {}.", AGENT_PROVIDERS.join(", ")));
            }
            let settings = s.update_settings(|st| {
                st.agent.provider = p.clone();
                if let Some(m) = a.opt_str("model") {
                    st.agent.model = m.trim().to_string();
                }
                if let Some(u) = a.opt_str("baseUrl") {
                    st.agent.base_url = u.trim().to_string();
                }
            })?;
            Ok(json!({ "provider": settings.agent.provider, "model": settings.agent.model, "baseUrl": settings.agent.base_url }))
        }
        "agent.setKey" => {
            let p = a.str("provider")?.trim().to_ascii_lowercase();
            if !AGENT_KEY_IDS.contains(&p.as_str()) {
                return Err(format!("The agent keeps keys for {}, not `{p}` (Claude Code, Codex and Ollama need no key).", AGENT_KEY_IDS.join(", ")));
            }
            s.set_secret(&p, Some(a.str("key")?).filter(|k| !k.trim().is_empty()))?;
            s.emit(Event::SettingsChanged);
            Ok(json!({ "provider": p, "saved": s.secret(&p).is_some() }))
        }
        _ => {
            let host = s.agent_host().ok_or_else(|| format!("`{}` needs the nori app, where the built-in agent runs. Start it and use nori-cli without --file, or nori-mcp --live.", cx.spec.name))?;
            if let Some(t) = a.opt_f64("timeout")
                && t <= 0.0
            {
                return Err("timeout is a number of seconds above 0".into());
            }
            host.call(s.clone(), cx.source, cx.spec.name, a).await
        }
    }
}
