//! The models each provider offers, for the model pickers: fetched from the provider's own
//! list where it has one (cached for a few hours, refreshed on request), else a short built-in
//! list. Where the list says so, a model that can't call tools is marked; the agent needs tools.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use nori_control::Session;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;

use crate::providers::{Group, Wire};
use crate::{AgentConfig, ProviderKind};

/// How long a fetched list is used before it is fetched again.
const TTL: Duration = Duration::from_secs(6 * 3600);

/// Lists longer than this are cut (a proxy may have hundreds; the field still takes any id).
const MAX_MODELS: usize = 400;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    /// The provider's name for it, when it has one besides the id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether it can call tools, when the provider says (`false`: it can't run the agent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    /// Context window in tokens, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// Loaded in the local server now (LM Studio).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loaded: Option<bool>,
}

impl ModelInfo {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), name: None, tools: None, context: None, loaded: None }
    }
}

/// What `agent.models` answers.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelList {
    pub provider: ProviderKind,
    /// `api`: from the provider; `builtin`: nori's short list (no list to fetch, or it failed).
    pub source: &'static str,
    pub models: Vec<ModelInfo>,
    /// The model used when none is chosen (empty: the provider decides).
    pub default_model: String,
    pub fetched_at: DateTime<Utc>,
    /// Why the provider's list couldn't be fetched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

type Cache = Mutex<HashMap<String, (Instant, ModelList)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// A short fingerprint of a key, so a new key fetches a new list without the key being kept.
fn fingerprint(key: Option<&str>) -> String {
    key.map(|k| {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in k.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{h:x}")
    })
    .unwrap_or_default()
}

fn builtin(kind: ProviderKind) -> Vec<ModelInfo> {
    kind.info().models.iter().map(|m| ModelInfo::new(*m)).collect()
}

/// What is known of `kind`'s models without asking it: the last list fetched, else the built-in one.
pub fn known(kind: ProviderKind) -> (Vec<ModelInfo>, &'static str) {
    let prefix = format!("{kind}|");
    let cached = cache().lock().iter().filter(|(k, _)| k.starts_with(&prefix)).max_by_key(|(_, (at, _))| *at).map(|(_, (_, l))| (l.models.clone(), l.source));
    cached.unwrap_or_else(|| (builtin(kind), "builtin"))
}

/// The models of `kind` (the chosen provider's address and key, or its defaults for another).
pub async fn list(session: &Arc<Session>, kind: ProviderKind, refresh: bool) -> ModelList {
    let settings = session.settings().agent;
    let mut config = AgentConfig::from_settings(&settings);
    if config.provider != kind {
        config = AgentConfig::new(kind);
    }
    let key = config.api_key(session);
    let base = config.base_url();
    let cache_key = format!("{kind}|{base}|{}|{}", config.base_url, fingerprint(key.as_deref()));
    if !refresh && let Some((at, list)) = cache().lock().get(&cache_key).cloned() && at.elapsed() < TTL {
        return list;
    }
    let default_model = kind.default_model().to_string();
    let http = crate::http::client();
    let fetched: Result<Vec<ModelInfo>, String> = match kind {
        ProviderKind::ClaudeCode | ProviderKind::Codex => Err(String::new()),
        ProviderKind::Ollama => ollama(&http, &base).await,
        _ if kind.info().key.is_some_and(|k| k.required) && key.is_none() => Err("Add a key to see this provider's models.".into()),
        _ => fetch(&http, kind, &base, key.as_deref()).await,
    };
    let (source, mut models, error) = match fetched {
        Ok(m) if !m.is_empty() => ("api", m, None),
        Ok(_) => ("builtin", builtin(kind), Some("The provider listed no models.".to_string())),
        Err(e) => ("builtin", builtin(kind), (!e.is_empty()).then_some(e)),
    };
    models.truncate(MAX_MODELS);
    let list = ModelList { provider: kind, source, models, default_model, fetched_at: Utc::now(), error };
    // A failed fetch is tried again next time.
    if source == "api" || kind.group() == Group::Cli {
        cache().lock().insert(cache_key, (Instant::now(), list.clone()));
    }
    list
}

/// Not a model that chats (embeddings, speech, pictures, moderation…).
fn not_chat(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    ["embed", "whisper", "tts", "dall-e", "moderation", "rerank", "transcribe", "realtime", "image", "audio", "-search", "davinci", "babbage", "guard", "veo", "imagen", "lyria", "sora", "aqa"]
        .iter()
        .any(|w| id.contains(w))
}

