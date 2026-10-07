//! `nori-mcp`: nori as a Model Context Protocol server over stdio (newline-delimited
//! JSON-RPC 2.0). Every registry command is a tool (`layer.add` becomes `layer_add`), with
//! its description and JSON schema taken from the registry, so the tools can't drift from what
//! the window, the CLI and the built-in agent accept.
//!
//! In live mode each call runs in the open nori window through its loopback bridge, so an
//! agent and the person edit the same document with one undo history. With `--file` the server
//! hosts a session on a file and saves after every change. MCP requests are always held
//! to Settings › Agent › Permissions. Only protocol goes to stdout; logs go to stderr.
//!
//! Requests are served concurrently: each one is a task, so `ping` is answered while a
//! filter runs, and `notifications/cancelled` aborts the request it names. One writer
//! thread owns stdout, one JSON line at a time.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nori_cli::Backend;
use nori_control::{Perm, Source, harness, registry};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

mod prompts;

/// Protocol revisions we speak, oldest first; an unknown request gets the newest.
const PROTOCOLS: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
const MAX_LINE: usize = 64 * 1024 * 1024;

const USAGE: &str = "nori-mcp — Model Context Protocol server for nori (stdio)

USAGE
  nori-mcp                  drive the running nori app; if it isn't running, host a session
                            in this process
  nori-mcp --live           drive the running app only (calls fail with a hint while it is closed)
  nori-mcp --file <path>    host that file in this process and save after every change (a .nori to
                            itself, another file to the .nori beside it); doc_new makes it
  nori-mcp --headless       host a session in this process (doc_open or doc_new first)

Register it with an MCP client, for example Claude Code:
  claude mcp add nori -- /path/to/nori-mcp --live
`nori-cli mcp-config` prints this line and the others with this computer's paths.";

