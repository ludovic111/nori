use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use nori_control::{Session, SessionOptions, Source};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use crate::*;

/// A scratch `LSUITE_HOME` for the whole test run (set once: the environment is shared), so no
/// test reads the plugins or apps of the person running them.
fn lsuite_home() -> &'static std::path::Path {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let d = tempfile::tempdir().unwrap();
        // SAFETY: set once, before any test reads them.
        unsafe {
            std::env::set_var("LSUITE_HOME", d.path());
            std::env::set_var("NORI_NO_SYSTEM_FONTS", "1");
        }
        d
    })
    .path()
}

fn session(dir: &std::path::Path) -> Arc<Session> {
    lsuite_home();
    Session::new(SessionOptions { data_dir: Some(dir.join("data")), config_dir: Some(dir.join("config")), secrets: None, headless: true }).unwrap()
}

pub(crate) async fn with_doc(dir: &std::path::Path) -> Arc<Session> {
    let s = session(dir);
    nori_control::call(&s, Source::Window, "doc.new", json!({ "name": "Test", "width": 400, "height": 300 })).await.unwrap();
    s
}

/// The names of the active page's layers, top first.
fn layer_names(s: &Session) -> Vec<String> {
    s.document().unwrap().page().all().iter().map(|l| l.name.clone()).collect()
}

/// Answers each request with the next body, in order (the last one repeats).
struct Script {
    bodies: Vec<String>,
    content_type: &'static str,
    next: AtomicUsize,
}

impl Respond for Script {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let i = self.next.fetch_add(1, Ordering::SeqCst).min(self.bodies.len() - 1);
        ResponseTemplate::new(200).insert_header("content-type", self.content_type).set_body_string(self.bodies[i].clone())
    }
}

async fn mock(route: &str, content_type: &'static str, bodies: Vec<String>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path(route)).respond_with(Script { bodies, content_type, next: AtomicUsize::new(0) }).mount(&server).await;
    server
}

fn sse(events: &[Value]) -> String {
    events.iter().map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap_or("message"))).collect()
}

fn openai_sse(chunks: &[Value]) -> String {
    let mut s: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    s.push_str("data: [DONE]\n\n");
    s
}

/// Every event of a run, up to and including the terminal one.
async fn collect(run: &mut AgentRun) -> Vec<AgentEvent> {
    let mut out = vec![];
    tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(e) = run.next_event().await {
            out.push(e);
        }
    })
    .await
    .expect("the run finished");
    out
}

/// The events a panel draws, without status lines and token counts.
fn visible(events: &[AgentEvent]) -> Vec<&AgentEvent> {
    events.iter().filter(|e| !matches!(e, AgentEvent::Status { .. } | AgentEvent::Usage { .. })).collect()
}

fn anthropic_text(text: &str) -> String {
    sse(&[
        json!({ "type": "message_start", "message": { "id": "msg_2", "role": "assistant", "content": [], "usage": { "input_tokens": 200, "output_tokens": 1 } } }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": 8 } }),
        json!({ "type": "message_stop" }),
    ])
}

/// One Anthropic answer that calls a tool.
fn anthropic_tool(id: &str, name: &str, input: &str) -> String {
    sse(&[
        json!({ "type": "message_start", "message": { "id": "msg_t", "role": "assistant", "content": [], "usage": { "input_tokens": 100, "output_tokens": 1 } } }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "tool_use", "id": id, "name": name, "input": {} } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "input_json_delta", "partial_json": input } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 20 } }),
        json!({ "type": "message_stop" }),
    ])
}

