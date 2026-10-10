//! nori-agent: the engine behind the Agent panel.
//!
//! The panel runs the model the person already has: their Claude Code or Codex, an Anthropic or
//! OpenAI key, or a model on a local server (Ollama, any OpenAI-compatible one). [`providers`]
//! describes each. Whatever the provider, the agent acts only through nori's command registry
//! (`nori_control::call`), so permissions (`settings.agent.permissions`), validation and the one
//! undo history behave exactly as for MCP and the CLI.
//!
//! * The API providers and the local servers get every registry command as a tool
//!   (`family_verb`) and are run here, as [`Source::Agent`].
//! * Claude Code and Codex run as child processes with `nori-mcp --live` attached; their
//!   commands reach the app through the bridge as [`Source::Mcp`] and are picked up from the
//!   session's event stream.
//!
//! A run is started with [`Agent::start`] and reports through [`AgentEvent`]s: streamed text,
//! one card per command, then `Done`/`Error`/`Cancelled` with the checkpoint taken before the
//! first change, which [`revert`] puts back. The panel itself talks to [`Host`], which keeps the
//! conversation, the runs and their outcomes for every client.

mod api;
mod cli;
pub mod context;
mod host;
mod http;
pub mod models;
pub mod providers;
mod status;
mod tools;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use futures::channel::mpsc;
use nori_control::session::Event;
use nori_control::settings::AgentSettings;
use nori_control::{CmdResult, CommandRecord, Session, Source};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub use cli::{cli_executable, mcp_executable};
pub use context::{Glance, glance};
pub use host::{ConversationInfo, Entry, Host, RunInfo, RunState, Snapshot};
pub use models::{ModelInfo, ModelList, list as list_models};
pub use providers::Group;
pub use status::{Action, KeyStatus, Next, ProviderStatus, provider_status, status_of};
pub use tools::{RUN_TOOL, TOOL_OUTPUT_LIMIT, ToolDef, ToolSet, system_prompt, tool_defs};

/// Most model round trips in one run before it stops and says so.
pub const MAX_STEPS: usize = 40;

/// Messages kept when a conversation continues; older ones are dropped at the
/// start of a turn (never in the middle of one).
pub const MAX_HISTORY: usize = 80;

// ---- configuration --------------------------------------------------------

/// Which model runs the agent (`settings.agent.provider`). [`providers::ALL`] has the facts
/// about each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
    /// `lsuite` too: conversations saved with lsuite AI (gone; it spoke Anthropic's API) still load.
    #[serde(rename = "anthropic", alias = "lsuite")]
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

impl ProviderKind {
    /// Every provider, in `nori_control::settings::AGENT_PROVIDERS` order: the CLIs, the model
    /// APIs, the local servers.
    pub const ALL: [ProviderKind; 6] = [
        ProviderKind::ClaudeCode,
        ProviderKind::Codex,
        ProviderKind::Anthropic,
        ProviderKind::OpenAi,
        ProviderKind::Ollama,
        ProviderKind::OpenAiCompatible,
    ];

    pub fn info(self) -> &'static providers::Info {
        providers::info(self)
    }

    /// The id used in settings.
    pub fn id(self) -> &'static str {
        self.info().id
    }

    pub fn label(self) -> &'static str {
        self.info().label
    }

    pub fn group(self) -> providers::Group {
        self.info().group
    }

    /// An id, or a name people use for it (`claude`, `local`, `vllm`…).
    pub fn parse(id: &str) -> Option<Self> {
        let id = id.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        if let Some(i) = providers::ALL.iter().find(|i| i.id == id) {
            return Some(i.kind);
        }
        Some(match id.as_str() {
            "claude" | "claudecode" => ProviderKind::ClaudeCode,
            "codex-cli" | "chatgpt" => ProviderKind::Codex,
            "claude-api" | "anthropic-api" => ProviderKind::Anthropic,
            "openai-api" | "gpt" => ProviderKind::OpenAi,
            "local" => ProviderKind::Ollama,
            "openai-compat" | "compatible" | "custom" | "vllm" | "llama.cpp" | "litellm" | "lmstudio" | "lm-studio" => ProviderKind::OpenAiCompatible,
            _ => return None,
        })
    }

    /// Model used when `settings.agent.model` is empty. Empty for the CLIs (their own default)
    /// and the local servers (the first model they have).
    pub fn default_model(self) -> &'static str {
        self.info().default_model
    }

    pub fn default_base_url(self) -> &'static str {
        self.info().default_base_url
    }

    /// Keychain id of the API key, and the environment variable read when it is missing.
    pub fn key_source(self) -> Option<(&'static str, &'static str)> {
        self.info().key.map(|k| (k.id, k.env.first().copied().unwrap_or("")))
    }

    /// Runs as the person's installed CLI, connected back through `nori-mcp --live`.
    pub fn is_cli(self) -> bool {
        self.group() == providers::Group::Cli
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// What a run uses, read from `settings.agent`.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentConfig {
    pub provider: ProviderKind,
    /// Empty: the provider's default.
    pub model: String,
    /// Empty: the provider's default (lets an OpenAI-compatible server or a proxy stand in).
    pub base_url: String,
    /// Model round trips before the run stops (in-process providers).
    pub max_steps: usize,
}