#[tokio::main]
async fn main() {
    let mut file: Option<PathBuf> = None;
    let (mut live, mut headless) = (false, false);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            "--version" | "-V" => {
                println!("nori-mcp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--file" | "-f" => match args.next().filter(|p| !p.starts_with("--")) {
                Some(p) => file = Some(PathBuf::from(p)),
                None => exit_usage("--file needs a path"),
            },
            "--live" => live = true,
            "--headless" => headless = true,
            _ => exit_usage(&format!("Unknown option `{arg}`")),
        }
    }
    if usize::from(file.is_some()) + usize::from(live) + usize::from(headless) > 1 {
        exit_usage("--file, --live and --headless are exclusive");
    }
    let backend = match (file, live, headless) {
        (Some(path), _, _) => Backend::file(&path, Source::Mcp).await,
        (None, true, _) => {
            if let Err(e) = Backend::live("mcp").await {
                eprintln!("nori-mcp: {e}\nnori-mcp: waiting for the app; each call tries again");
            }
            Ok(Backend::live_lazy("mcp"))
        }
        (None, false, true) => Backend::headless(Source::Mcp).await,
        (None, false, false) => match Backend::live("mcp").await {
            Ok(b) => Ok(b),
            Err(e) => {
                eprintln!("nori-mcp: {e}\nnori-mcp: hosting a session in this process instead");
                Backend::headless(Source::Mcp).await
            }
        },
    };
    let backend = match backend {
        Ok(b) => b,
        Err(e) => {
            eprintln!("nori-mcp: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("nori-mcp {}: {} mode{}", env!("CARGO_PKG_VERSION"), backend.mode(), backend.path().map(|p| format!(" on {}", p.display())).unwrap_or_default());

    let (out, writer) = protocol_out();
    let server = Arc::new(Server { backend, context: Mutex::new(None) });
    // Requests in flight by id (as JSON text), to cancel them.
    let running: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>> = Arc::default();
    let mut stdin = tokio::io::BufReader::with_capacity(1 << 16, tokio::io::stdin());
    loop {
        let line = match read_line(&mut stdin, MAX_LINE).await {
            Ok(Line::Text(l)) => l,
            Ok(Line::TooLong) => {
                eprintln!("nori-mcp: skipped a request over 64 MiB");
                out.send(error(Value::Null, -32600, "The request exceeds 64 MiB"));
                continue;
            }
            Ok(Line::Eof) => break,
            Err(e) => {
                eprintln!("nori-mcp: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let frame = match server.frame(&line) {
            Ok(f) => f,
            Err(reply) => {
                out.send(reply);
                continue;
            }
        };
        let Some(Frame { id, method, params }) = frame else { continue };
        let Some(id) = id else {
            // Notifications get no answer; a cancelled request gets none either.
            if method == "notifications/cancelled"
                && let Some(task) = params.get("requestId").and_then(|r| running.lock().unwrap_or_else(|e| e.into_inner()).remove(&r.to_string()))
            {
                task.abort();
            }
            continue;
        };
        let key = id.to_string();
        let (server, out, done) = (server.clone(), out.clone(), running.clone());
        // Held while spawning, so a quick task can't remove its entry before it is there.
        let mut tasks = running.lock().unwrap_or_else(|e| e.into_inner());
        let task_key = key.clone();
        let task = tokio::spawn(async move {
            let reply = match server.dispatch(&method, &params).await {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err((code, message)) => error(id, code, &message),
            };
            done.lock().unwrap_or_else(|e| e.into_inner()).remove(&task_key);
            out.send(reply);
        });
        tasks.insert(key, task);
    }
    // The client is gone; let calls already running finish (a file-mode edit saves its file),
    // up to a few seconds.
    let left: Vec<_> = running.lock().unwrap_or_else(|e| e.into_inner()).drain().map(|(_, t)| t).collect();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for t in left {
            let _ = t.await;
        }
    })
    .await;
    // Out with the answers already given, then stop (a call still running keeps a sender: bounded).
    drop(out);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), tokio::task::spawn_blocking(move || writer.join())).await;
}

/// One line of input, read without ever holding more than `max` bytes of it.
#[derive(Debug, PartialEq)]
enum Line {
    Text(String),
    /// Over `max`: skipped up to its end.
    TooLong,
    Eof,
}

async fn read_line(r: &mut (impl AsyncBufRead + Unpin), max: usize) -> std::io::Result<Line> {
    let mut buf: Vec<u8> = vec![];
    let mut over = false;
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(match (over, buf.is_empty()) {
                (true, _) => Line::TooLong,
                (false, true) => Line::Eof,
                (false, false) => Line::Text(String::from_utf8_lossy(&buf).into_owned()),
            });
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let take = newline.map_or(chunk.len(), |i| i + 1);
        if !over && buf.len() + take > max + 1 {
            over = true;
            buf = vec![];
        }
        if !over {
            buf.extend_from_slice(&chunk[..take]);
        }
        r.consume(take);
        if newline.is_some() {
            if over {
                return Ok(Line::TooLong);
            }
            while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                buf.pop();
            }
            return Ok(Line::Text(String::from_utf8_lossy(&buf).into_owned()));
        }
    }
}

/// The protocol's way out: a thread that writes one JSON line per message. When stdout is gone
/// the client is too, and the server stops.
#[derive(Clone)]
struct Out(std::sync::mpsc::Sender<Value>);

impl Out {
    fn send(&self, v: Value) {
        let _ = self.0.send(v);
    }
}

fn protocol_out() -> (Out, std::thread::JoinHandle<()>) {
    let mut stdout = protocol_stdout();
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let writer = std::thread::spawn(move || {
        for v in rx {
            let written = serde_json::to_writer(&mut stdout, &v).map_err(std::io::Error::other).and_then(|()| stdout.write_all(b"\n")).and_then(|()| stdout.flush());
            if written.is_err() {
                std::process::exit(0);
            }
        }
    });
    (Out(tx), writer)
}

