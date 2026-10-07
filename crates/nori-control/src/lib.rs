//! nori-control: the one door into nori.
//!
//! * [`registry`] names, validates and runs every command (`family.verb`).
//! * [`session::Session`] holds the open document and its single undo history, shared by the
//!   window, the built-in agent, `nori-cli` and `nori-mcp`.
//! * [`bridge`] lets the CLI and MCP drive the running app over a token-protected loopback
//!   socket; [`discovery`] tells other lsuite apps where nori is.
//! * [`account`] is the lsuite AI sign-in; [`plugins`] the plugin host.
//! * [`harness`] is what makes agents good here: the expert brief, skills, live context and checks.

pub mod account;
pub mod bridge;
pub mod commands;
pub mod diagnostics;
pub mod discovery;
pub mod harness;
pub mod plugins;
pub mod registry;
pub mod release_notes;
pub mod secrets;
pub mod session;
pub mod settings;
pub mod update;
pub mod vision;

pub use registry::{Kind, Param, Perm, Spec, call, commands as specs, describe, input_schema, markdown, spec};
pub use session::{CmdResult, CommandRecord, Event, Session, SessionOptions, Source, ToastKind, UiCall, UiState};
pub use settings::Settings;

#[cfg(test)]
mod tests;
