//! `history.*`: the one undo history every client shares.

use std::sync::Arc;

use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "history.list" => s.read(|ed| {
            json!({
                "undo": ed.undo_steps().iter().map(|st| json!({ "command": st.label, "by": st.source })).collect::<Vec<_>>(),
                "redo": ed.redo_steps().iter().map(|st| json!({ "command": st.label, "by": st.source })).collect::<Vec<_>>(),
            })
        }),
        "history.undo" => {
            let step = s.read(|ed| ed.undo_steps().last().cloned())?;
            let done = s.with_editor(|ed| ed.undo())?;
            if !done {
                return Err("Nothing to undo.".into());
            }
            Ok(json!({ "undone": true, "step": step.as_ref().map(|s| s.label.clone()), "source": step.map(|s| s.source) }))
        }
        "history.redo" => {
            let step = s.read(|ed| ed.redo_steps().first().cloned())?;
            let done = s.with_editor(|ed| ed.redo())?;
            if !done {
                return Err("Nothing to redo.".into());
            }
            Ok(json!({ "redone": true, "step": step.as_ref().map(|s| s.label.clone()), "source": step.map(|s| s.source) }))
        }
        "history.goTo" => {
            let n = a.opt_i64("steps").unwrap_or(0).max(0) as usize;
            s.with_editor(|ed| ed.go_to(n))?;
            let (u, r) = s.read(|ed| (ed.undo_steps().len(), ed.redo_steps().len()))?;
            Ok(json!({ "undo": u, "redo": r }))
        }
        "history.checkpoint" => {
            let (_, cp) = s.checkpoint().ok_or(crate::session::NO_DOCUMENT)?;
            Ok(json!({ "checkpoint": cp }))
        }
        "history.revertTo" => {
            let cp = a.opt_i64("checkpoint").unwrap_or(0) as u64;
            let source = cx.source;
            let ok = s.with_editor(|ed| {
                ed.set_step_info("history.revertTo", source.as_str());
                ed.revert_to(cp)
            })?;
            match ok {
                Ok(true) => Ok(json!({ "reverted": true })),
                Ok(false) => Err("That checkpoint isn't in this document's history (another document's, or too old).".into()),
                Err(e) => Err(e.to_string()),
            }
        }
        _ => Err(super::unhandled(cx)),
    }
}