#[tokio::test(flavor = "multi_thread")]
async fn anthropic_tool_call_lands_in_the_document_and_the_run_reverts() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_doc(dir.path()).await;
    s.set_secret("anthropic", Some("sk-ant-test")).unwrap();
    let before = layer_names(&s);
    let first = sse(&[
        json!({ "type": "message_start", "message": { "id": "msg_1", "role": "assistant", "content": [], "usage": { "input_tokens": 120, "output_tokens": 1 } } }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "" } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "signature_delta", "signature": "sig-abc" } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "text", "text": "" } }),
        json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "Adding a sky." } }),
        json!({ "type": "content_block_stop", "index": 1 }),
        json!({ "type": "content_block_start", "index": 2, "content_block": { "type": "tool_use", "id": "toolu_1", "name": "layer_add", "input": {} } }),
        json!({ "type": "content_block_delta", "index": 2, "delta": { "type": "input_json_delta", "partial_json": "{\"name\": \"Sk" } }),
        json!({ "type": "content_block_delta", "index": 2, "delta": { "type": "input_json_delta", "partial_json": "y\", \"kind\": \"fill\", \"color\": \"#3366ff\"}" } }),
        json!({ "type": "content_block_stop", "index": 2 }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 40 } }),
        json!({ "type": "message_stop" }),
    ]);
    let server = mock("/v1/messages", "text/event-stream", vec![first, anthropic_text("Added the sky."), anthropic_text("Sure.")]).await;
    let config = AgentConfig { base_url: server.uri(), ..AgentConfig::new(ProviderKind::Anthropic) };

    let mut run = Agent::start(&s, config.clone(), "Make the sky blue", Conversation::new());
    let events = collect(&mut run).await;
    let shown = visible(&events);
    assert_eq!(shown.len(), 4, "{shown:#?}");
    assert!(matches!(shown[0], AgentEvent::Text { delta } if delta == "Adding a sky."));
    match shown[1] {
        AgentEvent::Command { record, result } => {
            assert_eq!(record.command, "layer.add");
            assert_eq!(record.source, Source::Agent);
            assert!(record.ok && record.mutates);
            assert!(record.seq > 0);
            assert_eq!(record.created.len(), 1, "{record:?}");
            assert!(result.is_some());
        }
        e => panic!("expected a command card, got {e:?}"),
    }
    assert!(matches!(shown[2], AgentEvent::Text { delta } if delta == "\n\nAdded the sky."));
    let (checkpoint, conversation) = match shown[3] {
        AgentEvent::Done { summary, checkpoint, changes, conversation } => {
            assert_eq!(summary, "Added the sky.");
            assert_eq!(*changes, 1);
            (checkpoint.expect("a checkpoint before the change"), conversation.clone())
        }
        e => panic!("expected Done, got {e:?}"),
    };
    assert_eq!(layer_names(&s)[0], "Sky");
    assert_eq!(run.checkpoint(), Some(checkpoint));

    // What the model was sent back: the thinking block verbatim, then the tool result.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].headers.get("x-api-key").unwrap(), "sk-ant-test");
    let second: Value = serde_json::from_slice(&requests[1].body).unwrap();
    assert_eq!(second["messages"][1]["content"][0], json!({ "type": "thinking", "thinking": "", "signature": "sig-abc" }));
    assert_eq!(second["messages"][1]["content"][2]["input"], json!({ "name": "Sky", "kind": "fill", "color": "#3366ff" }));
    let result = &second["messages"][2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], "toolu_1");
    assert_eq!(result["is_error"], false);
    assert_eq!(second["tools"].as_array().unwrap().len(), tool_defs().len());
    assert!(second["system"].as_str().unwrap().contains("nori"));

    // A follow-up continues the thread.
    let mut next = Agent::start(&s, config, "Thanks", conversation);
    let events = collect(&mut next).await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { summary, checkpoint: None, changes: 0, .. }) if summary == "Sure."));
    let third: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[2].body).unwrap();
    assert_eq!(third["messages"].as_array().unwrap().len(), 5);
    let followup = third["messages"][4]["content"][0]["text"].as_str().unwrap();
    assert!(followup.starts_with("<context>\n") && followup.ends_with("\n</context>\n\nThanks"), "{followup}");

    // Revert this run.
    revert(&s, checkpoint).await.unwrap();
    assert_eq!(layer_names(&s), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_permission_goes_back_to_the_model_as_a_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_doc(dir.path()).await;
    s.set_secret("openai", Some("sk-test")).unwrap();
    let first = openai_sse(&[
        json!({ "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "Turning off update checks." } }] }),
        json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_1", "type": "function", "function": { "name": "app_setSetting", "arguments": "" } }] } }] }),
        json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": "{\"key\":\"updates.checkOnStart\",\"value\":false}" } }] } }] }),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] }),
    ]);
    let second = openai_sse(&[
        json!({ "choices": [{ "index": 0, "delta": { "content": "I can't: the settings permission is off." } }] }),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] }),
    ]);
    let server = mock("/chat/completions", "text/event-stream", vec![first, second]).await;
    let config = AgentConfig { base_url: server.uri(), ..AgentConfig::new(ProviderKind::OpenAi) };
    let mut run = Agent::start(&s, config, "Stop checking for updates", Conversation::new());
    let events = collect(&mut run).await;
    let shown = visible(&events);
    assert_eq!(shown.len(), 4, "{shown:#?}");
    match shown[1] {
        AgentEvent::Command { record, result } => {
            assert_eq!(record.command, "app.setSetting");
            assert!(!record.ok);
            assert!(record.error.as_deref().unwrap().contains("\"settings\" permission"), "{record:?}");
            assert!(result.is_none());
        }
        e => panic!("expected a command card, got {e:?}"),
    }
    assert!(matches!(shown[3], AgentEvent::Done { checkpoint: None, changes: 0, .. }), "{:?}", shown[3]);
    assert!(s.settings().updates.check_on_start);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].headers.get("authorization").unwrap(), "Bearer sk-test");
    let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
    let tool = body["messages"].as_array().unwrap().iter().find(|m| m["role"] == "tool").expect("a tool message");
    assert_eq!(tool["tool_call_id"], "call_1");
    assert!(tool["content"].as_str().unwrap().contains("permission"), "{tool}");
    assert_eq!(body["messages"][0]["role"], "system");
}