/// stdout for the protocol only. On Unix the real stdout is kept aside and fd 1 points at
/// stderr from here on, so a stray print (ours, a library's or a child process's, like ffmpeg)
/// can never corrupt the JSON-RPC stream.
#[cfg(unix)]
fn protocol_stdout() -> Box<dyn Write + Send> {
    use std::os::fd::FromRawFd;
    unsafe extern "C" {
        fn dup(fd: i32) -> i32;
        fn dup2(old: i32, new: i32) -> i32;
    }
    // SAFETY: plain descriptor calls on 0–2, which exist for the life of the process; the
    // duplicate is owned by the File alone.
    unsafe {
        let fd = dup(1);
        if fd >= 0 && dup2(2, 1) >= 0 {
            return Box::new(std::io::BufWriter::new(std::fs::File::from_raw_fd(fd)));
        }
    }
    Box::new(std::io::stdout())
}

#[cfg(not(unix))]
fn protocol_stdout() -> Box<dyn Write + Send> {
    Box::new(std::io::stdout())
}

fn exit_usage(message: &str) -> ! {
    eprintln!("{message}\n\n{USAGE}");
    std::process::exit(2);
}

struct Server {
    backend: Backend,
    /// The last live context sent with a tool result: its `seq` and text.
    context: Mutex<Option<(u64, String)>>,
}

/// A request (with an id) or a notification.
struct Frame {
    id: Option<Value>,
    method: String,
    params: Value,
}