impl AgentConfig {
    pub fn new(provider: ProviderKind) -> Self {
        Self { provider, model: String::new(), base_url: String::new(), max_steps: MAX_STEPS }
    }

    /// An unknown provider id falls back to Claude Code, the settings default.
    pub fn from_settings(s: &AgentSettings) -> Self {
        Self {
            provider: ProviderKind::parse(&s.provider).unwrap_or(ProviderKind::ClaudeCode),
            model: s.model.trim().to_string(),
            base_url: s.base_url.trim().to_string(),
            max_steps: MAX_STEPS,
        }
    }

    /// The configured model, else the provider's default (may be empty, see [`ProviderKind::default_model`]).
    pub fn model(&self) -> String {
        if self.model.trim().is_empty() { self.provider.default_model().to_string() } else { self.model.trim().to_string() }
    }

    /// The configured base URL without a trailing slash, else the provider's default.
    pub fn base_url(&self) -> String {
        let b = self.base_url.trim();
        let b = if b.is_empty() { self.provider.default_base_url() } else { b };
        b.trim_end_matches('/').to_string()
    }

    /// The API key: the keychain, else the provider's environment variable.
    pub fn api_key(&self, session: &Session) -> Option<String> {
        key_for(session, self.provider).map(|(k, _)| k)
    }
}

/// Where a key was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeySource {
    Keychain,
    Env(&'static str),
}

/// A provider's key and where it came from. The session's store reads the keychain, then the
/// provider's environment variable; a key equal to that variable is said to come from it.
pub fn key_for(session: &Session, kind: ProviderKind) -> Option<(String, KeySource)> {
    let spec = kind.info().key?;
    let key = session.secret(spec.id)?;
    let from_env = spec.env.iter().find(|var| std::env::var(var).ok().is_some_and(|v| v.trim() == key.trim()));
    Some((key, from_env.map_or(KeySource::Keychain, |v| KeySource::Env(v))))
}

// ---- conversation ----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Provider-neutral message content, so a conversation survives switching provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text { text: String },
    /// `name` is the tool name (`layer_add`).
    ToolUse { id: String, name: String, input: Value },
    ToolResult { id: String, name: String, output: String, is_error: bool },
    /// A provider block replayed verbatim to the same wire only (Anthropic thinking blocks).
    Opaque { provider: ProviderKind, block: Value },
    /// A picture a command showed the model (`call`: the tool call whose result it belongs to),
    /// base64-encoded. Kept after the results it belongs to, in the same message.
    Image { call: Option<String>, media_type: String, data: String },
    /// The live context refreshed before a later model step (`<context>…</context>`), after the
    /// tool results it follows. Not the person's words: never shown as a request.
    Context { text: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self { role: Role::User, parts: vec![Part::Text { text: text.into() }] }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self { role: Role::Assistant, parts: vec![Part::Text { text: text.into() }] }
    }

    /// The message's text parts, joined.
    pub fn text(&self) -> String {
        let texts: Vec<&str> = self.parts.iter().filter_map(|p| if let Part::Text { text } = p { Some(text.as_str()) } else { None }).collect();
        texts.join("\n\n")
    }

    /// The live context blocks refreshed into this message, joined.
    pub fn context(&self) -> String {
        let blocks: Vec<&str> = self.parts.iter().filter_map(|p| if let Part::Context { text } = p { Some(text.as_str()) } else { None }).collect();
        blocks.join("\n\n")
    }
}

/// A CLI's own conversation, continued with `--resume` (Claude Code) or `exec resume` (Codex).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CliSession {
    pub provider: ProviderKind,
    pub id: String,
}