#[tokio::test(flavor = "multi_thread")]
async fn ollama_runs_tools_locally() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_doc(dir.path()).await;
    let first = [
        json!({ "message": { "role": "assistant", "content": "", "tool_calls": [{ "function": { "name": "layer_add", "arguments": { "name": "Drop" } } }] }, "done": false }),
        json!({ "message": { "role": "assistant", "content": "" }, "done": true, "done_reason": "stop", "prompt_eval_count": 10, "eval_count": 5 }),
    ]
    .map(|v| v.to_string())
    .join("\n");
    let second = json!({ "message": { "role": "assistant", "content": "Layer added." }, "done": true }).to_string();
    let server = mock("/api/chat", "application/x-ndjson", vec![first, second]).await;
    let config = AgentConfig { base_url: server.uri(), model: "qwen3".into(), ..AgentConfig::new(ProviderKind::Ollama) };
    let mut run = Agent::start(&s, config, "Add a layer called Drop", Conversation::new());
    let events = collect(&mut run).await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { changes: 1, summary, .. }) if summary == "Layer added."), "{events:#?}");
    assert_eq!(layer_names(&s)[0], "Drop");
    // The run first asks Ollama whether the model can see (`/api/show`).
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].url.path(), "/api/show");
    let chats: Vec<_> = requests.iter().filter(|r| r.url.path() == "/api/chat").collect();
    let body: Value = serde_json::from_slice(&chats[0].body).unwrap();
    assert!(body["tools"].as_array().unwrap().len() < 30, "small models get the short list");
    let body: Value = serde_json::from_slice(&chats[1].body).unwrap();
    let tool = body["messages"].as_array().unwrap().iter().find(|m| m["role"] == "tool").unwrap();
    assert_eq!(tool["tool_name"], "layer_add");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_stops_a_run_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_doc(dir.path()).await;
    s.set_secret("anthropic", Some("k")).unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60))).mount(&server).await;
    let config = AgentConfig { base_url: server.uri(), ..AgentConfig::new(ProviderKind::Anthropic) };
    let mut run = Agent::start(&s, config, "Do something slow", Conversation::new());
    assert!(matches!(run.next_event().await, Some(AgentEvent::Status { .. })));
    run.cancel();
    let events = tokio::time::timeout(Duration::from_secs(5), collect(&mut run)).await.expect("cancelled promptly");
    assert!(matches!(events.last(), Some(AgentEvent::Cancelled { checkpoint: None, changes: 0 })), "{events:?}");
    assert!(run.is_finished());
    // The thread keeps the request, so a follow-up has the context.
    assert_eq!(run.conversation().messages.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_keys_and_disabled_agents_are_explained() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_doc(dir.path()).await;
    // Point at a dead address so a key from the environment can't reach anything.
    let config = AgentConfig { base_url: "http://127.0.0.1:9".into(), ..AgentConfig::new(ProviderKind::Anthropic) };
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        let mut run = Agent::start(&s, config.clone(), "Hi", Conversation::new());
        let events = collect(&mut run).await;
        assert!(matches!(events.last(), Some(AgentEvent::Error { message, .. }) if message.contains("No Anthropic API key")), "{events:?}");
    }
    s.update_settings(|st| st.agent.permissions.enabled = false).unwrap();
    let mut run = Agent::start(&s, config, "Hi", Conversation::new());
    let events = collect(&mut run).await;
    assert!(matches!(events.last(), Some(AgentEvent::Error { message, .. }) if message.contains("turned off")), "{events:?}");
}

#[test]
fn tools_cover_the_registry_except_person_only_commands() {
    let defs = tool_defs();
    assert!(defs.iter().any(|t| t.name == "doc_overview"));
    assert!(defs.iter().any(|t| t.name == "page_look"));
    assert!(defs.iter().all(|t| t.name != "agent_setKey" && !t.name.starts_with("agent_")));
    let mut names: Vec<&str> = defs.iter().map(|t| t.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), defs.len());
    for t in &defs {
        assert!(t.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && t.name.len() <= 64, "{}", t.name);
        assert_eq!(t.schema["type"], "object");
        assert_eq!(tools::spec_for_tool(&t.name).unwrap().name, t.command);
    }
    assert_eq!(tools::spec_for_tool("mcp__nori__layer_add").unwrap().name, "layer.add");
    let long = "é".repeat(TOOL_OUTPUT_LIMIT);
    let (out, err) = tools::tool_output(&Ok(json!(long)));
    assert!(!err && out.contains("truncated") && out.len() < TOOL_OUTPUT_LIMIT + 200);
}

#[test]
fn trimmed_tool_sets_keep_the_core_and_name_real_commands() {
    let set = ToolSet::new(Some(64), false);
    assert!(set.trimmed && set.defs.len() <= 64);
    assert_eq!(set.defs.last().unwrap().name, RUN_TOOL);
    assert!(set.defs.iter().any(|t| t.name == "doc_overview"));
    let compact = ToolSet::new(None, true);
    assert!(compact.defs.len() < 30);
    assert!(compact.system_prompt().contains(RUN_TOOL));
    assert!(!ToolSet::new(None, false).trimmed);
    // Every command named in the lists exists (a renamed command would silently drop out).
    for name in tools::CORE.iter().chain(tools::COMPACT) {
        assert!(nori_control::spec(name).is_some(), "{name} is in a tool list but not in the registry");
    }
    let core = ToolSet::new(Some(128), false);
    assert!(core.defs.len() <= 128);
}

#[test]
fn settings_choose_the_provider() {
    let mut a = nori_control::settings::AgentSettings::default();
    assert_eq!(AgentConfig::from_settings(&a).provider, ProviderKind::ClaudeCode);
    a.provider = "anthropic".into();
    let c = AgentConfig::from_settings(&a);
    assert_eq!((c.provider, c.model(), c.base_url()), (ProviderKind::Anthropic, "claude-sonnet-5-5".to_string(), "https://api.anthropic.com".to_string()));
    a.provider = "ollama".into();
    a.base_url = "http://box:11434/".into();
    assert_eq!(AgentConfig::from_settings(&a).base_url(), "http://box:11434");
    a.provider = "nonsense".into();
    assert_eq!(AgentConfig::from_settings(&a).provider, ProviderKind::ClaudeCode);
    assert_eq!(serde_json::to_value(ProviderKind::OpenAi).unwrap(), "openai");
}

#[test]
fn provider_ids_parse_from_what_people_type() {
    assert_eq!(ProviderKind::parse("Claude"), Some(ProviderKind::ClaudeCode));
    assert_eq!(ProviderKind::parse("lsuite AI"), None, "lsuite AI is gone");
    assert_eq!(ProviderKind::parse("lm studio"), Some(ProviderKind::OpenAiCompatible));
    assert_eq!(ProviderKind::parse("claude_code"), Some(ProviderKind::ClaudeCode));
    assert_eq!(ProviderKind::parse("gemini"), None);
    assert_eq!(serde_json::to_value(ProviderKind::OpenAiCompatible).unwrap(), "openai-compatible");
    // Conversations saved with lsuite AI (Anthropic's API) still load.
    let saved: Part = serde_json::from_value(json!({ "type": "opaque", "provider": "lsuite", "block": { "type": "thinking" } })).unwrap();
    assert!(matches!(saved, Part::Opaque { provider: ProviderKind::Anthropic, .. }));
    let key_ids: Vec<&str> = ProviderKind::ALL.iter().filter_map(|k| k.info().key.map(|k| k.id)).collect();
    assert_eq!(key_ids, nori_control::commands::agent::AGENT_KEY_IDS, "the agent and agent.setKey keep keys under the same ids");
    for id in nori_control::commands::agent::AGENT_KEY_IDS {
        let env = nori_control::secrets::env_var(id);
        assert_eq!(ProviderKind::parse(id).unwrap().info().key.unwrap().env.first().copied(), env, "{id}: the same environment variable as the key store");
    }
}

