//! In-memory REPL for phase-1 tests. No Python subprocess; behavior is
//! scripted via `on_exec` and `on_lookup`.

use super::{ReplClient, ReplExecResult};
use crate::error::{ReclamoError, ReclamoResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Mutex;

/// Scripted executor signature.
pub type ExecFn = Box<dyn Fn(&str, &Value) -> ReplExecResult + Send + Sync>;

/// Scripted variable-lookup signature.
pub type LookupFn = Box<dyn Fn(&str, &Value) -> Value + Send + Sync>;

/// In-memory REPL for tests.
pub struct InMemoryRepl {
    state: Mutex<Value>,
    exec_fn: Option<ExecFn>,
    lookup_fn: Option<LookupFn>,
}

impl InMemoryRepl {
    /// Build a fresh, empty in-memory REPL. The initial state mirrors the
    /// worker's bootstrap (`answer = {"content": ""}` and a few helpers).
    pub fn new() -> Self {
        Self {
            state: Mutex::new(json!({
                "answer": {"content": ""},
                "__helpers_initialized__": true,
            })),
            exec_fn: None,
            lookup_fn: None,
        }
    }

    /// Set a custom executor. The closure receives `(code, current_state)`
    /// and returns a `ReplExecResult`.
    pub fn on_exec<F>(mut self, f: F) -> Self
    where
        F: Fn(&str, &Value) -> ReplExecResult + Send + Sync + 'static,
    {
        self.exec_fn = Some(Box::new(f));
        self
    }

    /// Set a custom lookup function. The closure receives
    /// `(name, current_state)` and returns the JSON value.
    pub fn on_lookup<F>(mut self, f: F) -> Self
    where
        F: Fn(&str, &Value) -> Value + Send + Sync + 'static,
    {
        self.lookup_fn = Some(Box::new(f));
        self
    }

    /// Insert a variable into the namespace from the test side (mirrors the
    /// worker receiving a `FINAL_VAR` lookup of a name that exists).
    pub fn put(&self, key: impl Into<String>, value: Value) {
        let mut g = self.state.lock().unwrap();
        if let Value::Object(map) = &mut *g {
            map.insert(key.into(), value);
        }
    }
}

impl Default for InMemoryRepl {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ReplClient for InMemoryRepl {
    async fn execute(&mut self, code: &str) -> ReclamoResult<ReplExecResult> {
        let state = self.state.lock().map_err(|e| ReclamoError::Repl(format!("mutex poisoned: {e}")))?.clone();
        let r = match &self.exec_fn {
            Some(f) => f(code, &state),
            None => ReplExecResult::ok("(in-memory; no script)"),
        };
        // Side-effects from the script: a real REPL would have actually run
        // the Python, but for testing the script itself does its own state
        // bookkeeping via the closure. So we don't second-guess it here.
        Ok(r)
    }

    async fn lookup_var(&mut self, name: &str) -> ReclamoResult<Value> {
        let state = self.state.lock().map_err(|e| ReclamoError::Repl(format!("mutex poisoned: {e}")))?.clone();
        Ok(match &self.lookup_fn {
            Some(f) => f(name, &state),
            None => state.get(name).cloned().unwrap_or(Value::Null),
        })
    }

    async fn snapshot_state(&mut self) -> ReclamoResult<Value> {
        Ok(self.state.lock().map_err(|e| ReclamoError::Repl(format!("mutex poisoned: {e}")))?.clone())
    }

    async fn shutdown(&mut self) -> ReclamoResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Message;

    #[tokio::test]
    async fn executes_via_script() {
        let mut repl = InMemoryRepl::new().on_exec(|code, _| {
            ReplExecResult::ok(format!("ran `{code}`"))
        });
        let r = repl.execute("print(1)").await.unwrap();
        assert!(r.ok);
        assert!(r.stdout.contains("print"));
    }

    #[tokio::test]
    async fn lookup_returns_explicit_value() {
        let mut repl = InMemoryRepl::new()
            .on_lookup(|name, _| json!({"name": name, "kind": "stub"}));
        let v = repl.lookup_var("answer").await.unwrap();
        assert_eq!(v["name"], "answer");
        assert_eq!(v["kind"], "stub");
    }

    #[tokio::test]
    async fn default_lookup_returns_null_for_missing() {
        let mut repl = InMemoryRepl::new();
        let v = repl.lookup_var("missing").await.unwrap();
        assert!(v.is_null());
    }

    #[tokio::test]
    async fn ok_message_is_user_tool_body() {
        let r = ReplExecResult::ok("hello\n");
        let msg: Message = r.to_user_message();
        assert_eq!(msg.role, crate::providers::Role::User);
        assert!(msg.content.contains("EXECUTION RESULT"));
        assert!(msg.content.contains("hello"));
    }

    #[tokio::test]
    async fn error_message_includes_class() {
        let r = ReplExecResult::err("NameError", "x is not defined");
        let msg = r.to_user_message();
        assert!(msg.content.contains("NameError"));
        assert!(!msg.content.contains("ok=false, "));
    }

    #[tokio::test]
    async fn put_makes_value_visible_to_lookup() {
        let mut repl = InMemoryRepl::new();
        repl.put("answer", json!("42"));
        let v = repl.lookup_var("answer").await.unwrap();
        assert_eq!(v, "42");
    }
}
