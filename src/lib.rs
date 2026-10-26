//! `cq`: a file-based message queue for Claude Code sessions.
//!
//! One binary with subcommands `init`, `send`, `watch`, `claim`, `done`,
//! `list`, `leave`, `members` and `session-start`. The library holds all queue logic so the integration tests can
//! check the message validator directly.

pub mod claim;
pub mod dispatch;
pub mod done;
pub mod error;
pub mod fsutil;
pub mod list;
pub mod members;
pub mod message;
pub mod names;
pub mod platform;
pub mod root;
pub mod send;
pub mod timefmt;
pub mod watch;

pub use error::{CqError, Result};
pub use root::Root;