#[test]
fn a_stopped_turn_is_closed_before_the_next() {
    let mut c = Conversation::new();
    c.messages.push(Message::user("go"));
    c.messages.push(Message {
        role: Role::Assistant,
        parts: vec![
            Part::ToolUse { id: "a".into(), name: "layer_list".into(), input: json!({}) },
            Part::ToolUse { id: "b".into(), name: "layer_list".into(), input: json!({}) },
        ],
    });
    c.messages.push(Message { role: Role::User, parts: vec![Part::ToolResult { id: "a".into(), name: "layer_list".into(), output: "[]".into(), is_error: false }] });
    c.prepare_turn();
    assert_eq!(c.messages.len(), 3);
    assert!(matches!(&c.messages[2].parts[1], Part::ToolResult { id, is_error: true, .. } if id == "b"));
}

#[test]
fn cli_invocations_attach_nori_only() {
    let args = cli::claude_args(std::path::Path::new("/tmp/mcp.json"), "", Some("sess-1"), false);
    let joined = args.join(" ");
    for flag in ["-p", "--output-format stream-json", "--strict-mcp-config", "--allowedTools mcp__nori__*", "--mcp-config /tmp/mcp.json", "--resume sess-1"] {
        assert!(joined.contains(flag), "{flag} in {joined}");
    }
    assert!(!joined.contains("--model"));
    let live = cli::Live { mcp: "/Apps/nori \"x\"/nori-mcp".into(), control: "/data/control.json".into() };
    let config = cli::mcp_config(&live);
    assert_eq!(config["mcpServers"]["nori"]["env"], json!({ "NORI_CONTROL": "/data/control.json", "NORI_MCP_BUILTIN_AGENT": "1" }));
    let args = cli::codex_args(&live, "gpt-5", Some("thread-9"), &["node_repl".into(), "nori".into()]);
    assert_eq!(&args[..2], ["exec", "--json"]);
    for c in ["features.plugins=false", "features.computer_use=false", "mcp_servers.node_repl.enabled=false", "mcp_servers.nori.env.NORI_MCP_BUILTIN_AGENT=\"1\""] {
        assert!(args.windows(2).any(|w| w == ["-c", c]), "{c} in {args:?}");
    }
    assert!(!args.iter().any(|a| a == "mcp_servers.nori.enabled=false"));
    assert!(args.windows(2).any(|w| w == ["-c", "mcp_servers.nori.default_tools_approval_mode=\"approve\""]), "Codex runs nori's tools without asking");
    assert!(args.contains(&"mcp_servers.nori.command=\"/Apps/nori \\\"x\\\"/nori-mcp\"".to_string()), "{args:?}");
    assert!(args.windows(2).any(|w| w == ["resume", "thread-9"]));
    assert_eq!(args.last().unwrap(), "-");
}

#[test]
fn a_batch_shim_gets_the_system_prompt_on_one_line() {
    let args = cli::claude_args(std::path::Path::new("/tmp/mcp.json"), "", None, true);
    let system = &args[args.iter().position(|a| a == "--append-system-prompt").unwrap() + 1];
    assert!(!system.contains('\n') && system.contains("mcp__nori__family_verb"), "{system}");
}

#[test]
fn codex_config_servers_are_found() {
    let config = r#"
model = "gpt-5"
[mcp_servers.node_repl]
command = "node"
[mcp_servers.node_repl.env]
X = "1"
[mcp_servers."my.server"]
command = "x"
[projects."/home/me"]
trust_level = "trusted"
[mcp_servers]
inline = { command = "y" }
"#;
    assert_eq!(cli::codex_mcp_servers(config), ["node_repl", "\"my.server\"", "inline"]);
}

#[test]
fn children_get_a_path_with_the_cli_folder_first() {
    let path = cli::child_path(std::path::Path::new("/opt/somewhere/bin/claude"));
    let dirs: Vec<_> = std::env::split_paths(&path).collect();
    assert_eq!(dirs[0], std::path::Path::new("/opt/somewhere/bin"));
    for d in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|d| !d.as_os_str().is_empty()) {
        assert!(dirs.contains(&d), "{d:?} kept");
    }
}

