//! Everything nori keeps in memory: the open document and its one undo history, settings, the
//! foreground and background colours, imported brushes and swatches, plugins, and the event
//! stream every client listens to.
//!
//! There is one [`Session`] per process. In the desktop app the window, the built-in agent and
//! every bridge client (CLI, MCP) share it, so they share the undo history too. `nori-cli
//! --file` and `nori-mcp --file` make their own headless session around a file.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use futures::channel::{mpsc, oneshot};
use nori_core::{Color, Document, Editor};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::secrets::SecretStore;
use crate::settings::Settings;

pub type CmdResult<T = Value> = Result<T, String>;

/// Identifies an open document within this process (a new one each time a document opens).
pub type DocId = u64;

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

tokio::task_local! {
    /// Set while `doc.batch` runs its commands: their edits belong to its open batch.
    static IN_BATCH: ();
}

pub(crate) fn in_batch_scope() -> bool {
    IN_BATCH.try_with(|_| ()).is_ok()
}

pub(crate) async fn batch_scope<F: std::future::Future>(f: F) -> F::Output {
    IN_BATCH.scope((), f).await
}

/// Who is calling a command. Agent and MCP calls are checked against
/// `settings.agent.permissions`; the window and plain CLI calls are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Window,
    Agent,
    Cli,
    Mcp,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Window => "window",
            Source::Agent => "agent",
            Source::Cli => "cli",
            Source::Mcp => "mcp",
        }
    }

    pub fn is_agent(self) -> bool {
        matches!(self, Source::Agent | Source::Mcp)
    }
}

/// The open document and where it lives.
pub struct OpenDoc {
    pub id: DocId,
    pub editor: Editor,
    /// The `.nori` file it saves to (none: not saved yet).
    pub path: Option<PathBuf>,
    /// The file it was opened from when that isn't a `.nori` (a photo, a PSD, an SVG).
    pub source: Option<PathBuf>,
    /// Save to `path` after every change (`--file` sessions do; the window saves on request).
    pub autosave: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

/// One command any client ran (the Agent panel's cards, its list of changes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRecord {
    pub seq: u64,
    pub source: Source,
    pub command: String,
    pub params: Value,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    pub mutates: bool,
    pub at: DateTime<Utc>,
    /// Layers the command created (from its result), so the window can select them.
    #[serde(default)]
    pub created: Vec<String>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub checkpoint: Option<u64>,
}

/// What the window shows. `ui.state` returns it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    /// `home`, `editor`.
    pub screen: String,
    /// The tool in hand: `move`, `select`, `lasso`, `wand`, `crop`, `eyedropper`, `brush`,
    /// `eraser`, `fill`, `gradient`, `pen`, `direct`, `shape`, `text`, `frame`, `hand`, `zoom`.
    pub tool: String,
    /// Canvas zoom (1 = 100 %).
    pub zoom: f64,
    /// Panels and dialogs open (`agent`, `settings`, `export`, `plugins`…).
    pub open: Vec<String>,
    /// `dark` or `light`.
    pub theme: String,
    /// The right column's tab: `layers`, `pages`.
    pub panel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// The open document changed (any client).
    DocChanged { doc_id: DocId, revision: u64 },
    /// Another document was opened, or it was closed (`None`).
    DocSwitched { doc_id: Option<DocId> },
    Toast { kind: ToastKind, text: String },
    Command { record: CommandRecord },
    SettingsChanged,
    /// Colours, brushes or swatches changed.
    ToolsChanged,
    /// The lsuite account (signed in, out, plan).
    AccountChanged,
    /// Plugins were found, loaded, switched or removed.
    PluginsChanged,
    /// Something being done in the background (export, filter, build) progressed.
    Progress { id: String, label: String, done: bool, error: Option<String> },
}

/// The built-in agent, installed by the app (`nori-agent` depends on this crate, so the
/// `agent.*` commands reach it through this trait).
pub trait AgentHost: Send + Sync {
    fn call(self: Arc<Self>, session: Arc<Session>, source: Source, command: &'static str, args: crate::registry::Args) -> futures::future::BoxFuture<'static, CmdResult>;
}

/// A command only the window can carry out.
pub struct UiCall {
    pub command: String,
    pub params: Value,
    pub reply: oneshot::Sender<CmdResult>,
}

pub struct SessionOptions {
    pub data_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub secrets: Option<Arc<dyn SecretStore>>,
    pub headless: bool,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self { data_dir: None, config_dir: None, secrets: None, headless: true }
    }
}

/// The foreground and background colours (the swatches under the tools).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Colors {
    pub foreground: Color,
    pub background: Color,
}

impl Default for Colors {
    fn default() -> Self {
        Self { foreground: Color::BLACK, background: Color::WHITE }
    }
}

