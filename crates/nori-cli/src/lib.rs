//! Shared plumbing for `nori-cli` and `nori-mcp`: run registry commands on the running app
//! (over the loopback bridge) or in a session of our own, on a file or in memory.
//!
//! Nothing here knows a command by name except opening the file (`doc.open`) and `doc.new` on a
//! file that doesn't exist yet: everything else goes through [`nori_control::call`], so the CLI
//! and MCP can do exactly what the window and the built-in agent can.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nori_control::bridge::{self, Client};
use nori_control::registry::{self, Kind};
use nori_control::session::NO_DOCUMENT;
use nori_control::{CmdResult, Session, SessionOptions, Source};
use serde_json::{Map, Value, json};

pub use nori_control;

/// Where commands run. Calls may overlap (`nori-mcp` serves requests concurrently).
pub enum Backend {
    /// The running app. Connects on the first call and again after a lost connection; each call
    /// in flight has a connection of its own (a bridge connection answers one request at a time),
    /// and idle ones are kept for the next calls.
    Live { idle: std::sync::Mutex<Vec<Client>>, name: &'static str },
    /// A session in this process: on a file (`file`) or in memory.
    Local { session: Arc<Session>, source: Source, file: Option<PathBuf> },
}

impl Backend {
    /// The running app, connecting now. `name` is how we introduce ourselves to the bridge
    /// (`cli`, `mcp` or `agent`): agent and MCP requests are held to the agent permissions.
    pub async fn live(name: &'static str) -> CmdResult<Self> {
        let client = Client::connect(&bridge::default_control_path(), name).await?;
        Ok(Backend::Live { idle: std::sync::Mutex::new(vec![client]), name })
    }

    /// The running app, connecting on the first call (the MCP server may start before the app).
    pub fn live_lazy(name: &'static str) -> Self {
        Backend::Live { idle: std::sync::Mutex::new(vec![]), name }
    }

    /// A session in memory in this process (open, change and export files without the app).
    pub async fn headless(source: Source) -> CmdResult<Self> {
        Ok(Backend::Local { session: local_session()?, source, file: None })
    }

    /// A session on a file, saved after every change. A `.nori` file saves back to itself; any
    /// other file nori opens (a photo, a PSD, an SVG) saves to the `.nori` beside it, so the
    /// original is never overwritten (export.file writes pictures). A file that doesn't exist
    /// yet is made by `doc.new`. A file open in the running app is refused.
    pub async fn file(path: &Path, source: Source) -> CmdResult<Self> {
        let path = absolute(path)?;
        if let Some(open) = open_in_app().await
            && same_file(&open, &path)
        {
            return Err(format!(
                "{} is open in nori. Drive it live instead (drop --file), or close it in the app first.",
                path.display()
            ));
        }
        let session = local_session()?;
        if path.exists() {
            registry::call(&session, source, "doc.open", json!({ "path": path })).await?;
            session.set_autosave(save_target(&path));
        }
        Ok(Backend::Local { session, source, file: Some(path) })
    }

    pub fn mode(&self) -> &'static str {
        match self {
            Backend::Live { .. } => "live",
            Backend::Local { file: Some(_), .. } => "file",
            Backend::Local { .. } => "headless",
        }
    }

    /// The project file, in file mode.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Backend::Local { file, .. } => file.as_deref(),
            Backend::Live { .. } => None,
        }
    }

    pub async fn call(&self, command: &str, params: Value) -> CmdResult {
        match self {
            Backend::Live { idle, name } => {
                let taken = idle.lock().unwrap_or_else(|e| e.into_inner()).pop();
                let mut client = match taken {
                    Some(c) => c,
                    None => Client::connect(&bridge::default_control_path(), name).await?,
                };
                // A call dropped half-way (cancelled) drops its connection with it, so no
                // late answer is ever read as another call's.
                let result = client.call(command, params).await;
                if !result.as_ref().is_err_and(|e| e.starts_with("Lost the connection") || e.starts_with("nori closed the connection")) {
                    idle.lock().unwrap_or_else(|e| e.into_inner()).push(client);
                }
                result
            }
            Backend::Local { session, source, file } => {
                let (session, source) = (session.clone(), *source);
                if let Some(path) = file.as_deref()
                    && !session.is_open()
                {
                    if command == "doc.new" && !path.exists() {
                        let made = registry::call(&session, source, command, params).await?;
                        session.set_autosave(save_target(path));
                        session.save(Some(save_target(path)))?;
                        return Ok(made);
                    }
                    let result = registry::call(&session, source, command, params).await;
                    return result.map_err(|e| {
                        if e == NO_DOCUMENT && !path.exists() {
                            format!("{} doesn't exist yet. Make it with doc.new (nori-cli --file {} doc.new width=1920 height=1080).", path.display(), path.display())
                        } else {
                            e
                        }
                    });
                }
                registry::call(&session, source, command, params).await
            }
        }
    }
}