#[cfg(unix)]
fn script(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("fake-cli");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn claude_code_stream_json_becomes_events() {
    let dir = tempfile::tempdir().unwrap();
    let s = session(dir.path());
    let lines = [
        json!({ "type": "system", "subtype": "init", "session_id": "sess-1", "mcp_servers": [{ "name": "nori", "status": "connected" }] }),
        json!({ "type": "stream_event", "event": { "type": "message_start" } }),
        json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Hel" } } }),
        json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "lo" } } }),
        json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Hello" }] } }),
        json!({ "type": "result", "subtype": "success", "is_error": false, "result": "Hello", "session_id": "sess-1", "usage": { "input_tokens": 3, "output_tokens": 2 } }),
    ];
    let echo: String = lines.iter().map(|l| format!("echo '{l}'\n")).collect();
    let got = dir.path().join("prompt.txt");
    let exe = script(dir.path(), &format!("cat > '{}'\n{echo}", got.display()));
    let (mut run, mut handle) = Run::new(&s, AgentConfig::new(ProviderKind::ClaudeCode), &Conversation::new());
    let (out, result) = cli::run_child(&mut run, tokio::process::Command::new(exe), "the prompt".into(), "Claude Code", cli::parse_claude).await;
    result.unwrap();
    assert_eq!((out.reply.as_str(), out.session_id.as_deref()), ("Hello", Some("sess-1")));
    assert_eq!(std::fs::read_to_string(got).unwrap(), "the prompt");
    drop(run);
    let mut deltas = vec![];
    while let Some(e) = handle.next_event().await {
        if let AgentEvent::Text { delta } = e {
            deltas.push(delta);
        }
    }
    assert_eq!(deltas, ["Hel", "lo"]);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn cancelling_kills_the_cli_and_its_children() {
    let dir = tempfile::tempdir().unwrap();
    let s = session(dir.path());
    let pid_file = dir.path().join("pid");
    let exe = script(dir.path(), &format!("sleep 60 &\necho $! > '{}'\nwait", pid_file.display()));
    let (mut run, _handle) = Run::new(&s, AgentConfig::new(ProviderKind::ClaudeCode), &Conversation::new());
    let child = cli::run_child(&mut run, tokio::process::Command::new(exe), String::new(), "Claude Code", cli::parse_claude);
    assert!(tokio::time::timeout(Duration::from_millis(800), child).await.is_err(), "the fake CLI waits");
    let pid = std::fs::read_to_string(&pid_file).unwrap().trim().to_string();
    let alive = || std::process::Command::new("kill").args(["-0", &pid]).stderr(std::process::Stdio::null()).status().unwrap().success();
    for _ in 0..40 {
        if !alive() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the CLI's child {pid} survived the cancel");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cli_without_nori_tools_stops_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let s = session(dir.path());
    let init = json!({ "type": "system", "subtype": "init", "session_id": "s", "mcp_servers": [{ "name": "nori", "status": "failed" }] });
    let exe = script(dir.path(), &format!("echo '{init}'\nsleep 60"));
    let (mut run, _handle) = Run::new(&s, AgentConfig::new(ProviderKind::ClaudeCode), &Conversation::new());
    let child = cli::run_child(&mut run, tokio::process::Command::new(exe), String::new(), "Claude Code", cli::parse_claude);
    let (_, result) = tokio::time::timeout(Duration::from_secs(5), child).await.expect("stopped at once");
    assert!(result.unwrap_err().contains("couldn't start nori-mcp"));
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_status_says_what_is_usable() {
    let dir = tempfile::tempdir().unwrap();
    let s = session(dir.path());
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "models": [{ "name": "qwen3:8b" }] })))
        .mount(&server)
        .await;
    s.update_settings(|st| {
        st.agent.provider = "ollama".into();
        st.agent.base_url = server.uri();
    })
    .unwrap();
    s.set_secret("openai", Some("sk-test")).unwrap();
    let all = provider_status(&s).await;
    assert_eq!(all.len(), ProviderKind::ALL.len());
    assert_eq!(all[0].provider, ProviderKind::ClaudeCode, "the CLIs come first");
    let get = |k| all.iter().find(|p| p.provider == k).unwrap();
    let ollama = get(ProviderKind::Ollama);
    assert!(ollama.ready && ollama.active && ollama.on_device, "{ollama:?}");
    assert_eq!(ollama.models, ["qwen3:8b"]);
    assert!(get(ProviderKind::OpenAi).ready);
    // No bridge in a headless session: the CLIs can't reach nori, whatever is installed.
    let claude = get(ProviderKind::ClaudeCode);
    assert!(!claude.ready && !claude.message.is_empty(), "{claude:?}");
}

// ---- the host -------------------------------------------------------------------------------

/// A session with the host installed (and a stand-in window: `agent.*` needs the app).
async fn hosted(dir: &std::path::Path, server: &MockServer) -> Arc<Session> {
    use futures::StreamExt;
    let s = with_doc(dir).await;
    s.set_secret("anthropic", Some("sk-ant-test")).unwrap();
    s.update_settings(|st| {
        st.agent.provider = "anthropic".into();
        st.agent.base_url = server.uri();
    })
    .unwrap();
    let mut calls = s.attach_ui();
    tokio::spawn(async move { while calls.next().await.is_some() {} });
    Host::install(&s);
    s
}

