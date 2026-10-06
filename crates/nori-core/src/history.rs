//! Snapshot undo and redo. A step keeps the whole document as it was before it; since pixels
//! live in shared tiles (`raster.rs`), a snapshot costs a list of pointers and two steps share
//! every tile the edit between them didn't touch.
//!
//! Every client (the window, the agent, the CLI, MCP) edits through one [`Editor`], so they all
//! share this history. Each step remembers the command that made it and who ran it, so the
//! History panel and the Agent panel can say what changed and whose change it was.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::document::Document;
use crate::layer::{Adjustment, Content};

/// Steps kept at most (a setting can lower it), and the most memory the kept pixels may use.
pub const MAX_STEPS: usize = 100;
pub const DEFAULT_BUDGET: usize = 2 << 30;
const MAX_CHECKPOINTS: usize = 64;
const COALESCE_WINDOW: Duration = Duration::from_millis(1200);
static NEXT_CHECKPOINT: AtomicU64 = AtomicU64::new(1);

/// What one undo step did and who did it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StepInfo {
    /// The command that made the step, e.g. `paint.stroke`.
    pub label: String,
    /// `window`, `agent`, `cli` or `mcp`.
    pub source: String,
}

#[derive(Debug, Clone)]
struct Step {
    before: Document,
    info: StepInfo,
}

#[derive(Debug, Clone)]
struct Batch {
    levels: Vec<Level>,
    pushed: bool,
    info: StepInfo,
}

#[derive(Debug, Clone)]
struct Level {
    start: Document,
    undo_len: usize,
    redo: Vec<Step>,
    pushed: bool,
}

/// Why an edit was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    /// Another client's batch is open; its owner (`source`) has to finish first.
    Busy(String),
    Invalid(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Busy(who) => write!(f, "A batch of edits from {who} is in progress; try again when it ends."),
            EditError::Invalid(m) => f.write_str(m),
        }
    }
}

impl From<String> for EditError {
    fn from(s: String) -> Self {
        EditError::Invalid(s)
    }
}

impl From<&str> for EditError {
    fn from(s: &str) -> Self {
        EditError::Invalid(s.to_string())
    }
}

pub struct Editor {
    doc: Document,
    undo: Vec<Step>,
    redo: Vec<Step>,
    last_coalesce: Option<(String, Instant)>,
    dirty: bool,
    batch: Option<Batch>,
    checkpoints: BTreeMap<u64, Document>,
    current: StepInfo,
    outsider: bool,
    /// Bumped by every change (edits, undo, redo): views redraw when it moves.
    revision: u64,
    max_steps: usize,
    budget: usize,
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        Self {
            doc,
            undo: vec![],
            redo: vec![],
            last_coalesce: None,
            dirty: false,
            batch: None,
            checkpoints: BTreeMap::new(),
            current: StepInfo { label: "edit".into(), source: "window".into() },
            outsider: false,
            revision: 1,
            max_steps: MAX_STEPS,
            budget: DEFAULT_BUDGET,
        }
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// How many steps and how many bytes of pixels the history may keep.
    pub fn set_limits(&mut self, steps: usize, bytes: usize) {
        self.max_steps = steps.clamp(1, 1000);
        self.budget = bytes.max(64 << 20);
        self.trim();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The undo steps, oldest first.
    pub fn undo_steps(&self) -> Vec<StepInfo> {
        self.undo.iter().map(|s| s.info.clone()).collect()
    }

    /// The redo steps, the next redo first.
    pub fn redo_steps(&self) -> Vec<StepInfo> {
        self.redo.iter().rev().map(|s| s.info.clone()).collect()
    }

    /// Changes not yet saved to the document's file.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn set_step_info(&mut self, label: impl Into<String>, source: impl Into<String>) {
        self.current = StepInfo { label: label.into(), source: source.into() };
    }

    pub fn set_outsider(&mut self, outsider: bool) {
        self.outsider = outsider;
    }

    fn busy(&self) -> Result<(), EditError> {
        match &self.batch {
            Some(b) if self.outsider => Err(EditError::Busy(b.info.source.clone())),
            _ => Ok(()),
        }
    }