impl Server {
    /// Reads one line: `Ok(None)` for something to ignore (a client's response), `Err` for the
    /// error answer to a bad frame.
    fn frame(&self, line: &str) -> Result<Option<Frame>, Value> {
        let frame: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Err(error(Value::Null, -32700, &format!("Parse error: {e}"))),
        };
        let Some(obj) = frame.as_object() else {
            return Err(error(Value::Null, -32600, "Batch requests are not supported"));
        };
        let id = obj.get("id").cloned();
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || obj.get("method").and_then(Value::as_str).is_none_or(str::is_empty)
            || id.as_ref().is_some_and(|id| !id.is_null() && !id.is_string() && !id.is_number())
        {
            // A response from the client (we send no requests) or a malformed frame.
            if obj.contains_key("result") || obj.contains_key("error") {
                return Ok(None);
            }
            return Err(error(id.unwrap_or(Value::Null), -32600, "Invalid JSON-RPC 2.0 request"));
        }
        let method = obj.get("method").and_then(Value::as_str).unwrap_or("").to_string();
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        Ok(Some(Frame { id, method, params }))
    }

    async fn dispatch(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = if PROTOCOLS.contains(&requested) { requested } else { PROTOCOLS[PROTOCOLS.len() - 1] };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "resources": { "subscribe": false, "listChanged": false },
                        "prompts": { "listChanged": false },
                    },
                    "serverInfo": { "name": "nori", "title": "nori: pictures, drawings and pages", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": self.instructions(),
                }))
            }
            "ping" => Ok(json!({})),
            "logging/setLevel" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "tools/call needs `name`".to_string()))?;
                let spec = registry::commands()
                    .iter()
                    .find(|s| s.tool_name() == name || s.name == name)
                    .ok_or_else(|| (-32602, format!("Unknown tool `{name}`")))?;
                if for_builtin_agent() && spec.family() == "agent" {
                    return Err((-32602, format!("Unknown tool `{name}`: the built-in agent doesn't drive itself")));
                }
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                let mut reply = match self.backend.call(spec.name, arguments).await {
                    Ok(result) => {
                        let mut text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
                        if spec.mutates
                            && let Some(path) = self.backend.path()
                        {
                            text.push_str(&format!("\n(saved {})", path.display()));
                        }
                        // A frame, a media look or a screenshot: the client's model gets the picture itself.
                        let mut pictures = vec![];
                        for path in nori_control::vision::pictures_in(spec.name, &result) {
                            match nori_control::vision::picture(&path).await {
                                Ok(p) => pictures.push(json!({ "type": "image", "data": p.data, "mimeType": p.media_type })),
                                Err(e) => text.push_str(&format!("\n(The picture couldn't be attached: {e})")),
                            }
                        }
                        let mut reply = json!({ "content": std::iter::once(json!({ "type": "text", "text": text })).chain(pictures).collect::<Vec<_>>(), "isError": false });
                        if result.is_object() {
                            reply["structuredContent"] = result;
                        }
                        reply
                    }
                    Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
                };
                // The live context, when the document changed since the last one sent.
                if let Some(block) = self.context_update(spec.name).await
                    && let Some(content) = reply["content"].as_array_mut()
                {
                    content.push(json!({ "type": "text", "text": block }));
                }
                Ok(reply)
            }
            "resources/list" => {
                let mut list: Vec<Value> = RESOURCES.iter().map(|(uri, name, description, _)| json!({
                    "uri": uri, "name": name, "description": description, "mimeType": if *uri == "nori://harness/brief" { "text/markdown" } else { "application/json" },
                })).collect();
                list.extend(harness::skills().iter().map(|k| json!({
                    "uri": format!("nori://skills/{}", k.name), "name": k.title, "description": format!("Skill: {}", k.when), "mimeType": "text/markdown",
                })));
                Ok(json!({ "resources": list }))
            }
            "resources/templates/list" => Ok(json!({ "resourceTemplates": [{
                "uriTemplate": "nori://skills/{name}", "name": "Skill", "description": "A playbook for a design job (harness_skills lists them).", "mimeType": "text/markdown",
            }] })),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).ok_or((-32602, "resources/read needs `uri`".to_string()))?;
                if let Some(name) = uri.strip_prefix("nori://skills/") {
                    let k = harness::skill(name).map_err(|e| (-32002, e))?;
                    return Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": k.markdown }] }));
                }
                if uri == "nori://harness/brief" {
                    return Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": harness::brief() }] }));
                }
                let command = RESOURCES.iter().find(|(u, ..)| *u == uri).map(|(.., c)| *c).ok_or((-32002, format!("Unknown resource `{uri}`")))?;
                let value = self.backend.call(command, json!({})).await.map_err(|e| (-32000, e))?;
                Ok(json!({ "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&value).unwrap_or_default(),
                }] }))
            }
            "prompts/list" => Ok(json!({ "prompts": prompts::list() })),
            "prompts/get" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "prompts/get needs `name`".to_string()))?;
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                prompts::get(name, &arguments).map_err(|e| (-32602, e))
            }
            "completion/complete" => Ok(json!({ "completion": { "values": [] } })),
            _ => Err((-32601, format!("Method not found: {method}"))),
        }
    }

    fn instructions(&self) -> String {
        let mode = match &self.backend {
            Backend::Live { .. } => "nori-mcp, live mode: every tool runs in the open nori window. The person may be working at the same time; you share one undo history (history_undo undoes the last step, whoever made it).".to_string(),
            Backend::Local { file: Some(p), .. } => format!(
                "nori-mcp, file mode on {}: the document is saved after every change (a picture or PSD saves to the .nori beside it; export_file writes pictures). Commands that need the window (tools, zoom, panels) are unavailable.",
                p.display()
            ),
            Backend::Local { .. } => "nori-mcp, headless mode (the app isn't running): doc_open or doc_new first; export_file to write results. Commands that need the window are unavailable.".into(),
        };
        let permissions = "Agent permissions (Settings › Agent › Permissions) decide whether you may open and write files, change settings, build plugins or control the app; API keys, the lsuite sign-in and the permissions stay with the person.";
        format!("{}\n\n{permissions}", harness::mcp_instructions(&mode))
    }

    /// The live context to append to a tool result: the new block when the document changed
    /// since the last one sent (`NORI_MCP_CONTEXT=0` turns it off).
    async fn context_update(&self, command: &str) -> Option<String> {
        if std::env::var("NORI_MCP_CONTEXT").is_ok_and(|v| v == "0") || command.starts_with("harness.") || command == "app.commands" {
            return None;
        }
        let since = self.context.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|(seq, _)| *seq);
        let mut params = json!({ "exclude": "mcp" });
        if let Some(seq) = since {
            params["since"] = json!(seq);
        }
        let now = self.backend.call("harness.context", params).await.ok()?;
        let (seq, text) = (now["seq"].as_u64().unwrap_or(0), now["text"].as_str().unwrap_or("").to_string());
        let mut last = self.context.lock().unwrap_or_else(|e| e.into_inner());
        let changed = last.as_ref().is_none_or(|(_, t)| *t != text);
        *last = Some((seq, text.clone()));
        (changed && !text.is_empty()).then(|| format!("<context>\n{text}\n</context>"))
    }
}