async fn call(s: &Arc<Session>, source: Source, name: &str, params: Value) -> Value {
    nori_control::call(s, source, name, params).await.unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// `agent.*` from the CLI: send and wait, read the run with its commands and the conversation,
/// revert it; the next request continues the thread.
#[tokio::test(flavor = "multi_thread")]
async fn clients_drive_the_agent_with_commands() {
    let dir = tempfile::tempdir().unwrap();
    let server = mock(
        "/v1/messages",
        "text/event-stream",
        vec![anthropic_tool("toolu_1", "layer_add", "{\"name\": \"Hello\"}"), anthropic_text("Added Hello."), anthropic_text("You're welcome.")],
    )
    .await;
    let s = hosted(dir.path(), &server).await;
    // A command from a terminal before the run shows as a card of its own.
    call(&s, Source::Cli, "layer.add", json!({ "name": "Beat" })).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while call(&s, Source::Cli, "agent.conversation", json!({})).await["entries"] == json!([]) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the layer's card");

    let run = call(&s, Source::Cli, "agent.send", json!({ "prompt": "Add a layer called Hello", "wait": true })).await;
    assert_eq!(run["state"], "done", "{run}");
    assert_eq!(run["source"], "cli");
    assert_eq!(run["reply"], "Added Hello.");
    assert_eq!(run["changes"], 1);
    assert_eq!(run["canRevert"], true);
    let list = run["commandList"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{run}");
    assert_eq!(list[0]["command"], "layer.add");
    assert_eq!(layer_names(&s)[..2], ["Hello", "Beat"]);

    let status = call(&s, Source::Mcp, "agent.status", json!({})).await;
    assert_eq!(status["id"], run["id"]);
    assert_eq!(status["running"], Value::Null);
    let convo = call(&s, Source::Cli, "agent.conversation", json!({})).await;
    let kinds: Vec<&str> = convo["entries"].as_array().unwrap().iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["command", "user", "command", "assistant", "outcome"], "{convo}");
    assert_eq!(convo["entries"][0]["run"], Value::Null, "the terminal's layer is nobody's run");
    let next = convo["next"].as_u64().unwrap();
    assert_eq!(call(&s, Source::Cli, "agent.conversation", json!({ "since": next })).await["entries"], json!([]));

    let reverted = call(&s, Source::Cli, "agent.revert", json!({})).await;
    assert_eq!(reverted["run"], run["id"]);
    assert_eq!(layer_names(&s)[0], "Beat", "only the run is reverted");
    let e = nori_control::call(&s, Source::Cli, "agent.revert", json!({})).await.unwrap_err();
    assert!(e.contains("No agent run to revert"), "{e}");
    // Undoing the revert brings the run back, and it can be reverted again (redo too).
    let reverted = |s: &Arc<Session>, want: bool| {
        let s = s.clone();
        async move {
            tokio::time::timeout(Duration::from_secs(5), async {
                while call(&s, Source::Cli, "agent.runs", json!({})).await["runs"][0]["reverted"] != json!(want) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .is_ok()
        }
    };
    call(&s, Source::Window, "history.undo", json!({})).await;
    assert_eq!(layer_names(&s)[0], "Hello");
    assert!(reverted(&s, false).await, "the run is back");
    call(&s, Source::Cli, "history.redo", json!({})).await;
    assert!(reverted(&s, true).await, "and reverted again by the redo");
    call(&s, Source::Cli, "history.undo", json!({})).await;
    assert!(reverted(&s, false).await);
    call(&s, Source::Cli, "agent.revert", json!({})).await;
    assert_eq!(layer_names(&s)[0], "Beat");

    let follow = call(&s, Source::Window, "agent.send", json!({ "prompt": "Thanks", "wait": true })).await;
    assert_eq!(follow["reply"], "You're welcome.");
    let body: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[2].body).unwrap();
    assert_eq!(body["messages"].as_array().unwrap().len(), 5, "the thread goes on");
    assert!(body["tools"].as_array().unwrap().iter().all(|t| !t["name"].as_str().unwrap().starts_with("agent_")), "the agent doesn't drive itself");
    let runs = call(&s, Source::Cli, "agent.runs", json!({})).await;
    assert_eq!(runs["runs"].as_array().unwrap().len(), 2);

    // A new conversation is separate; the previous one stays selectable.
    let previous = call(&s, Source::Cli, "agent.conversations", json!({})).await["current"].clone();
    call(&s, Source::Cli, "agent.newConversation", json!({})).await;
    assert_eq!(call(&s, Source::Cli, "agent.conversation", json!({})).await["entries"], json!([]));
    assert_eq!(call(&s, Source::Cli, "agent.runs", json!({})).await["runs"], json!([]));
    call(&s, Source::Cli, "agent.selectConversation", json!({ "id": previous })).await;
    assert_eq!(call(&s, Source::Cli, "agent.runs", json!({})).await["runs"].as_array().unwrap().len(), 2);
}

/// One run at a time, stopped from another client; agents can't choose their own provider.
#[tokio::test(flavor = "multi_thread")]
async fn one_run_at_a_time_and_permissions_hold() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60))).mount(&server).await;
    let s = hosted(dir.path(), &server).await;
    let run = call(&s, Source::Window, "agent.send", json!({ "prompt": "Something slow" })).await;
    assert_eq!(run["state"], "running");
    let e = nori_control::call(&s, Source::Cli, "agent.send", json!({ "prompt": "Me too" })).await.unwrap_err();
    assert!(e.contains("still working on run"), "{e}");
    let e = nori_control::call(&s, Source::Cli, "agent.newConversation", json!({})).await.unwrap_err();
    assert!(e.contains("stop it first"), "{e}");
    call(&s, Source::Mcp, "agent.stop", json!({})).await;
    let done = call(&s, Source::Cli, "agent.status", json!({ "wait": true, "timeout": 10 })).await;
    assert_eq!(done["state"], "cancelled", "{done}");

    let e = nori_control::call(&s, Source::Mcp, "agent.setProvider", json!({ "provider": "ollama" })).await.unwrap_err();
    assert!(e.contains("stays with the person"), "{e}");
    let e = nori_control::call(&s, Source::Mcp, "agent.setMemory", json!({ "text": "x" })).await.unwrap_err();
    assert!(e.contains("stays with the person"), "{e}");
    s.update_settings(|st| st.agent.permissions.enabled = false).unwrap();
    let e = nori_control::call(&s, Source::Mcp, "agent.send", json!({ "prompt": "Hi" })).await.unwrap_err();
    assert!(e.contains("turned off"), "{e}");
    s.update_settings(|st| st.agent.permissions.enabled = true).unwrap();
    // The person picks the provider; every list answers the same ids.
    let v = call(&s, Source::Cli, "agent.setProvider", json!({ "provider": "codex" })).await;
    assert_eq!(v["provider"], "codex");
    let providers = call(&s, Source::Cli, "agent.providers", json!({})).await;
    let ids: Vec<&str> = providers["providers"].as_array().unwrap().iter().map(|p| p["provider"].as_str().unwrap()).collect();
    assert_eq!(ids, nori_control::settings::AGENT_PROVIDERS);
    assert_eq!(providers["groups"][0], json!({ "id": "cli", "label": "On this computer" }));
    let e = nori_control::call(&s, Source::Cli, "agent.models", json!({ "provider": "codexx" })).await.unwrap_err();
    assert!(e.contains("Did you mean codex?"), "{e}");
}