/// The panel's thread. Pass it back to [`Agent::start`] for a follow-up; a run returns the
/// updated one in [`AgentEvent::Done`] and [`RunHandle::conversation`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub cli_session: Option<CliSession>,
}

impl Conversation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.cli_session.is_none()
    }

    /// Readies the thread for a new turn: answers tool calls a stopped run left open, and drops
    /// the oldest messages beyond [`MAX_HISTORY`]. History is otherwise only appended to, which
    /// keeps prompt caches and Anthropic's thinking blocks valid; when it must be trimmed,
    /// earlier thinking blocks are dropped with it so no block is replayed after a changed prefix.
    pub(crate) fn prepare_turn(&mut self) {
        if let Some(ai) = self.messages.iter().rposition(|m| m.role == Role::Assistant) {
            let answers = self.messages.get(ai + 1).filter(|m| m.role == Role::User && m.parts.iter().all(|p| matches!(p, Part::ToolResult { .. } | Part::Image { .. } | Part::Context { .. })));
            let has_answers = answers.is_some();
            let answered: Vec<&str> = answers.into_iter().flat_map(|m| &m.parts).filter_map(|p| if let Part::ToolResult { id, .. } = p { Some(id.as_str()) } else { None }).collect();
            let open: Vec<Part> = self.messages[ai]
                .parts
                .iter()
                .filter_map(|p| match p {
                    Part::ToolUse { id, name, .. } if !answered.contains(&id.as_str()) => {
                        Some(Part::ToolResult { id: id.clone(), name: name.clone(), output: "Stopped by the person before this ran.".into(), is_error: true })
                    }
                    _ => None,
                })
                .collect();
            if !open.is_empty() {
                if has_answers {
                    self.messages[ai + 1].parts.extend(open);
                } else {
                    self.messages.insert(ai + 1, Message { role: Role::User, parts: open });
                }
            }
        }
        if self.messages.len() > MAX_HISTORY {
            // Start on a person's prompt, never on tool results or a reply.
            let cut = (self.messages.len() - MAX_HISTORY..self.messages.len())
                .find(|&i| self.messages[i].role == Role::User && self.messages[i].parts.iter().any(|p| matches!(p, Part::Text { .. })))
                .unwrap_or(self.messages.len());
            self.messages.drain(..cut);
            for m in &mut self.messages {
                m.parts.retain(|p| !matches!(p, Part::Opaque { .. }));
            }
        }
    }
}

// ---- events -----------------------------------------------------------------

/// What a run reports, in order. Exactly one of `Done`, `Error` or `Cancelled` ends it.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A short progress line for the panel's status ("Thinking with claude-sonnet-5-5…").
    Status { message: String },
    /// Streamed reply text, to append to the current assistant bubble.
    Text { delta: String },
    /// One command the agent ran (a card): who, what, parameters, ok or error. `result` is the
    /// command's JSON answer when the agent ran it in-process; the CLI providers' answers stay
    /// in the CLI.
    Command { record: Box<CommandRecord>, result: Option<Value> },
    /// Tokens used by one model request, when the provider reports them.
    Usage { input_tokens: u64, output_tokens: u64 },
    /// The run finished. `summary` is the final reply; `checkpoint` (set when the run changed
    /// something) is what "Revert this run" passes to [`revert`].
    Done { summary: String, checkpoint: Option<u64>, changes: usize, conversation: Conversation },
    /// The run failed; finished edits stay (and `checkpoint` can revert them).
    Error { message: String, checkpoint: Option<u64>, changes: usize },
    /// Stopped by [`AgentRun::cancel`]; finished edits stay.
    Cancelled { checkpoint: Option<u64>, changes: usize },
}

impl AgentEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(self, AgentEvent::Done { .. } | AgentEvent::Error { .. } | AgentEvent::Cancelled { .. })
    }
}

// ---- runs -------------------------------------------------------------------

#[derive(Default)]
struct Shared {
    checkpoint: Mutex<Option<u64>>,
    changes: AtomicUsize,
    conversation: Mutex<Conversation>,
    finished: AtomicBool,
    steering: Mutex<std::collections::VecDeque<String>>,
    steered: tokio::sync::Notify,
}

/// A cloneable handle on a run: cancel it from anywhere, read its state.
#[derive(Clone)]
pub struct RunHandle {
    cancel: CancellationToken,
    shared: Arc<Shared>,
}