    /// Changes the document with `f`, as one undo step (or part of the open batch). If `f`
    /// fails, the document is put back as it was. Changes sharing a `coalesce` key within a
    /// second or so (a slider being dragged) are one step; a `gesture:` key keeps a gesture
    /// together however slow it is. A change that changes nothing is no step.
    pub fn change<R>(&mut self, coalesce: Option<&str>, f: impl FnOnce(&mut Document) -> Result<R, String>) -> Result<R, EditError> {
        self.busy()?;
        let before = self.doc.clone();
        let out = match f(&mut self.doc) {
            Ok(out) => out,
            Err(e) => {
                self.doc = before;
                return Err(EditError::Invalid(e));
            }
        };
        if same(&before, &self.doc) {
            return Ok(out);
        }
        self.dirty = true;
        self.revision += 1;
        self.record(before, coalesce);
        Ok(out)
    }

    /// Replaces the whole document as one undo step (a revert to a checkpoint).
    pub fn replace(&mut self, doc: Document) -> Result<(), EditError> {
        self.busy()?;
        if same(&doc, &self.doc) {
            return Ok(());
        }
        let before = std::mem::replace(&mut self.doc, doc);
        self.dirty = true;
        self.revision += 1;
        self.record(before, None);
        Ok(())
    }

    fn record(&mut self, before: Document, coalesce: Option<&str>) {
        self.redo.clear();
        if let Some(batch) = &mut self.batch {
            if !batch.pushed {
                batch.pushed = true;
                self.undo.push(Step { before, info: batch.info.clone() });
            }
            self.last_coalesce = None;
            return;
        }
        let now = Instant::now();
        let merge = match (coalesce, &self.last_coalesce) {
            (Some(key), Some((last, at))) => {
                let gesture = key.strip_prefix("gesture:").is_some_and(|id| !id.is_empty());
                key == last && (gesture || now.duration_since(*at) < COALESCE_WINDOW) && self.undo.last().is_some_and(|s| s.info.source == self.current.source)
            }
            _ => false,
        };
        if !merge {
            self.undo.push(Step { before, info: self.current.clone() });
        }
        self.last_coalesce = coalesce.map(|k| (k.to_string(), now));
        self.trim();
    }

    /// Drops the oldest steps past the step limit or the memory budget (never the last one).
    fn trim(&mut self) {
        if self.batch.is_some() {
            return;
        }
        if self.undo.len() > self.max_steps {
            let extra = self.undo.len() - self.max_steps;
            self.undo.drain(..extra);
        }
        while self.undo.len() > 1 && self.history_bytes() > self.budget {
            self.undo.remove(0);
        }
    }

    /// Bytes of pixels the history keeps beyond the document itself (each tile counted once).
    pub fn history_bytes(&self) -> usize {
        let mut seen: HashSet<usize> = HashSet::new();
        tiles_of(&self.doc, &mut seen, &mut 0);
        let mut bytes = 0;
        for s in self.undo.iter().chain(self.redo.iter()) {
            tiles_of(&s.before, &mut seen, &mut bytes);
        }
        bytes
    }

    pub fn begin_batch(&mut self, label: impl Into<String>, source: impl Into<String>) {
        let pushed = self.batch.as_ref().is_some_and(|b| b.pushed);
        let level = Level { start: self.doc.clone(), undo_len: self.undo.len(), redo: self.redo.clone(), pushed };
        match &mut self.batch {
            Some(b) => b.levels.push(level),
            None => self.batch = Some(Batch { levels: vec![level], pushed: false, info: StepInfo { label: label.into(), source: source.into() } }),
        }
    }

    pub fn end_batch(&mut self) {
        if let Some(b) = &mut self.batch {
            b.levels.pop();
            if b.levels.is_empty() {
                self.batch = None;
                self.last_coalesce = None;
                self.trim();
            }
        }
    }

    pub fn rollback_batch(&mut self) {
        let Some(b) = &mut self.batch else { return };
        let Some(l) = b.levels.pop() else { return };
        b.pushed = l.pushed;
        if b.levels.is_empty() {
            self.batch = None;
        }
        self.doc = l.start;
        self.undo.truncate(l.undo_len);
        self.redo = l.redo;
        self.last_coalesce = None;
        self.revision += 1;
    }

    pub fn in_batch(&self) -> bool {
        self.batch.is_some()
    }