/// Another document starts another conversation: a run going on is stopped.
#[tokio::test(flavor = "multi_thread")]
async fn switching_documents_starts_afresh() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60))).mount(&server).await;
    let s = hosted(dir.path(), &server).await;
    call(&s, Source::Window, "agent.send", json!({ "prompt": "Something slow" })).await;
    call(&s, Source::Window, "doc.new", json!({ "name": "Other" })).await;
    let runs = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let v = call(&s, Source::Cli, "agent.runs", json!({})).await;
            if v["running"].is_null() {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the run stops");
    assert_eq!(runs["runs"], json!([]));
    assert_eq!(call(&s, Source::Cli, "agent.conversation", json!({})).await["entries"], json!([]));
}

/// Conversations and memory belong to the document: saved, they follow it to its file; they
/// survive a restart of the host; another document has its own; opening the file brings them back.
#[tokio::test(flavor = "multi_thread")]
async fn conversations_and_memory_stay_with_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let server = mock("/v1/messages", "text/event-stream", vec![anthropic_text("Planned the poster.")]).await;
    let s = hosted(dir.path(), &server).await;
    call(&s, Source::Window, "agent.setMemory", json!({ "text": "Keep the palette warm." })).await;
    let first = call(&s, Source::Cli, "agent.conversations", json!({})).await["current"].clone();
    call(&s, Source::Window, "agent.send", json!({ "prompt": "Plan the poster", "wait": true })).await;
    let body: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert!(body.to_string().contains("Keep the palette warm."));
    // Saved for the first time: the conversation moves from the name to the file.
    let file = dir.path().join("poster.nori");
    call(&s, Source::Window, "doc.save", json!({ "path": file })).await;
    let stored = || std::fs::read_to_string(s.data_dir.join("agent/conversations.json")).unwrap_or_default();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !stored().contains("poster.nori") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the conversation is kept under the file");
    call(&s, Source::Window, "agent.newConversation", json!({})).await;
    call(&s, Source::Window, "agent.renameConversation", json!({ "title": "Type" })).await;

    let host = Host::install(&s);
    assert_eq!(host.snapshot().conversation.title, "Type");
    assert_eq!(host.snapshot().conversations.len(), 2);
    assert_eq!(host.snapshot().memory, "Keep the palette warm.");
    call(&s, Source::Window, "agent.selectConversation", json!({ "id": first })).await;
    assert!(host.snapshot().entries.iter().any(|e| matches!(e, Entry::Assistant { text, .. } if text == "Planned the poster.")));
    assert_eq!(host.snapshot().conversation.title, "Plan the poster");
    assert!(host.snapshot().runs.iter().all(|r| r.checkpoint.is_none()), "checkpoints don't survive a restart");

    call(&s, Source::Window, "doc.new", json!({ "name": "Separate" })).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while host.snapshot().conversation.id.to_string() == first.as_str().unwrap() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(host.snapshot().memory.is_empty());
    assert!(nori_control::call(&s, Source::Window, "agent.selectConversation", json!({ "id": first })).await.is_err());
    call(&s, Source::Window, "doc.open", json!({ "path": file })).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while host.snapshot().memory.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(host.snapshot().conversations.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn steering_interrupts_a_model_request_and_keeps_the_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)).set_body_string(anthropic_text("Old direction.")))
        .mount(&server)
        .await;
    let s = hosted(dir.path(), &server).await;
    let run = call(&s, Source::Window, "agent.send", json!({ "prompt": "Make a red title" })).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while server.received_requests().await.unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(anthropic_text("Using blue instead.")))
        .mount(&server)
        .await;
    let steer = call(&s, Source::Window, "agent.steer", json!({ "prompt": "Use blue instead" })).await;
    assert_eq!(steer["run"], run["id"]);
    let done = call(&s, Source::Window, "agent.status", json!({ "wait": true, "timeout": 5 })).await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["reply"], "Using blue instead.");
    let body: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert!(body.to_string().contains("Make a red title"));
    assert!(body.to_string().contains("Use blue instead"));
}

#[tokio::test(flavor = "multi_thread")]
async fn unreadable_history_is_never_overwritten() {
    use futures::StreamExt;
    let dir = tempfile::tempdir().unwrap();
    let s = with_doc(dir.path()).await;
    let mut calls = s.attach_ui();
    tokio::spawn(async move { while calls.next().await.is_some() {} });
    let path = s.data_dir.join("agent").join("conversations.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{damaged but recoverable history").unwrap();
    let host = Host::install(&s);
    assert!(host.snapshot().storage_error.is_some());
    call(&s, Source::Window, "agent.setMemory", json!({ "text": "New memory" })).await;
    call(&s, Source::Window, "agent.newConversation", json!({})).await;
    assert_eq!(std::fs::read(&path).unwrap(), b"{damaged but recoverable history");
    assert!(host.snapshot().storage_error.is_some());
}

// ---- what the person sees, and pictures -----------------------------------------------------

#[test]
fn the_context_block_frames_a_request_and_comes_off_again() {
    let g = Glance { lines: vec!["What the person sees:".into(), "Active layer: \"Sky\".".into()], short: "\"Sky\"".into(), seq: 0, changes: vec![] };
    let framed = g.frame("Make it darker");
    assert_eq!(framed, "<context>\nWhat the person sees:\nActive layer: \"Sky\".\n</context>\n\nMake it darker");
    assert_eq!(context::unframed(&framed), "Make it darker");
    assert_eq!(context::unframed("no block"), "no block");
    assert_eq!(Glance::default().frame("as is"), "as is");
}