/// Where a `--file` session saves: the file itself when it is a `.nori`, else the `.nori`
/// beside it.
pub fn save_target(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nori")) { path.to_path_buf() } else { path.with_extension("nori") }
}

/// A headless session with the app's own folders and the OS keychain for API keys.
fn local_session() -> CmdResult<Arc<Session>> {
    Session::new(SessionOptions { data_dir: None, config_dir: None, secrets: Some(nori_control::secrets::default_store()), headless: true })
        .map_err(|e| format!("Couldn't start a nori session: {e}"))
}

/// Whether the app answers on its bridge.
pub async fn app_running() -> bool {
    Client::connect(&bridge::default_control_path(), "cli").await.is_ok()
}

/// The project file the running app has open, if it runs and has one open.
pub async fn open_in_app() -> Option<PathBuf> {
    let mut c = Client::connect(&bridge::default_control_path(), "cli").await.ok()?;
    let o = c.call("doc.overview", json!({})).await.ok()?;
    o["savedTo"].as_str().or(o["openedFrom"].as_str()).map(PathBuf::from)
}

/// An absolute path with symlinks resolved, also for a file that doesn't exist yet.
pub fn absolute(path: &Path) -> CmdResult<PathBuf> {
    if let Ok(p) = path.canonicalize() {
        return Ok(p);
    }
    let path = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map_err(|e| e.to_string())?.join(path) };
    let name = path.file_name().ok_or_else(|| format!("{} isn't a file path", path.display()))?;
    let parent = path.parent().unwrap_or(Path::new("/"));
    Ok(parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf()).join(name))
}

pub fn same_file(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| absolute(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

// ---- command-line arguments ---------------------------------------------------------------

/// A parsed `nori-cli` command line.
#[derive(Debug, Default, PartialEq)]
pub struct Invocation {
    pub file: Option<PathBuf>,
    pub live: bool,
    pub headless: bool,
    pub agent: bool,
    pub compact: bool,
    pub keep_going: bool,
    pub help: bool,
    pub version: bool,
    /// The command (`layer.add`) or tool (`commands`, `batch`, `convert`…).
    pub command: Option<String>,
    /// Registry commands: the parameters, coerced to their declared types.
    pub params: Map<String, Value>,
    /// Built-in tools: their own arguments, untouched.
    pub rest: Vec<String>,
}

/// Commands of the CLI itself rather than the registry. Their arguments are kept as given.
pub const TOOLS: &[&str] = &["commands", "help", "docs", "batch", "doctor", "mcp-config", "convert"];

/// Parses a command line. Options may come before or after the command; for a registry
/// command, every other argument is a parameter: `--name value`, `--name=value` or `name=value`
/// (`--name` alone is true for a boolean parameter). `--args '<json>'` gives several at once.
pub fn parse_args(args: &[String]) -> Result<Invocation, String> {
    let mut inv = Invocation::default();
    let mut pairs: Vec<(String, Option<String>)> = vec![];
    let mut args_json: Option<String> = None;
    let mut i = 0;
    let value = |i: &mut usize, flag: &str| -> Result<String, String> {
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| format!("{flag} needs a value"))
    };
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--help" | "-h" => inv.help = true,
            "--version" | "-V" => inv.version = true,
            "--file" | "-f" => inv.file = Some(PathBuf::from(value(&mut i, arg)?)),
            "--live" => inv.live = true,
            "--headless" => inv.headless = true,
            "--agent" => inv.agent = true,
            "--compact" | "-c" => inv.compact = true,
            "--continue" => inv.keep_going = true,
            "--args" => args_json = Some(value(&mut i, arg)?),
            _ if inv.command.is_none() => {
                if arg.starts_with('-') {
                    return Err(format!("Unknown option `{arg}`"));
                }
                inv.command = Some(arg.to_string());
            }
            _ if inv.command.as_deref().is_some_and(|c| TOOLS.contains(&c)) => inv.rest.push(arg.to_string()),
            _ => {
                if let Some(key) = arg.strip_prefix("--") {
                    match key.split_once('=') {
                        Some((k, v)) => pairs.push((k.into(), Some(v.into()))),
                        None => match args.get(i + 1) {
                            Some(next) if !next.starts_with("--") => {
                                pairs.push((key.into(), Some(next.clone())));
                                i += 1;
                            }
                            _ => pairs.push((key.into(), None)),
                        },
                    }
                } else if let Some((k, v)) = arg.split_once('=') {
                    pairs.push((k.into(), Some(v.into())));
                } else {
                    return Err(format!("Expected `--name value` or `name=value`, got `{arg}`"));
                }
            }
        }
        i += 1;
    }
    if usize::from(inv.file.is_some()) + usize::from(inv.live) + usize::from(inv.headless) > 1 {
        return Err("--file, --live and --headless are exclusive".into());
    }
    let command = inv.command.clone().unwrap_or_default();
    if let Some(text) = args_json {
        match serde_json::from_str::<Value>(&text).map_err(|e| format!("--args must be a JSON object: {e}"))? {
            Value::Object(m) => inv.params = m,
            _ => return Err("--args must be a JSON object".into()),
        }
    }
    for (k, v) in pairs {
        let v = match v {
            Some(v) => coerce(&command, &k, &v)?,
            None if kind_of(&command, &k) == Some(Kind::Boolean) => Value::Bool(true),
            None => return Err(format!("--{k} needs a value")),
        };
        // A repeated array parameter collects its values: --paths a.mp4 --paths b.mp4.
        match (inv.params.get_mut(&k), v) {
            (Some(Value::Array(have)), Value::Array(more)) if kind_of(&command, &k) == Some(Kind::Array) => have.extend(more),
            (_, v) => {
                inv.params.insert(k, v);
            }
        }
    }
    absolute_paths(&mut inv.params);
    Ok(inv)
}

