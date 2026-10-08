//! What the person is looking at when they ask, sent with each request and refreshed before
//! every later model step (the harness's live context, `nori_control::harness::context`).
//!
//! The first block is part of the person's message; later ones are [`crate::Part::Context`]
//! parts after the tool results they follow. The thread is only ever appended to, so prompt
//! caches and thinking blocks stay valid.

use nori_control::{Session, Source};

pub use nori_control::harness::context::{Glance, unframed};

/// What the person sees now.
pub fn glance(session: &Session) -> Glance {
    nori_control::harness::context::glance(session, None, &[])
}

/// The context before a later step: what nori shows, and what others (the person in the
/// window, a script) changed after `since`; the agent's own changes (`mine`) aren't repeated.
pub fn refresh(session: &Session, since: u64, mine: Source) -> Glance {
    nori_control::harness::context::glance(session, Some(since), &[mine])
}
