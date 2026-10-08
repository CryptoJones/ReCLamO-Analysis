//! REPL client trait + Phase 1 in-memory implementation.
//!
//! Phase 1 ships `InMemoryRepl` only — enough for unit tests. The
//! `SubprocessRepl` is sketched in `subprocess_repl.rs` and will be wired
//! in during phase 2, once the worker.py protocol is stable.
//!
//! The protocol: JSON Lines on stdin/stdout. `init` first (one line), then
//! `exec` / `lookup` / `snapshot` / `shutdown` indefinitely.

use crate::error::ReclamoResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::Mutex;

/// Blanket impl so `Box<dyn ReplClient>` works directly inside the loop.
/// Required because `async-trait` does NOT auto-implement `ReplClient` for
/// `Box<T>`.
#[async_trait]
impl<T: ReplClient + ?Sized> ReplClient for Box<T> {
    async fn execute(&mut self, code: &str) -> ReclamoResult<ReplExecResult> {
        (**self).execute(code).await
    }
    async fn lookup_var(&mut self, name: &str) -> ReclamoResult<Value> {
        (**self).lookup_var(name).await
    }
    async fn snapshot_state(&mut self) -> ReclamoResult<Value> {
        (**self).snapshot_state().await
    }
    async fn shutdown(&mut self) -> ReclamoResult<()> {
        (**self).shutdown().await
    }
}

/// One REPL execution result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplExecResult {
    /// `true` if the code ran without an unhandled exception.
    pub ok: bool,
    /// Captured stdout (truncated to ~2K chars by the worker).
    pub stdout: String,
    /// Captured stderr (truncated).
    pub stderr: String,
    /// Brief error class name (e.g. `NameError`) if `ok` is false.
    #[serde(default)]
    pub error: Option<String>,
    /// Tokens used by sub-calls issued inside this exec (currently 0).
    #[serde(default)]
    pub tokens: u64,
    /// Sub-call records (filled by the worker if any `llm_query` calls were made).
    #[serde(default)]
    pub subcalls: Vec<SubcallRecord>,
    /// Heuristic: did the exec produce observable output? (`True` means the
    /// model did something we can show progress on.)
    #[serde(default)]
    pub made_progress: bool,
}

impl ReplExecResult {
    /// Convenience constructor for the happy path.
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self {
            ok: true,
            stdout: stdout.into(),
            stderr: String::new(),
            error: None,
            tokens: 0,
            subcalls: vec![],
            made_progress: true,
        }
    }

    /// Convenience constructor for the unhappy path.
    pub fn err(class: impl Into<String>, message: impl Into<String>) -> Self {
        let made_progress = !message.into().is_empty();
        Self {
            ok: false,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(class.into()),
            tokens: 0,
            subcalls: vec![],
            made_progress,
        }
    }

    /// Push this exec result back into the conversation as a tool-style user
    /// message so the next turn sees it.
    pub fn to_user_message(&self) -> crate::providers::Message {
        let body = if self.ok {
            format!("EXECUTION RESULT (success):\n{}", self.stdout)
        } else {
            let cls = self.error.as_deref().unwrap_or("Error");
            format!("EXECUTION RESULT ({cls}):\n{}", self.stderr)
        };
        crate::providers::Message::user(body)
    }
}

/// One sub-call recorded inside an exec (`llm_query` etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubcallRecord {
    /// Prompt sent to the sub-model.
    pub prompt: String,
    /// Response text from the sub-model.
    pub response: String,
    /// Wall-clock seconds.
    pub duration_secs: f64,
    /// Tokens used.
    pub tokens: u64,
}

#[async_trait]
pub trait ReplClient: Send {
    /// Execute one Python block.
    async fn execute(&mut self, code: &str) -> ReclamoResult<ReplExecResult>;
    /// Look up a variable in the namespace. Returns `Value::Null` if absent.
    async fn lookup_var(&mut self, name: &str) -> ReclamoResult<Value>;
    /// Snapshot the namespace (best-effort) for forced-finish reporting.
    async fn snapshot_state(&mut self) -> ReclamoResult<Value>;
    /// Close the worker cleanly.
    async fn shutdown(&mut self) -> ReclamoResult<()>;
}

/// Phase 1 default: in-memory REPL, no Python subprocess. Phase 2 adds
/// `SubprocessRepl` and switches `default_client` once worker.py is stable.
pub fn default_client(_context: &str, _subcall_timeout: std::time::Duration) -> ReclamoResult<Box<dyn ReplClient>> {
    Ok(Box::new(InMemoryRepl::new()))
}

// Re-export for callers that want to build one explicitly (tests).
pub use in_memory_repl::InMemoryRepl;

mod in_memory_repl;
pub mod subprocess_repl;

// Convenience so callers don't have to write `Arc<dyn ReplClient>`.
pub type SharedRepl = Arc<Mutex<Box<dyn ReplClient>>>;
