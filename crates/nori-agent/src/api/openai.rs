//! OpenAI Chat Completions with streamed text and function tools. Also any compatible server
//! (LM Studio, vLLM, llama.cpp, LiteLLM, a proxy, through `settings.agent.baseUrl`): one client,
//! with [`Quirks`] for where they differ (usage in the stream, how many tools they take).

use serde_json::{Value, json};

use super::{Api, Call, Step, parse_args};
use crate::http::{self, Lines};
use crate::providers::Quirks;
use crate::tools::ToolSet;
use crate::{Message, Part, Role, Run, ToolDef};

pub(super) fn tools(defs: &[ToolDef]) -> Vec<Value> {
    defs.iter().map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.schema } })).collect()
}

/// Tool messages carry text only: pictures follow them in a user message (the latest few;
/// older ones, and all of them for a model that can't see, become a line of text).
pub(super) fn wire(system: &str, messages: &[Message], vision: bool) -> Vec<Value> {
    let recent = super::recent_pictures(messages, vision);
    let mut out = vec![json!({ "role": "system", "content": system })];
    for (i, m) in messages.iter().enumerate() {
        let text = m.text();
        match m.role {
            Role::User => {
                // Tool answers come first: they must follow the assistant's calls.
                for p in &m.parts {
                    if let Part::ToolResult { id, output, .. } = p {
                        out.push(json!({ "role": "tool", "tool_call_id": id, "content": output }));
                    }
                }
                let (shown, older) = super::pictures_of(m, i, &recent);
                if !shown.is_empty() || older > 0 {
                    let mut content = vec![json!({ "type": "text", "text": super::pictures_note(shown.len(), older) })];
                    content.extend(shown.iter().map(|(media_type, data)| json!({ "type": "image_url", "image_url": { "url": format!("data:{media_type};base64,{data}") } })));
                    out.push(json!({ "role": "user", "content": content }));
                }
                if !text.is_empty() {
                    out.push(json!({ "role": "user", "content": text }));
                }
            }
            Role::Assistant => {
                let calls: Vec<Value> = m
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::ToolUse { id, name, input } => Some(json!({ "id": id, "type": "function", "function": { "name": name, "arguments": input.to_string() } })),
                        _ => None,
                    })
                    .collect();
                let mut msg = json!({ "role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) } });
                if !calls.is_empty() {
                    msg["tool_calls"] = json!(calls);
                } else if text.is_empty() {
                    msg["content"] = json!("…");
                }
                out.push(msg);
            }
        }
    }
    out
}

pub(super) async fn step(api: &Api, q: Quirks, run: &Run, set: &ToolSet, messages: &[Message]) -> Result<Step, String> {
    let mut body = json!({
        "model": api.model,
        "messages": wire(&set.system_prompt(), messages, api.sees()),
        "tools": tools(&set.defs),
        "tool_choice": "auto",
        "stream": true,
    });
    if q.usage {
        body["stream_options"] = json!({ "include_usage": true });
    }
    let url = format!("{}/chat/completions", api.base);
    let label = api.label();
    let response = http::post(&run.cancel, &label, || api.authorized(api.http.post(&url)), &body).await?;

    let mut lines = Lines::new(response);
    let mut text = String::new();
    // index → (id, name, arguments)
    let mut calls: Vec<(String, String, String)> = vec![];
    let mut finish = String::new();
    let mut done = false;
    while let Some(data) = lines.next_sse().await? {
        let data = data.trim();
        if data == "[DONE]" {
            done = true;
            break;
        }
        let chunk: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(e) = chunk["error"]["message"].as_str().or_else(|| chunk["error"].as_str()) {
            return Err(format!("{label} error: {e}"));
        }
        if let Some(u) = chunk.get("usage").filter(|u| u.is_object()) {
            run.usage(u["prompt_tokens"].as_u64().unwrap_or(0), u["completion_tokens"].as_u64().unwrap_or(0));
        }
        // Some servers send notes (content filters) as chunks without choices.
        let Some(choice) = chunk["choices"].get(0) else { continue };
        let delta = &choice["delta"];
        if let Some(t) = delta["content"].as_str() {
            text.push_str(t);
            run.text(t);
        }
        for tc in delta["tool_calls"].as_array().into_iter().flatten() {
            // Some servers send whole calls without an index: a new call, unless its id was seen.
            let seen = tc["id"].as_str().and_then(|id| calls.iter().position(|c| !id.is_empty() && c.0 == id));
            let index = tc["index"].as_u64().map(|i| i as usize).or(seen).unwrap_or(calls.len());
            while calls.len() <= index {
                calls.push(Default::default());
            }
            let c = &mut calls[index];
            if let Some(id) = tc["id"].as_str() {
                c.0 = id.to_string();
            }
            if let Some(n) = tc["function"]["name"].as_str() {
                c.1.push_str(n);
            }
            if let Some(a) = tc["function"]["arguments"].as_str() {
                c.2.push_str(a);
            }
        }
        if let Some(f) = choice["finish_reason"].as_str() {
            finish = f.to_string();
        }
    }
    // Some compatible servers close after the finish reason without `[DONE]`; with neither, the
    // reply (and any half-streamed command) is cut off.
    if !done && finish.is_empty() {
        return Err(format!("{label}: the connection closed before the reply finished. Try again."));
    }
    match finish.as_str() {
        "length" => return Err("The reply reached its length limit; no unfinished command was run. Ask for less at a time.".into()),
        "content_filter" => return Err("The model declined this request.".into()),
        _ => {}
    }
    let mut parts = vec![];
    if !text.is_empty() {
        parts.push(Part::Text { text });
    }
    let mut out = vec![];
    for (i, (id, name, args)) in calls.into_iter().enumerate().filter(|(_, c)| !c.1.is_empty()) {
        let id = if id.is_empty() { format!("call_{i}") } else { id };
        let input = parse_args(&args);
        parts.push(Part::ToolUse { id: id.clone(), name: name.clone(), input: input.clone().unwrap_or_else(|_| json!({})) });
        out.push(Call { id, name, input });
    }
    Ok(Step { parts, calls: out })
}