/// Paths are relative to where the CLI runs, not to the app (whose folder is `/` when it was
/// opened from the Dock): `path` and `paths` become absolute.
fn absolute_paths(params: &mut serde_json::Map<String, Value>) {
    let fix = |v: &mut Value| {
        if let Value::String(s) = v
            && !s.is_empty()
            && !s.contains("://")
            && let Ok(p) = absolute(Path::new(s.as_str()))
        {
            *s = p.to_string_lossy().into_owned();
        }
    };
    for key in ["path", "lut"] {
        if let Some(v) = params.get_mut(key) {
            fix(v);
        }
    }
    if let Some(Value::Array(list)) = params.get_mut("paths") {
        list.iter_mut().for_each(fix);
    }
}

fn kind_of(command: &str, key: &str) -> Option<Kind> {
    let spec = registry::spec(command)?;
    spec.param(key).map(|p| p.kind).or((key == registry::COALESCE.name && spec.mutates).then_some(Kind::String))
}

/// A command-line value for a parameter, as its declared type. Strings stay strings, booleans
/// accept yes/no/on/off/1/0, an array takes JSON or a single value, anything else is JSON when
/// it parses and a string otherwise (so the registry names the mistake).
pub fn coerce(command: &str, key: &str, raw: &str) -> Result<Value, String> {
    let json = || serde_json::from_str::<Value>(raw);
    Ok(match kind_of(command, key) {
        Some(Kind::String) => Value::String(raw.into()),
        Some(Kind::Boolean) => match raw.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Value::Bool(true),
            "false" | "no" | "off" | "0" => Value::Bool(false),
            _ => Value::String(raw.into()),
        },
        Some(Kind::Array) => match json() {
            Ok(Value::Array(a)) => Value::Array(a),
            Err(e) if raw.trim_start().starts_with('[') => return Err(format!("`{key}` looks like JSON but isn't: {e}")),
            Ok(v) if !v.is_string() => Value::Array(vec![v]),
            _ => Value::Array(vec![Value::String(raw.into())]),
        },
        Some(Kind::Object) => match json() {
            Ok(v) => v,
            Err(e) if raw.trim_start().starts_with('{') => return Err(format!("`{key}` looks like JSON but isn't: {e}")),
            Err(_) => Value::String(raw.into()),
        },
        _ => json().unwrap_or_else(|_| Value::String(raw.into())),
    })
}