    pub fn checkpoint(&mut self) -> u64 {
        let id = NEXT_CHECKPOINT.fetch_add(1, Ordering::Relaxed);
        self.checkpoints.insert(id, self.doc.clone());
        while self.checkpoints.len() > MAX_CHECKPOINTS {
            let oldest = *self.checkpoints.keys().next().expect("not empty");
            self.checkpoints.remove(&oldest);
        }
        id
    }

    /// Back to `checkpoint` as one new step; false for a checkpoint this history doesn't have.
    pub fn revert_to(&mut self, checkpoint: u64) -> Result<bool, EditError> {
        let Some(d) = self.checkpoints.get(&checkpoint).cloned() else { return Ok(false) };
        self.replace(d)?;
        Ok(true)
    }

    pub fn has_checkpoint(&self, checkpoint: u64) -> bool {
        self.checkpoints.contains_key(&checkpoint)
    }

    pub fn is_at_checkpoint(&self, checkpoint: u64) -> bool {
        self.checkpoints.get(&checkpoint).is_some_and(|d| same(d, &self.doc))
    }

    pub fn undo(&mut self) -> bool {
        if self.batch.is_some() {
            return false;
        }
        let Some(step) = self.undo.pop() else { return false };
        let after = std::mem::replace(&mut self.doc, step.before);
        self.redo.push(Step { before: after, info: step.info });
        self.last_coalesce = None;
        self.dirty = true;
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.batch.is_some() {
            return false;
        }
        let Some(step) = self.redo.pop() else { return false };
        let before = std::mem::replace(&mut self.doc, step.before);
        self.undo.push(Step { before, info: step.info });
        self.last_coalesce = None;
        self.dirty = true;
        self.revision += 1;
        true
    }

    /// Undoes or redoes until `undo_len` steps are left to undo (the History panel's click).
    pub fn go_to(&mut self, undo_len: usize) -> bool {
        let mut moved = false;
        while self.undo.len() > undo_len && self.undo() {
            moved = true;
        }
        while self.undo.len() < undo_len && self.redo() {
            moved = true;
        }
        moved
    }
}

/// Whether two documents are the same: equal settings and layers, and the very same tiles.
pub fn same(a: &Document, b: &Document) -> bool {
    if serde_json::to_value(a).ok() != serde_json::to_value(b).ok() {
        return false;
    }
    let (la, lb) = (a.all(), b.all());
    if la.len() != lb.len() {
        return false;
    }
    let planes_same = |x: &crate::raster::Raster, y: &crate::raster::Raster| {
        let (gx, gy) = x.grid();
        (0..gy).all(|ty| (0..gx).all(|tx| x.same_tile(y, tx, ty)))
    };
    let masks_same = |x: &crate::raster::Mask, y: &crate::raster::Mask| {
        let (gx, gy) = x.grid();
        (0..gy).all(|ty| (0..gx).all(|tx| x.same_tile(y, tx, ty)))
    };
    for (x, y) in la.iter().zip(lb.iter()) {
        if let (Some((_, _, p)), Some((_, _, q))) = (x.raster(), y.raster())
            && !planes_same(p, q)
        {
            return false;
        }
        match (&x.mask, &y.mask) {
            (Some(m), Some(n)) if !masks_same(&m.mask, &n.mask) => return false,
            _ => {}
        }
        if let (Content::Adjustment { adjustment: Adjustment::Lut { table: t, .. } }, Content::Adjustment { adjustment: Adjustment::Lut { table: u, .. } }) = (&x.content, &y.content)
            && !Arc::ptr_eq(t, u)
        {
            return false;
        }
    }
    match (&a.selection, &b.selection) {
        (Some(m), Some(n)) => masks_same(m, n),
        _ => true,
    }
}

