//! kanna-keeper: creates Kanna's ptys and holds a copy of each, so the
//! processes in them outlive a daemon restart. It hands each pty master to
//! the daemon (`SCM_RIGHTS`) and is never in the path of terminal bytes.
//! See docs/keeper.md.

// Wired up by the server in the next step.
#![allow(dead_code)]

mod fdpass;
mod ghostty_env;
pub mod paths;
mod pty;
mod sig;

use serde_json::{json, Value};

/// What `hello` reports. Bumped when the protocol changes incompatibly.
pub const PROTOCOL_VERSION: u64 = 1;

/// How a process ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    /// It exited with this code.
    Code(i32),
    /// A signal killed it.
    Signal(i32),
}

impl ExitStatus {
    /// The status a shell would report: the code, or 128 + the signal.
    pub fn shell_status(self) -> i32 {
        match self {
            ExitStatus::Code(c) => c,
            ExitStatus::Signal(s) => 128 + s,
        }
    }

    pub(crate) fn to_json(self) -> Value {
        match self {
            ExitStatus::Code(c) => json!({"code": c}),
            ExitStatus::Signal(s) => json!({"signal": s}),
        }
    }

    pub(crate) fn from_json(v: &Value) -> Option<Self> {
        if let Some(c) = v["code"].as_i64() {
            return Some(ExitStatus::Code(c as i32));
        }
        v["signal"].as_i64().map(|s| ExitStatus::Signal(s as i32))
    }
}