/// One line of a batch: `{"command": "layer.add", "params": {…}}`. `method`/`name` and
/// `arguments` are accepted too, so JSON-RPC and MCP-shaped lines work.
pub fn parse_batch_line(line: &str) -> Result<(String, Value), String> {
    let frame: Value = serde_json::from_str(line).map_err(|e| format!("Batch line is not JSON: {e}"))?;
    let name = frame
        .get("command")
        .or_else(|| frame.get("method"))
        .or_else(|| frame.get("name"))
        .and_then(Value::as_str)
        .ok_or("Batch line needs a `command`")?
        .to_string();
    let params = frame.get("params").or_else(|| frame.get("arguments")).cloned().unwrap_or(Value::Null);
    Ok((name, params))
}

/// An unknown command, with the closest one.
pub fn unknown_command(name: &str) -> String {
    let names: Vec<&str> = registry::commands().iter().map(|s| s.name).collect();
    let tools: Vec<&str> = names.iter().copied().chain(TOOLS.iter().copied()).collect();
    match registry::closest(name, &tools) {
        Some(c) => format!("Unknown command `{name}`. Did you mean `{c}`? (`nori-cli commands` lists them all.)"),
        None => format!("Unknown command `{name}`. `nori-cli commands` lists them all."),
    }
}

// ---- doctor and MCP configuration -------------------------------------------------------------

/// An executable shipped next to this one, if it is there.
pub fn sibling(name: &str) -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let p = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    p.is_file().then_some(p)
}

/// Health report for `nori-cli doctor`.
pub async fn doctor() -> Value {
    use nori_control::{discovery, session};
    let mut checks = vec![];
    let mut push = |name: &str, ok: bool, detail: String| checks.push(json!({ "check": name, "ok": ok, "detail": detail }));
    let mine = env!("CARGO_PKG_VERSION");

    let control = bridge::default_control_path();
    let discovery = bridge::read_discovery(&control);
    match &discovery {
        Ok(d) => push("control file", true, format!("{} (port {}, pid {})", control.display(), d.port, d.pid)),
        Err(e) => push("control file", false, e.clone()),
    }
    let app_version = if discovery.is_ok() {
        match Client::connect(&control, "cli").await {
            Ok(mut c) => {
                let info = c.call("app.version", json!({})).await.unwrap_or_default();
                let v = info["version"].as_str().unwrap_or("?").to_string();
                push("bridge", true, format!("nori {v} answers on 127.0.0.1"));
                Some(v)
            }
            Err(e) => {
                push("bridge", false, e);
                None
            }
        }
    } else {
        push("bridge", false, "nori isn't running; --file and --headless still work.".into());
        None
    };
    push(
        "versions",
        app_version.as_deref().is_none_or(|v| v == mine),
        match &app_version {
            Some(v) if v != mine => format!("nori-cli {mine} but the app is {v}: update both together"),
            _ => format!("nori-cli {mine} · {} commands", registry::commands().len()),
        },
    );
    for (name, dir) in [("data folder", session::default_data_dir()), ("config folder", session::default_config_dir())] {
        let writable = std::fs::create_dir_all(&dir)
            .and_then(|()| {
                let probe = dir.join(".doctor-write-test");
                std::fs::write(&probe, b"ok")?;
                std::fs::remove_file(&probe)
            })
            .is_ok();
        push(name, writable, dir.display().to_string());
    }
    match discovery::find("nori") {
        Some(e) => push(
            "lsuite entry",
            true,
            format!(
                "{} (nori {}{})",
                discovery::apps_dir().join("nori.json").display(),
                e.version,
                if e.running.is_some() { ", running" } else { "" }
            ),
        ),
        None => push("lsuite entry", false, format!("no {} yet: start nori once to write it", discovery::apps_dir().join("nori.json").display())),
    }
    match sibling("nori-mcp") {
        Some(p) => push("nori-mcp", true, p.display().to_string()),
        None => push("nori-mcp", false, "not next to nori-cli; `cargo build -p nori-mcp` or reinstall the app".into()),
    }
    match nori_control::account::read() {
        Some(a) => push("lsuite AI", true, format!("signed in as {} ({})", a.email, if a.plan.is_empty() { "no plan" } else { a.plan.as_str() })),
        None => push("lsuite AI", true, "not signed in (nori-cli account.signIn, or another provider)".into()),
    }
    let ok = checks.iter().all(|c| c["ok"] == true);
    json!({ "ok": ok, "checks": checks })
}