impl RunHandle {
    /// More instructions for the run in progress: the model request under way is dropped and the
    /// run goes on with the thread so far plus these words.
    pub fn steer(&self, prompt: String) -> Result<(), String> {
        if self.is_finished() || self.is_cancelled() {
            return Err("The run has ended. Send a new message.".into());
        }
        self.shared.steering.lock().push_back(prompt);
        self.shared.steered.notify_one();
        Ok(())
    }

    /// Stops the run: the model request is dropped and CLI processes (with their `nori-mcp`)
    /// are killed. Finished edits stay in the undo history.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Acquire)
    }

    /// The checkpoint taken before the run's first change, if any.
    pub fn checkpoint(&self) -> Option<u64> {
        *self.shared.checkpoint.lock()
    }

    /// Successful commands that could change the document, so far.
    pub fn changes(&self) -> usize {
        self.shared.changes.load(Ordering::Acquire)
    }

    /// The thread as it stands (complete once the run is finished).
    pub fn conversation(&self) -> Conversation {
        self.shared.conversation.lock().clone()
    }
}

/// One agent run. Read its events with [`next_event`](Self::next_event) or move the receiver
/// elsewhere with [`take_events`](Self::take_events).
pub struct AgentRun {
    handle: RunHandle,
    events: Option<mpsc::UnboundedReceiver<AgentEvent>>,
}

impl AgentRun {
    pub fn handle(&self) -> RunHandle {
        self.handle.clone()
    }

    pub fn cancel(&self) {
        self.handle.cancel();
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    pub fn checkpoint(&self) -> Option<u64> {
        self.handle.checkpoint()
    }

    pub fn conversation(&self) -> Conversation {
        self.handle.conversation()
    }

    /// The event stream (a `futures` stream, usable from any executor). `None` once taken.
    pub fn take_events(&mut self) -> Option<mpsc::UnboundedReceiver<AgentEvent>> {
        self.events.take()
    }

    /// The next event; `None` after the terminal one (or once the receiver was taken).
    pub async fn next_event(&mut self) -> Option<AgentEvent> {
        use futures::StreamExt;
        self.events.as_mut()?.next().await
    }
}

pub struct Agent;

impl Agent {
    /// Starts a run on the session's Tokio runtime and returns at once. Call from any thread.
    /// `conversation` is the thread so far (`Conversation::new()` for a fresh one).
    pub fn start(session: &Arc<Session>, config: AgentConfig, prompt: impl Into<String>, conversation: Conversation) -> AgentRun {
        let (mut run, run_handle) = Run::new(session, config, &conversation);
        let cancel = run.cancel.clone();
        let prompt = prompt.into();
        session.runtime().spawn(async move {
            let outcome = tokio::select! {
                r = run.execute(prompt, conversation) => Some(r),
                _ = cancel.cancelled() => None,
            };
            run.finish(outcome);
        });
        run_handle
    }
}

/// "Revert this run": puts the document back as it was at `checkpoint`, as one new undo step
/// (the revert can be undone too). The person's action, so it runs as the window and is not
/// subject to agent permissions.
pub async fn revert(session: &Arc<Session>, checkpoint: u64) -> CmdResult {
    nori_control::call(session, Source::Window, "history.revertTo", json!({ "checkpoint": checkpoint })).await
}

/// The state a provider works with while a run is going.
pub(crate) struct Run {
    pub session: Arc<Session>,
    pub config: AgentConfig,
    events: mpsc::UnboundedSender<AgentEvent>,
    pub cancel: CancellationToken,
    shared: Arc<Shared>,
    /// Session events, for the command records of this run.
    pub commands: broadcast::Receiver<Event>,
    /// Text was streamed since the last break.
    text_started: AtomicBool,
    /// The next text starts a new paragraph (a new model message).
    text_break: AtomicBool,
}

impl Run {
    pub(crate) fn new(session: &Arc<Session>, config: AgentConfig, conversation: &Conversation) -> (Self, AgentRun) {
        let (tx, rx) = mpsc::unbounded();
        let cancel = CancellationToken::new();
        let shared = Arc::new(Shared { conversation: Mutex::new(conversation.clone()), ..Default::default() });
        let handle = RunHandle { cancel: cancel.clone(), shared: shared.clone() };
        let run = Run {
            session: session.clone(),
            config,
            events: tx,
            cancel,
            shared,
            commands: session.subscribe(),
            text_started: AtomicBool::new(false),
            text_break: AtomicBool::new(false),
        };
        (run, AgentRun { handle, events: Some(rx) })
    }

    pub fn emit(&self, event: AgentEvent) {
        let _ = self.events.unbounded_send(event);
    }