pub struct Session {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub headless: bool,
    secrets: Arc<dyn SecretStore>,
    doc: Mutex<Option<OpenDoc>>,
    settings: RwLock<Settings>,
    events: broadcast::Sender<Event>,
    ui: Mutex<Option<mpsc::UnboundedSender<UiCall>>>,
    ui_state: RwLock<UiState>,
    agent: RwLock<Option<Arc<dyn AgentHost>>>,
    seq: AtomicU64,
    next_doc: AtomicU64,
    runtime: tokio::runtime::Handle,
    pub(crate) bridge_port: Mutex<Option<u16>>,
    pub(crate) batch_lock: Arc<tokio::sync::Mutex<()>>,
    pub colors: RwLock<Colors>,
    pub brushes: RwLock<Vec<Arc<nori_render::brush::TipImage>>>,
    pub swatches: RwLock<Vec<nori_io::assets::Swatch>>,
    pub plugins: crate::plugins::Host,
    /// Where the sign-in in progress listens (`account.signIn`), to cancel it.
    pub(crate) sign_in: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Session {
    /// Must be called inside a Tokio runtime.
    pub fn new(opts: SessionOptions) -> std::io::Result<Arc<Self>> {
        let data_dir = opts.data_dir.unwrap_or_else(default_data_dir);
        let config_dir = opts.config_dir.unwrap_or_else(default_config_dir);
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&config_dir)?;
        let secrets = opts.secrets.unwrap_or_else(|| Arc::new(crate::secrets::MemorySecrets::default()));
        let settings = Settings::load(&config_dir);
        let (events, _) = broadcast::channel(1024);
        let plugins = crate::plugins::Host::new(&settings);
        let session = Arc::new(Self {
            data_dir,
            config_dir,
            headless: opts.headless,
            secrets,
            doc: Mutex::new(None),
            settings: RwLock::new(settings),
            events,
            ui: Mutex::new(None),
            ui_state: RwLock::new(UiState::default()),
            agent: RwLock::new(None),
            seq: AtomicU64::new(1),
            next_doc: AtomicU64::new(1),
            runtime: tokio::runtime::Handle::current(),
            bridge_port: Mutex::new(None),
            batch_lock: Arc::new(tokio::sync::Mutex::new(())),
            colors: RwLock::new(Colors::default()),
            brushes: RwLock::new(vec![]),
            swatches: RwLock::new(vec![]),
            plugins,
            sign_in: Mutex::new(None),
        });
        crate::commands::color::load_assets(&session);
        session.plugins.rescan(&session.data_dir);
        Ok(session)
    }

    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }

    // ---- events -----------------------------------------------------------------------------

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    pub fn toast(&self, kind: ToastKind, text: impl Into<String>) {
        self.emit(Event::Toast { kind, text: text.into() });
    }

    pub(crate) fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    // ---- settings ---------------------------------------------------------------------------

    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> CmdResult<Settings> {
        let s = {
            let mut s = self.settings.write();
            f(&mut s);
            s.clone()
        };
        s.save(&self.config_dir).map_err(|e| format!("Couldn't save settings in {}: {e}", self.config_dir.display()))?;
        crate::diagnostics::set_level(&s.diagnostics.log_level);
        if let Some(d) = self.doc.lock().as_mut() {
            d.editor.set_limits(s.history.steps as usize, s.history.memory_mb as usize * (1 << 20));
        }
        self.emit(Event::SettingsChanged);
        Ok(s)
    }

    /// Remembers a file in the recent list (newest first, 20 at most).
    pub fn remember(&self, path: &Path) {
        let p = path.display().to_string();
        let _ = self.update_settings(|s| {
            s.recent.retain(|r| *r != p);
            s.recent.insert(0, p);
            s.recent.truncate(20);
        });
    }

    pub fn secret(&self, id: &str) -> Option<String> {
        self.secrets.get(id).filter(|k| !k.trim().is_empty())
    }

    pub fn set_secret(&self, id: &str, key: Option<&str>) -> CmdResult<()> {
        match key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(k) => self.secrets.set(id, k),
            None => self.secrets.delete(id),
        }
    }

    // ---- the window -------------------------------------------------------------------------

    pub fn attach_ui(&self) -> mpsc::UnboundedReceiver<UiCall> {
        let (tx, rx) = mpsc::unbounded();
        *self.ui.lock() = Some(tx);
        rx
    }

    pub fn has_ui(&self) -> bool {
        self.ui.lock().as_ref().is_some_and(|tx| !tx.is_closed())
    }

    pub async fn ui_call(&self, command: &str, params: Value) -> CmdResult {
        let tx = self.ui.lock().clone().filter(|tx| !tx.is_closed()).ok_or_else(|| format!("`{command}` needs the nori window. Start the app and use the CLI without --file, or nori-mcp --live."))?;
        let (reply, rx) = oneshot::channel();
        tx.unbounded_send(UiCall { command: command.into(), params, reply }).map_err(|_| "the nori window has closed".to_string())?;
        rx.await.map_err(|_| "the nori window didn't answer".to_string())?
    }

    pub fn set_ui_state(&self, state: UiState) {
        *self.ui_state.write() = state;
    }

    pub fn ui_state(&self) -> UiState {
        self.ui_state.read().clone()
    }

    // ---- the agent --------------------------------------------------------------------------

    pub fn set_agent_host(&self, host: Arc<dyn AgentHost>) {
        *self.agent.write() = Some(host);
    }

    pub fn agent_host(&self) -> Option<Arc<dyn AgentHost>> {
        self.agent.read().clone()
    }

    // ---- the open document ------------------------------------------------------------------

    pub fn is_open(&self) -> bool {
        self.doc.lock().is_some()
    }

    pub fn current_id(&self) -> Option<DocId> {
        self.doc.lock().as_ref().map(|d| d.id)
    }

    /// A copy of the open document (cheap: pixels are shared).
    pub fn document(&self) -> CmdResult<Document> {
        self.read(|ed| ed.doc().clone())
    }

    /// The open document, its revision and its id.
    pub fn snapshot(&self) -> CmdResult<(Document, u64, DocId)> {
        let g = self.doc.lock();
        let d = g.as_ref().ok_or(NO_DOCUMENT)?;
        Ok((d.editor.doc().clone(), d.editor.revision(), d.id))
    }

    pub fn read<R>(&self, f: impl FnOnce(&Editor) -> R) -> CmdResult<R> {
        let g = self.doc.lock();
        let d = g.as_ref().ok_or(NO_DOCUMENT)?;
        Ok(f(&d.editor))
    }

    /// Where the open document saves and what it came from.
    pub fn paths(&self) -> Option<(Option<PathBuf>, Option<PathBuf>, bool)> {
        self.doc.lock().as_ref().map(|d| (d.path.clone(), d.source.clone(), d.editor.is_dirty()))
    }

    /// Opens `doc` (replacing whatever was open) and announces it.
    pub fn open_doc(&self, doc: Document, path: Option<PathBuf>, source: Option<PathBuf>, autosave: bool) -> DocId {
        let id = self.next_doc.fetch_add(1, Ordering::Relaxed);
        let s = self.settings();
        let mut editor = Editor::new(doc);
        editor.set_limits(s.history.steps as usize, s.history.memory_mb as usize * (1 << 20));
        *self.doc.lock() = Some(OpenDoc { id, editor, path, source, autosave });
        self.emit(Event::DocSwitched { doc_id: Some(id) });
        id
    }

    /// From now on the open document saves to `path` after every change (`--file` sessions).
    pub fn set_autosave(&self, path: PathBuf) {
        if let Some(d) = self.doc.lock().as_mut() {
            d.path = Some(path);
            d.autosave = true;
        }
    }

    pub fn close_doc(&self) {
        if self.doc.lock().take().is_some() {
            self.emit(Event::DocSwitched { doc_id: None });
        }
    }

    pub fn checkpoint(&self) -> Option<(DocId, u64)> {
        let mut g = self.doc.lock();
        let d = g.as_mut()?;
        Some((d.id, d.editor.checkpoint()))
    }

    /// Changes the open document with `f` as one undo step (or part of the open batch),
    /// recorded as `label` from `source`; then saves (autosave documents) and announces it.
    pub fn edit<R>(&self, label: &str, source: Source, coalesce: Option<&str>, f: impl FnOnce(&mut Document) -> Result<R, String>) -> CmdResult<R> {
        let mut g = self.doc.lock();
        let d = g.as_mut().ok_or(NO_DOCUMENT)?;
        d.editor.set_step_info(label, source.as_str());
        d.editor.set_outsider(d.editor.in_batch() && !in_batch_scope());
        let r = d.editor.change(coalesce, f);
        d.editor.set_outsider(false);
        let r = r.map_err(err)?;
        let (id, rev) = (d.id, d.editor.revision());
        self.autosave(d)?;
        drop(g);
        self.emit(Event::DocChanged { doc_id: id, revision: rev });
        Ok(r)
    }

    /// Changes what is being looked at (active layer, page shown) without an undo step
    /// ([`Editor::look`]), then announces it.
    pub fn look<R>(&self, f: impl FnOnce(&mut Document) -> Result<R, String>) -> CmdResult<R> {
        let mut g = self.doc.lock();
        let d = g.as_mut().ok_or(NO_DOCUMENT)?;
        let r = d.editor.look(f).map_err(err)?;
        let (id, rev) = (d.id, d.editor.revision());
        drop(g);
        self.emit(Event::DocChanged { doc_id: id, revision: rev });
        Ok(r)
    }

    /// Like [`edit`](Self::edit), but only if the document is still at `revision` (work done on
    /// a snapshot outside the lock lands only on the state it was computed from).
    pub fn edit_at<R>(&self, revision: u64, label: &str, source: Source, f: impl FnOnce(&mut Document) -> Result<R, String>) -> CmdResult<R> {
        {
            let g = self.doc.lock();
            let d = g.as_ref().ok_or(NO_DOCUMENT)?;
            if d.editor.revision() != revision {
                return Err("The document changed while this was being worked out; run it again.".into());
            }
        }
        self.edit(label, source, None, f)
    }

    /// Runs `f` on the editor itself (undo, redo, batches, checkpoints), then announces it.
    pub fn with_editor<R>(&self, f: impl FnOnce(&mut Editor) -> R) -> CmdResult<R> {
        let mut g = self.doc.lock();
        let d = g.as_mut().ok_or(NO_DOCUMENT)?;
        let before = d.editor.revision();
        let r = f(&mut d.editor);
        let (id, rev) = (d.id, d.editor.revision());
        if rev != before {
            self.autosave(d)?;
        }
        drop(g);
        if rev != before {
            self.emit(Event::DocChanged { doc_id: id, revision: rev });
        }
        Ok(r)
    }

    fn autosave(&self, d: &mut OpenDoc) -> CmdResult<()> {
        if d.autosave
            && d.editor.is_dirty()
            && !d.editor.in_batch()
            && let Some(p) = d.path.clone()
        {
            save_file(d.editor.doc(), &p)?;
            d.editor.mark_saved();
        }
        Ok(())
    }

    /// Saves the open document to `path` (or where it saves), as a `.nori` file.
    pub fn save(&self, path: Option<PathBuf>) -> CmdResult<PathBuf> {
        let (doc, target) = {
            let g = self.doc.lock();
            let d = g.as_ref().ok_or(NO_DOCUMENT)?;
            let target = path.or_else(|| d.path.clone()).ok_or("This document hasn't been saved yet: give a path (a .nori file).")?;
            (d.editor.doc().clone(), target)
        };
        let target = if target.extension().is_some_and(|e| e.eq_ignore_ascii_case("nori")) { target } else { target.with_extension("nori") };
        save_file(&doc, &target)?;
        let mut g = self.doc.lock();
        if let Some(d) = g.as_mut() {
            d.path = Some(target.clone());
            d.editor.mark_saved();
        }
        drop(g);
        self.remember(&target);
        Ok(target)
    }

    pub fn bridge_port(&self) -> Option<u16> {
        *self.bridge_port.lock()
    }

    /// Batches: everything until `end` is one undo step (`doc.batch`).
    pub(crate) fn begin_batch(&self, label: &str, source: Source) -> CmdResult<DocId> {
        let mut g = self.doc.lock();
        let d = g.as_mut().ok_or(NO_DOCUMENT)?;
        d.editor.begin_batch(label, source.as_str());
        Ok(d.id)
    }

    pub(crate) fn close_batch(&self, id: DocId, rollback: bool) -> CmdResult<()> {
        let mut g = self.doc.lock();
        let Some(d) = g.as_mut().filter(|d| d.id == id && d.editor.in_batch()) else { return Ok(()) };
        if rollback {
            d.editor.rollback_batch();
        } else {
            d.editor.end_batch();
        }
        let rev = d.editor.revision();
        self.autosave(d)?;
        drop(g);
        self.emit(Event::DocChanged { doc_id: id, revision: rev });
        Ok(())
    }
}

/// Writes a `.nori` file with its preview.
pub fn save_file(doc: &Document, path: &Path) -> CmdResult<()> {
    let rgba = nori_render::flatten(doc);
    let (w, h, thumb) = nori_render::transform::fit_rgba(&rgba, doc.width(), doc.height(), 512);
    nori_core::file::save(doc, path, Some(&nori_core::file::Preview { width: w, height: h, rgba: thumb }))
}

pub const NO_DOCUMENT: &str = "No document is open. Open one with doc.open or make one with doc.new.";

pub fn default_data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("NORI_DATA_DIR").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("nori")
}

pub fn default_config_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("NORI_CONFIG_DIR").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("nori")
}

/// `$LSUITE_HOME`, else `~/.lsuite`.
pub fn lsuite_home() -> PathBuf {
    if let Some(p) = std::env::var_os("LSUITE_HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    dirs::home_dir().unwrap_or_else(std::env::temp_dir).join(".lsuite")
}