fn get(http: &reqwest::Client, url: &str) -> reqwest::RequestBuilder {
    http.get(url).timeout(Duration::from_secs(8))
}

async fn json(r: reqwest::RequestBuilder, what: &str) -> Result<Value, String> {
    let r = r.send().await.map_err(|e| if e.is_connect() || e.is_timeout() { format!("{what} isn't answering.") } else { format!("Couldn't reach {what}: {e}") })?;
    let status = r.status();
    let text = r.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{what} answered {}: {}", status.as_u16(), crate::http::api_error(&text)));
    }
    serde_json::from_str(&text).map_err(|e| format!("{what} answered oddly: {e}"))
}

/// The provider's own model list (every wire but the CLIs and Ollama).
pub(crate) async fn fetch(http: &reqwest::Client, kind: ProviderKind, base: &str, key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
    let label = kind.label();
    let bearer = |r: reqwest::RequestBuilder| match key {
        Some(k) => r.bearer_auth(k),
        None => r,
    };
    let mut out: Vec<ModelInfo> = match kind.info().wire {
        Wire::Anthropic => {
            let v = json(get(http, &format!("{base}/v1/models?limit=1000")).header("x-api-key", key.unwrap_or("")).header("anthropic-version", "2023-06-01"), label).await?;
            v["data"].as_array().into_iter().flatten().filter_map(|m| Some(ModelInfo { name: m["display_name"].as_str().map(str::to_string), tools: Some(true), context: m["max_input_tokens"].as_u64(), ..ModelInfo::new(m["id"].as_str()?) })).collect()
        }
        _ => {
            let v = json(bearer(get(http, &format!("{base}/models"))), label).await?;
            let items = v["data"].as_array().or_else(|| v.as_array()).cloned().unwrap_or_default();
            items
                .iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    let tools = m["capabilities"]["function_calling"].as_bool().or_else(|| m["supports_tools"].as_bool());
                    let context = m["context_length"].as_u64().or_else(|| m["context_window"].as_u64()).or_else(|| m["max_context_length"].as_u64());
                    let name = m["name"].as_str().or_else(|| m["display_name"].as_str()).filter(|n| *n != id).map(str::to_string);
                    Some(ModelInfo { name, tools, context, ..ModelInfo::new(id) })
                })
                .filter(|m| !not_chat(&m.id))
                .collect()
        }
    };
    // Newest-looking first is the provider's order; keep it, but tool-capable ones before others.
    out.sort_by_key(|m| m.tools == Some(false));
    let default = kind.default_model();
    if let Some(i) = out.iter().position(|m| m.id == default) {
        let m = out.remove(i);
        out.insert(0, m);
    }
    Ok(out)
}

/// Installed Ollama models, with whether each can call tools (`/api/show` capabilities).
pub(crate) async fn ollama(http: &reqwest::Client, base: &str) -> Result<Vec<ModelInfo>, String> {
    let v = json(get(http, &format!("{base}/api/tags")).timeout(Duration::from_secs(3)), "Ollama")
        .await
        .map_err(|_| format!("Ollama isn't running at {base}. Start it, or choose another provider in Settings › Agent."))?;
    let names: Vec<String> = v["models"].as_array().into_iter().flatten().filter_map(|m| m["name"].as_str().map(str::to_string)).take(40).collect();
    let shows = names.iter().map(|name| async move {
        let r = http.post(format!("{base}/api/show")).json(&serde_json::json!({ "model": name })).timeout(Duration::from_secs(4)).send().await.ok()?;
        r.json::<Value>().await.ok()
    });
    let shows = futures::future::join_all(shows).await;
    Ok(names
        .into_iter()
        .zip(shows)
        .map(|(id, show)| {
            let tools = show.as_ref().and_then(|s| s["capabilities"].as_array()).map(|c| c.iter().any(|x| x == "tools"));
            let context = show.as_ref().and_then(|s| s["model_info"].as_object()).and_then(|o| o.iter().find(|(k, _)| k.ends_with(".context_length")).and_then(|(_, v)| v.as_u64()));
            ModelInfo { tools, context, ..ModelInfo::new(id) }
        })
        .collect())
}