/// Registry-backed resources: uri, name, description, command.
const RESOURCES: [(&str, &str, &str, &str); 7] = [
    ("nori://harness/brief", "Expert brief", "How a senior designer works in nori: the quality bar, the commands for the common jobs, the usual mistakes, the finish routine, the skills.", "harness.brief"),
    ("nori://harness/context", "Live context", "What nori shows now: the page and its grid, its layers, the active layer, the selection, quick problems.", "harness.context"),
    ("nori://doc/overview", "Document overview", "Read it first: the whole open document in one bounded answer, with problems to notice.", "doc.overview"),
    ("nori://doc", "Open document", "The complete open document as JSON (document.json of the .nori format).", "doc.get"),
    ("nori://commands", "Commands", "Every command with its parameters, permission and whether it needs the window.", "app.commands"),
    ("nori://settings", "Settings", "nori's settings: agent permissions, tools, appearance, history.", "app.settings"),
    ("nori://app", "Application", "Version, platform, folders and the bridge.", "app.version"),
];

/// Started by the app's built-in agent (Claude Code or Codex as the panel's model): it gets no
/// `agent_*` tools, which would drive the agent itself.
fn for_builtin_agent() -> bool {
    std::env::var_os("NORI_MCP_BUILTIN_AGENT").is_some_and(|v| !v.is_empty() && v != "0")
}

/// One tool per command an agent can run (person-only commands are left out: they are always refused).
fn tools() -> Vec<Value> {
    let builtin = for_builtin_agent();
    registry::commands()
        .iter()
        .filter(|s| s.perm != Perm::PersonOnly && !(builtin && s.family() == "agent"))
        .map(|spec| {
            let mut description = spec.doc.to_string();
            if spec.perm != Perm::Edit {
                description.push_str(&format!(
                    " Needs the \"{}\" agent permission (Settings › Agent › Permissions).",
                    spec.perm.label()
                ));
            }
            if spec.needs_window {
                description.push_str(" Needs the running nori window (nori-mcp --live).");
            }
            let destructive = spec.mutates && ["delete", "remove", "close", "cancel", "quit", "revertTo", "open", "create"].iter().any(|w| spec.name.contains(w));
            json!({
                "name": spec.tool_name(),
                "title": spec.name,
                "description": description,
                "inputSchema": registry::input_schema(spec),
                "annotations": {
                    "readOnlyHint": !spec.mutates,
                    "destructiveHint": destructive,
                    "idempotentHint": !spec.mutates,
                    "openWorldHint": matches!(spec.family(), "handoff" | "account") || matches!(spec.name, "app.checkUpdates" | "agent.send"),
                },
            })
        })
        .collect()
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lines_are_read_within_the_limit() {
        let input: &[u8] = b"{\"a\":1}\r\n0123456789abcdef\nshort\nlast";
        let mut r = tokio::io::BufReader::with_capacity(4, input);
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::Text("{\"a\":1}".into()));
        // Skipped to its end, and the next line is whole.
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::TooLong);
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::Text("short".into()));
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::Text("last".into()));
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::Eof);
    }
}