fn tiles_of(d: &Document, seen: &mut HashSet<usize>, bytes: &mut usize) {
    let mut add = |arc: Option<&Arc<Vec<u8>>>| {
        if let Some(a) = arc
            && seen.insert(Arc::as_ptr(a) as usize)
        {
            *bytes += a.len();
        }
    };
    for l in d.all() {
        if let Some((_, _, p)) = l.raster() {
            let (gx, gy) = p.grid();
            for ty in 0..gy {
                for tx in 0..gx {
                    add(p.tile_arc(tx, ty));
                }
            }
        }
        if let Some(m) = &l.mask {
            let (gx, gy) = m.mask.grid();
            for ty in 0..gy {
                for tx in 0..gx {
                    add(m.mask.tile_arc(tx, ty));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    fn paint(d: &mut Document, x: i32, v: u8) -> Result<(), String> {
        let id = d.active_id().ok_or("no layer")?;
        let l = d.layer_mut(&id).ok_or("no layer")?;
        let (_, _, p) = l.raster_mut().ok_or("not pixels")?;
        p.set(x, 0, [v, v, v, 255]);
        Ok(())
    }

    #[test]
    fn undo_and_redo_pixels_and_share_tiles() {
        let mut ed = Editor::new(Document::new("t", 600, 300, Some(Color::WHITE)));
        ed.change(None, |d| paint(d, 1, 0)).unwrap();
        ed.change(None, |d| paint(d, 500, 0)).unwrap();
        assert_eq!(ed.undo_steps().len(), 2);
        let px = |ed: &Editor, x| ed.doc().page().layers[0].raster().unwrap().2.get(x, 0);
        assert_eq!(px(&ed, 1), [0, 0, 0, 255]);
        assert!(ed.undo());
        assert_eq!(px(&ed, 500), [255, 255, 255, 255]);
        assert_eq!(px(&ed, 1), [0, 0, 0, 255]);
        assert!(ed.undo());
        assert_eq!(px(&ed, 1), [255, 255, 255, 255]);
        assert!(ed.redo());
        assert!(ed.redo());
        assert_eq!(px(&ed, 500), [0, 0, 0, 255]);
        // Two edits on two tiles: the history keeps two tiles beyond the document, not two images.
        assert_eq!(ed.history_bytes(), 2 * (crate::TILE * crate::TILE * 4) as usize);
    }

    #[test]
    fn failed_changes_leave_nothing_and_no_ops_make_no_step() {
        let mut ed = Editor::new(Document::new("t", 10, 10, None));
        let r: Result<(), _> = ed.change(None, |d| {
            d.name = "changed".into();
            Err("nope".to_string())
        });
        assert!(r.is_err());
        assert_eq!(ed.doc().name, "t");
        ed.change(None, |_| Ok(())).unwrap();
        assert!(!ed.can_undo());
        assert!(!ed.is_dirty());
    }

    #[test]
    fn coalescing_batches_and_checkpoints() {
        let mut ed = Editor::new(Document::new("t", 10, 10, None));
        for i in 0..5 {
            ed.change(Some("opacity"), |d| {
                d.page_mut().layers[0].opacity = i as f32 / 10.0;
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(ed.undo_steps().len(), 1);
        let cp = ed.checkpoint();
        ed.begin_batch("batch", "agent");
        ed.change(None, |d| paint(d, 1, 9)).unwrap();
        ed.change(None, |d| {
            d.name = "x".into();
            Ok(())
        })
        .unwrap();
        ed.end_batch();
        assert_eq!(ed.undo_steps().len(), 2);
        assert_eq!(ed.undo_steps()[1].source, "agent");
        assert!(ed.revert_to(cp).unwrap());
        assert_eq!(ed.doc().name, "t");
        assert!(ed.is_at_checkpoint(cp));
        assert!(ed.go_to(0));
        assert!(!ed.can_undo());
        assert_eq!(ed.redo_steps().len(), 3);
    }

    #[test]
    fn outsiders_wait_for_a_batch() {
        let mut ed = Editor::new(Document::new("t", 10, 10, None));
        ed.begin_batch("b", "agent");
        ed.set_outsider(true);
        assert!(matches!(ed.change(None, |d| paint(d, 0, 1)), Err(EditError::Busy(_))));
        ed.set_outsider(false);
        ed.rollback_batch();
        assert!(!ed.in_batch());
    }

    #[test]
    fn memory_budget_drops_old_steps() {
        let mut ed = Editor::new(Document::new("t", 2048, 256, Some(Color::WHITE)));
        ed.set_limits(100, 64 << 20);
        // Each step rewrites every tile of a 2048×256 layer (8 tiles, 2 MB).
        for i in 0..60u8 {
            ed.change(None, |d| {
                let (_, _, p) = d.page_mut().layers[0].raster_mut().unwrap();
                for tx in 0..8 {
                    p.set(tx * 256, 0, [i, 0, 0, 255]);
                }
                Ok(())
            })
            .unwrap();
        }
        assert!(ed.history_bytes() <= 64 << 20);
        assert!(ed.undo_steps().len() < 60);
    }
}