/// How to connect an MCP client to the running app: the server command, its environment and
/// ready lines for Claude Code and Codex, and the JSON block most other clients take.
pub fn mcp_config() -> Value {
    let command = sibling("nori-mcp")
        .or_else(|| nori_control::discovery::find("nori").and_then(|e| e.mcp))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "nori-mcp".into());
    let env: Map<String, Value> =
        std::env::var("NORI_CONTROL").ok().filter(|v| !v.is_empty()).map(|v| ("NORI_CONTROL".to_string(), json!(v))).into_iter().collect();
    let quoted = shell_quote(&command);
    let env_flags = |flag: &str| env.iter().map(|(k, v)| format!(" {flag} {k}={}", shell_quote(v.as_str().unwrap_or("")))).collect::<String>();
    let mut server = json!({ "command": command, "args": ["--live"] });
    if !env.is_empty() {
        server["env"] = Value::Object(env.clone());
    }
    json!({
        "command": command,
        "args": ["--live"],
        "env": env,
        "claudeCode": format!("claude mcp add nori{} -- {quoted} --live", env_flags("-e")),
        "codex": format!("codex mcp add nori{} -- {quoted} --live", env_flags("--env")),
        "json": { "mcpServers": { "nori": server } },
    })
}

/// Quoted for this computer's shell: single quotes on Unix, double quotes on Windows (cmd and
/// PowerShell don't take single-quoted paths; `"` can't appear in a Windows path).
fn shell_quote(s: &str) -> String {
    quote_for(s, cfg!(windows))
}

fn quote_for(s: &str, windows: bool) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+=:@,".contains(c) || (windows && c == '\\')) {
        s.to_string()
    } else if windows {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Invocation, String> {
        parse_args(&line.split(' ').map(str::to_string).collect::<Vec<_>>())
    }

    #[test]
    fn parameters_take_their_declared_types() {
        let inv = parse("text.add --text 42 x=1.5 --y=3 --frameWidth 300").unwrap();
        assert_eq!(inv.command.as_deref(), Some("text.add"));
        assert_eq!(inv.params["text"], json!("42"));
        assert_eq!(inv.params["x"], json!(1.5));
        assert_eq!(inv.params["y"], json!(3));
        let inv = parse("layer.addAdjustment kind=levels --settings {\"gamma\":1.2}").unwrap();
        assert_eq!(inv.params["settings"], json!({ "gamma": 1.2 }));
    }

    #[test]
    fn options_go_anywhere_and_booleans_stand_alone() {
        let inv = parse("--file a.nori layer.update visible=false --compact --agent --clipped").unwrap();
        assert_eq!(inv.file, Some(PathBuf::from("a.nori")));
        assert!(inv.compact && inv.agent);
        assert_eq!(inv.params["visible"], json!(false));
        assert_eq!(inv.params["clipped"], json!(true));
        assert!(parse("text.add --text").unwrap_err().contains("needs a value"));
    }

    #[test]
    fn paths_are_made_absolute_where_the_cli_runs() {
        let inv = parse("export.file path=out.png").unwrap();
        let p = PathBuf::from(inv.params["path"].as_str().unwrap());
        assert!(p.is_absolute() && p.ends_with("out.png"), "{p:?}");
    }

    #[test]
    fn arrays_take_json_or_repeats() {
        let inv = parse("layer.delete --layerIds L2 --layerIds L3").unwrap();
        assert_eq!(inv.params["layerIds"], json!(["L2", "L3"]));
        let inv = parse("raster.stroke points=[[1,2],[3,4]]").unwrap();
        assert_eq!(inv.params["points"], json!([[1, 2], [3, 4]]));
        assert!(parse("layer.delete layerIds=[oops").unwrap_err().contains("JSON"));
    }

    #[test]
    fn file_sessions_never_overwrite_the_original() {
        assert_eq!(save_target(Path::new("/a/photo.jpg")), PathBuf::from("/a/photo.nori"));
        assert_eq!(save_target(Path::new("/a/b.nori")), PathBuf::from("/a/b.nori"));
    }

    #[test]
    fn typos_get_a_suggestion() {
        assert!(unknown_command("layer.ad").contains("layer.add"));
        assert!(unknown_command("comands").contains("`commands`"));
    }

    #[test]
    fn mcp_config_names_the_live_server() {
        let c = mcp_config();
        assert!(c["claudeCode"].as_str().unwrap().starts_with("claude mcp add nori"));
        assert_eq!(c["json"]["mcpServers"]["nori"]["args"], json!(["--live"]));
    }
}