/// A red fill layer named Red, active, with a selection, the brush in hand.
async fn red_and_selected(dir: &std::path::Path) -> (Arc<Session>, String) {
    let s = with_doc(dir).await;
    let added = nori_control::call(&s, Source::Window, "layer.add", json!({ "kind": "fill", "color": "#ff0000", "name": "Red" })).await.unwrap();
    let id = nori_control::registry::created_layers(&added)[0].clone();
    nori_control::call(&s, Source::Window, "select.rect", json!({ "x": 10, "y": 20, "width": 100, "height": 50 })).await.unwrap();
    s.set_ui_state(nori_control::UiState { screen: "editor".into(), tool: "brush".into(), zoom: 0.5, open: vec!["agent".into(), "export".into()], ..Default::default() });
    (s, id)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_is_told_what_the_person_sees() {
    let dir = tempfile::tempdir().unwrap();
    let (s, id) = red_and_selected(dir.path()).await;
    let g = glance(&s);
    let block = g.lines.join("\n");
    assert!(block.contains("Document \"Test\": page 1 of 1 (\"Page 1\""), "{block}");
    assert!(block.contains("400×300 px"), "{block}");
    assert!(block.contains("not saved yet"), "{block}");
    assert!(block.contains(&format!("Active layer: \"Red\" ({id}, colour fill #ff0000")), "{block}");
    assert!(block.contains("Selection: at 10, 20, 100×50"), "{block}");
    assert!(block.contains("Window: tool brush, zoom 50 %."), "{block}");
    assert!(block.contains("Also open: export.") && !block.contains("agent,"), "{block}");
    assert_eq!(g.short, "Selection 100×50 · 400×300");
    nori_control::call(&s, Source::Window, "doc.close", json!({"discard":true})).await.unwrap();
    assert_eq!(glance(&s).short, "No document open");
}

#[tokio::test(flavor = "multi_thread")]
async fn anthropic_sees_the_page_it_looks_at() {
    let dir = tempfile::tempdir().unwrap();
    let (s, id) = red_and_selected(dir.path()).await;
    s.set_secret("anthropic", Some("sk-ant-test")).unwrap();
    let server = mock("/v1/messages", "text/event-stream", vec![anthropic_tool("toolu_1", "page_look", "{\"width\": 200}"), anthropic_text("It is red.")]).await;
    let config = AgentConfig { base_url: server.uri(), ..AgentConfig::new(ProviderKind::Anthropic) };
    let mut run = Agent::start(&s, config, "What colour is this?", Conversation::new());
    let events = collect(&mut run).await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })), "{events:#?}");

    let requests = server.received_requests().await.unwrap();
    let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
    let asked = first["messages"][0]["content"][0]["text"].as_str().unwrap();
    assert!(asked.starts_with("<context>\n") && asked.contains(&id) && asked.ends_with("</context>\n\nWhat colour is this?"), "{asked}");
    let second: Value = serde_json::from_slice(&requests[1].body).unwrap();
    let result = &second["messages"][2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert!(result["content"][0]["text"].as_str().unwrap().ends_with("The picture is attached."), "{result}");
    let image = &result["content"][1];
    assert_eq!(image["type"], "image");
    assert_eq!(image["source"]["media_type"], "image/png");
    // Base64 of a PNG's signature.
    assert!(image["source"]["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    // The picture stays in the thread, inside its result, for the next turn.
    let conv = run.conversation();
    assert!(conv.messages[2].parts.iter().any(|p| matches!(p, Part::Image { call: Some(c), .. } if c == "toolu_1")));
}

/// Answers in order with (status, body); the last one repeats.
struct Replies(Vec<(u16, String)>, AtomicUsize);

impl Respond for Replies {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let i = self.1.fetch_add(1, Ordering::SeqCst).min(self.0.len() - 1);
        let (status, body) = &self.0[i];
        ResponseTemplate::new(*status).insert_header("content-type", if *status == 200 { "text/event-stream" } else { "application/json" }).set_body_string(body.clone())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_openai_compatible_server_without_vision_gets_no_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = red_and_selected(dir.path()).await;
    let call = openai_sse(&[
        json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_1", "function": { "name": "page_look", "arguments": "{\"width\": 200}" } }] } }] }),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] }),
    ]);
    let answer = openai_sse(&[
        json!({ "choices": [{ "index": 0, "delta": { "content": "Red." } }] }),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] }),
    ]);
    let refusal = json!({ "error": { "message": "This model does not support image input." } }).to_string();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(Replies(vec![(200, call), (400, refusal), (200, answer)], AtomicUsize::new(0)))
        .mount(&server)
        .await;
    let config = AgentConfig { base_url: format!("{}/v1", server.uri()), model: "local-model".into(), ..AgentConfig::new(ProviderKind::OpenAiCompatible) };
    let mut run = Agent::start(&s, config, "Look at it", Conversation::new());
    let events = collect(&mut run).await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { summary, .. }) if summary == "Red."), "{events:#?}");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    let with: Value = serde_json::from_slice(&requests[1].body).unwrap();
    let pictures = with["messages"].as_array().unwrap().iter().find(|m| m["role"] == "user" && m["content"].is_array()).expect("the picture follows the tool answer");
    assert!(pictures["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
    let without: Value = serde_json::from_slice(&requests[2].body).unwrap();
    assert!(!without.to_string().contains("image_url"), "sent again without the picture");
    assert!(without.to_string().contains("earlier picture is not shown again"));
}