    pub fn status(&self, message: impl Into<String>) {
        self.emit(AgentEvent::Status { message: message.into() });
    }

    pub fn text(&self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let delta = if self.text_break.swap(false, Ordering::AcqRel) { format!("\n\n{delta}") } else { delta.to_string() };
        self.text_started.store(true, Ordering::Release);
        self.emit(AgentEvent::Text { delta });
    }

    /// A new model message begins: its text is a new paragraph in the reply bubble.
    pub fn break_text(&self) {
        if self.text_started.load(Ordering::Acquire) {
            self.text_break.store(true, Ordering::Release);
        }
    }

    pub fn text_break_pending(&self) -> bool {
        self.text_break.load(Ordering::Acquire)
    }

    pub fn usage(&self, input_tokens: u64, output_tokens: u64) {
        if input_tokens > 0 || output_tokens > 0 {
            self.emit(AgentEvent::Usage { input_tokens, output_tokens });
        }
    }

    pub fn set_conversation(&self, c: &Conversation) {
        *self.shared.conversation.lock() = c.clone();
    }

    /// Takes the run's checkpoint once, before its first change, if a document is open.
    pub async fn ensure_checkpoint(&self) {
        if self.shared.checkpoint.lock().is_some() || !self.session.is_open() {
            return;
        }
        // The panel takes it for the person (a checkpoint changes nothing), so it works whatever
        // the agent permissions say.
        if let Ok(v) = nori_control::call(&self.session, Source::Window, "history.checkpoint", json!({})).await {
            *self.shared.checkpoint.lock() = v["checkpoint"].as_u64();
        }
    }

    /// A command record from the session's stream: counts changes and shows the card.
    pub fn command(&self, record: CommandRecord, result: Option<Value>) {
        if record.ok && record.mutates {
            self.shared.changes.fetch_add(1, Ordering::AcqRel);
        }
        self.emit(AgentEvent::Command { record: Box::new(record), result });
    }

    /// Forwards every queued command record from `source` (the CLI providers' MCP calls).
    pub fn drain_commands(&mut self, source: Source) {
        loop {
            match self.commands.try_recv() {
                Ok(Event::Command { record }) if record.source == source => self.command(record, None),
                Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(_) => break,
            }
        }
    }

    async fn execute(&mut self, prompt: String, conversation: Conversation) -> Result<String, String> {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() {
            return Err("Type a request first.".into());
        }
        if !self.session.settings().agent.permissions.enabled {
            return Err("Agents are turned off in Settings › Agent › Permissions.".into());
        }
        // Commands from before this run are not its own.
        while !matches!(self.commands.try_recv(), Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed)) {}
        let mut request = prompt;
        let mut context = conversation;
        loop {
            let shared = self.shared.clone();
            let outcome = tokio::select! {
                biased;
                _ = shared.steered.notified() => None,
                result = async {
                    if self.config.provider.is_cli() {
                        cli::run(self, request.clone(), context.clone()).await
                    } else {
                        api::run(self, request.clone(), context.clone()).await
                    }
                } => Some(result),
            };
            if let Some(result) = outcome {
                return result;
            }
            let messages: Vec<String> = self.shared.steering.lock().drain(..).collect();
            if messages.is_empty() {
                continue;
            }
            context = self.shared.conversation.lock().clone();
            if context.messages.is_empty() {
                context.messages.push(Message::user(request.clone()));
            }
            request = format!("Steering from the person; keep the work done so far and follow these updated instructions:\n{}", messages.join("\n\n"));
            self.status("Applying your steering message…");
            self.break_text();
        }
    }

    fn finish(&mut self, outcome: Option<Result<String, String>>) {
        if self.config.provider.is_cli() {
            self.drain_commands(Source::Mcp);
        }
        let checkpoint = *self.shared.checkpoint.lock();
        let changes = self.shared.changes.load(Ordering::Acquire);
        // A checkpoint with nothing after it is nothing to revert.
        let checkpoint = checkpoint.filter(|_| changes > 0);
        let event = match outcome {
            None => AgentEvent::Cancelled { checkpoint, changes },
            Some(Ok(summary)) => AgentEvent::Done { summary, checkpoint, changes, conversation: self.shared.conversation.lock().clone() },
            Some(Err(message)) => AgentEvent::Error { message, checkpoint, changes },
        };
        self.shared.finished.store(true, Ordering::Release);
        self.emit(event);
        self.events.close_channel();
    }
}
