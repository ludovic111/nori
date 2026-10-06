//! Durable conversations, kept per document in `<data dir>/agent/conversations.json`: the
//! document's file (or `unsaved:<name>`) → its threads, the current one and its memory.

use std::path::{Path, PathBuf};

use super::*;

/// The title of a conversation nobody has asked anything in yet.
pub(super) const NEW_TITLE: &str = "New conversation";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationInfo {
    pub id: Uuid,
    pub title: String,
    pub updated_at: DateTime<Utc>,
}

impl Default for ConversationInfo {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), title: NEW_TITLE.into(), updated_at: Utc::now() }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SavedThread {
    pub info: ConversationInfo,
    pub conversation: Conversation,
    pub entries: Vec<Entry>,
    pub runs: Vec<RunInfo>,
    pub base: usize,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct DocThreads {
    pub current: Option<Uuid>,
    pub threads: Vec<SavedThread>,
    #[serde(default)]
    pub memory: String,
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Archive {
    #[serde(default)]
    pub documents: HashMap<String, DocThreads>,
    #[serde(default)]
    pub next_run: u64,
}

pub(super) fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("agent").join("conversations.json")
}

impl State {
    pub(super) fn save_current(&mut self) {
        let key = self.key.clone();
        let saved = SavedThread { info: self.thread.clone(), conversation: self.conversation.clone(), entries: self.entries.clone(), runs: self.runs.clone(), base: self.base };
        let doc = self.archive.documents.entry(key).or_default();
        doc.current = Some(saved.info.id);
        doc.memory = self.memory.clone();
        // A thread nobody wrote in isn't worth keeping (every document opened would leave one).
        let empty = saved.entries.is_empty() && saved.info.title == NEW_TITLE;
        match doc.threads.iter().position(|t| t.info.id == saved.info.id) {
            Some(i) if empty => {
                doc.threads.remove(i);
            }
            Some(i) => doc.threads[i] = saved,
            None if empty => {}
            None => doc.threads.push(saved),
        }
        self.archive.next_run = self.next_run;
    }

    /// The document's file or name changed (first save, rename): its current conversation and
    /// memory move to the new key; the others stay with the old one.
    pub(super) fn rekey(&mut self, key: String) {
        let old = std::mem::replace(&mut self.key, key);
        let id = self.thread.id;
        let moved = self.archive.documents.get_mut(&old).and_then(|d| {
            let thread = d.threads.iter().position(|t| t.info.id == id).map(|i| d.threads.remove(i));
            if d.current == Some(id) {
                d.current = None;
            }
            thread
        });
        if self.archive.documents.get(&old).is_some_and(|d| d.threads.is_empty()) {
            self.archive.documents.remove(&old);
        }
        let doc = self.archive.documents.entry(self.key.clone()).or_default();
        doc.threads.extend(moved);
        doc.current = Some(id);
        if doc.memory.is_empty() {
            doc.memory = self.memory.clone();
        } else {
            self.memory = doc.memory.clone();
        }
    }

    pub(super) fn load_doc(&mut self) {
        let doc = self.archive.documents.get(&self.key).cloned().unwrap_or_default();
        self.memory = doc.memory;
        match doc.threads.into_iter().find(|t| Some(t.info.id) == doc.current) {
            Some(thread) => self.load_thread(thread),
            None => self.fresh_thread(),
        }
    }

    pub(super) fn fresh_thread(&mut self) {
        self.clear_thread();
        self.runs.clear();
        self.reverts.clear();
        self.unreverts.clear();
        self.thread = ConversationInfo::default();
        self.base = 0;
    }

    pub(super) fn load_thread(&mut self, saved: SavedThread) {
        self.thread = saved.info;
        self.conversation = saved.conversation;
        self.entries = saved.entries;
        self.runs = saved.runs;
        self.base = saved.base;
        // Command sequence numbers and undo checkpoints are session-local: a restored
        // conversation never reverts an unrelated edit after a restart.
        self.seen.clear();
        self.reverts.clear();
        self.unreverts.clear();
    }

    /// The open document's conversations, the latest first.
    pub(super) fn conversations(&self) -> Vec<ConversationInfo> {
        let mut list: Vec<_> = self.archive.documents.get(&self.key).into_iter().flat_map(|d| &d.threads).filter(|t| t.info.id != self.thread.id).map(|t| t.info.clone()).collect();
        list.push(self.thread.clone());
        list.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        list
    }
}

pub(super) fn read(path: &Path) -> CmdResult<Archive> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Archive::default()),
        Err(e) => return Err(format!("Couldn't read the agent's conversations at {}: {e}. Saving them is paused to protect the file; restore it and restart nori.", path.display())),
    };
    let mut archive: Archive = match serde_json::from_slice(&bytes) {
        Ok(archive) => archive,
        Err(e) => {
            // Keep the original for recovery instead of overwriting a damaged file.
            let backup = path.with_extension(format!("unreadable-{}.json", Utc::now().timestamp_millis()));
            if let Err(error) = std::fs::copy(path, &backup) {
                tracing::error!(%error, "couldn't back up the agent's conversations");
            }
            tracing::error!(%e, ?backup, "couldn't decode the agent's conversations");
            return Err(format!("The agent's conversations at {} couldn't be read: {e}. Saving them is paused to protect the file; restore it and restart nori.", path.display()));
        }
    };
    for doc in archive.documents.values_mut() {
        for thread in &mut doc.threads {
            for run in &mut thread.runs {
                run.checkpoint = None;
                if run.state == RunState::Running {
                    run.state = RunState::Cancelled;
                    run.activity = None;
                    run.finished_at = Some(Utc::now());
                }
            }
        }
    }
    Ok(archive)
}

pub(super) fn write(path: &Path, archive: &Archive) -> CmdResult<()> {
    let parent = path.parent().ok_or("No folder for the agent's conversations")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, archive).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| format!("Couldn't save the agent's conversations: {e}"))?;
    Ok(())
}
