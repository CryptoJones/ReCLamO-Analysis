//! Subprocess REPL — JSON Lines over stdin/stdout to `python3 -I worker.py`.
//!
//! Phase 2 wires this into the loop. The orchestrator constructs a
//! [`SubcallFn`] that dispatches sub-calls back through the active provider
//! with sub-call-specific [`CompleteOpts`]; the worker then has working
//! `llm_query(...)` / `llm_query_batched(...)` helpers (without stub
//! raising).
//!
//! Protocol (JSON Lines):
//!
//! - Parent writes (init): `{"type":"init","context":"...","context_format":"raw"}`
//! - Parent writes (exec):  `{"type":"exec","id":"...","code":"..."}`
//! - Parent writes (lookup):`{"type":"lookup","id":"...","name":"..."}`
//! - Parent writes (snapshot):`{"type":"snapshot","id":"..."}`
//! - Parent writes (shutdown):`{"type":"shutdown"}`
//! - Worker writes: `{"type":"ready"}` after init
//! - Worker writes (during exec):
//!     `{"type":"subcall_request","id":"...","prompt":"..."}`
//! - Parent writes (in response to sub-call):
//!     `{"type":"subcall_response","id":"...","result":"...","tokens":N,"ok":true}`
//! - Worker writes (end of exec):
//!     `{"type":"result","id":"...","ok":true,"stdout":"...","stderr":"...","subcalls":[...]}`
//! - Worker writes (protocol violation):
//!     `{"type":"error","id":"...","message":"..."}`

use super::{ReplClient, ReplExecResult, SubcallRecord};
use crate::error::{ReclamoError, ReclamoResult};
use async_trait::async_trait;
use serde_json::Value;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use uuid::Uuid;

const WORKER_PY: &str = include_str!("worker.py");

/// Future returned by a [`SubcallFn`]. Send + 'static so it can move
/// between tasks.
pub type SubcallFuture =
    Pin<Box<dyn std::future::Future<Output = ReclamoResult<(String, u64)>> + Send + 'static>>;

/// Closure that dispatches a sub-call prompt through the active provider
/// with sub-call-tuned options. Returns `(visible_text, tokens_used)`.
pub type SubcallFn = std::sync::Arc<dyn Fn(String) -> SubcallFuture + Send + Sync + 'static>;

/// Real subprocess REPL wired to the Python worker.
pub struct SubprocessRepl {
    child: Child,
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    subcall: SubcallFn,
}

impl SubprocessRepl {
    /// Spawn the worker. Writes the bundled worker.py to a temp file once,
    /// then `python3 -I <path>`. Caller is responsible for `shutdown`.
    pub async fn spawn(context: &str, subcall: SubcallFn) -> ReclamoResult<Self> {
        let tmp = tempfile::NamedTempFile::new()
            .map_err(|e| ReclamoError::Repl(format!("create tempdir: {e}")))?;
        std::fs::write(tmp.path(), WORKER_PY)
            .map_err(|e| ReclamoError::Repl(format!("write worker.py: {e}")))?;
        let mut cmd = Command::new("python3");
        cmd.arg("-I")
            .arg(tmp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // No key material in the child — v0.1 invariant.
        cmd.env_remove("ANTHROPIC_API_KEY");
        cmd.env_remove("OPENAI_API_KEY");
        let mut child = cmd
            .spawn()
            .map_err(|e| ReclamoError::Repl(format!("spawn python: {e}")))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ReclamoError::Repl("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ReclamoError::Repl("no stdout".into()))?;
        let mut reader = BufReader::new(stdout);

        // Init.
        let init = serde_json::json!({
            "type": "init",
            "context": context,
            "context_format": "raw",
        });
        writeln_json(&mut stdin, &init).await?;
        // Wait for ready.
        let line = read_line(&mut reader).await?;
        let v: Value = serde_json::from_str(&line)
            .map_err(|e| ReclamoError::Repl(format!("bad init reply: {e}; line={line}")))?;
        if v.get("type").and_then(|s| s.as_str()) != Some("ready") {
            return Err(ReclamoError::Repl(format!("expected ready, got {v}")));
        }
        Ok(Self {
            child,
            stdin,
            reader,
            subcall,
        })
    }

    /// Send an exec and pump the response stream until a result for our
    /// `exec_id` arrives, demuxing `subcall_request` to the sub-call handler
    /// and recording them in the returned `ReplExecResult.subcalls`.
    async fn run_exec(&mut self, code: &str) -> ReclamoResult<ReplExecResult> {
        let exec_id = Uuid::new_v4().to_string();
        let req = serde_json::json!({
            "type": "exec",
            "id": exec_id,
            "code": code,
        });
        writeln_json(&mut self.stdin, &req).await?;

        let mut subcalls: Vec<SubcallRecord> = Vec::new();
        let mut total_subcall_tokens: u64 = 0;

        loop {
            let line = read_line(&mut self.reader).await?;
            let v: Value = serde_json::from_str(&line)
                .map_err(|e| ReclamoError::Repl(format!("bad exec reply: {e}; line={line}")))?;
            let kind = v.get("type").and_then(|s| s.as_str()).unwrap_or("");
            match kind {
                "subcall_request" => {
                    let id = v
                        .get("id")
                        .and_then(|s| s.as_str())
                        .ok_or_else(|| {
                            ReclamoError::Repl("subcall_request missing id".into())
                        })?
                        .to_string();
                    let prompt = v
                        .get("prompt")
                        .and_then(|s| s.as_str())
                        .ok_or_else(|| {
                            ReclamoError::Repl("subcall_request missing prompt".into())
                        })?
                        .to_string();
                    let started = Instant::now();
                    let result = (self.subcall)(prompt.clone()).await;
                    let secs = started.elapsed().as_secs_f64();
                    let (text, tokens, ok, err_msg) = match result {
                        Ok((t, n)) => (t, n, true, String::new()),
                        Err(e) => (format!("[subcall error: {e}]"), 0u64, false, e.to_string()),
                    };
                    let resp = serde_json::json!({
                        "type": "subcall_response",
                        "id": id,
                        "result": text,
                        "ok": ok,
                        "tokens": tokens,
                        "error": err_msg,
                    });
                    writeln_json(&mut self.stdin, &resp).await?;
                    subcalls.push(SubcallRecord {
                        prompt,
                        response: text,
                        duration_secs: secs,
                        tokens,
                    });
                    total_subcall_tokens += tokens;
                }
                "result" => {
                    let id_match = v
                        .get("id")
                        .and_then(|s| s.as_str())
                        .map(|s| s == exec_id)
                        .unwrap_or(false);
                    if !id_match {
                        // Wrong id — keep reading. Shouldn't happen since
                        // only one exec is in flight at a time.
                        continue;
                    }
                    let ok = v.get("ok").and_then(|s| s.as_bool()).unwrap_or(false);
                    let stdout = v
                        .get("stdout")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let stderr = v
                        .get("stderr")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let error = v
                        .get("error")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string());
                    let made_progress =
                        !stdout.trim().is_empty() || !subcalls.is_empty();
                    return Ok(ReplExecResult {
                        ok,
                        stdout,
                        stderr,
                        error,
                        tokens: total_subcall_tokens,
                        subcalls,
                        made_progress,
                    });
                }
                "error" => {
                    let msg = v
                        .get("message")
                        .and_then(|s| s.as_str())
                        .unwrap_or("worker error");
                    return Err(ReclamoError::Repl(msg.to_string()));
                }
                _ => {
                    // Unknown — skip and try again.
                }
            }
        }
    }

    /// Send a generic request and read until a matching id'd reply comes
    /// back. Used for `lookup` / `snapshot` / `shutdown` which don't have
    /// sub-calls.
    async fn send_and_recv(&mut self, body: Value) -> ReclamoResult<Value> {
        let id = Uuid::new_v4().to_string();
        let mut full = body;
        if let Value::Object(ref mut map) = full {
            map.insert("id".into(), Value::String(id.clone()));
        }
        writeln_json(&mut self.stdin, &full).await?;
        loop {
            let line = read_line(&mut self.reader).await?;
            let v: Value = serde_json::from_str(&line)
                .map_err(|e| ReclamoError::Repl(format!("bad reply: {e}; line={line}")))?;
            if v.get("id").and_then(|s| s.as_str()) == Some(&id) {
                return Ok(v);
            }
            // Drop and try again.
        }
    }
}

#[async_trait]
impl ReplClient for SubprocessRepl {
    async fn execute(&mut self, code: &str) -> ReclamoResult<ReplExecResult> {
        self.run_exec(code).await
    }

    async fn lookup_var(&mut self, name: &str) -> ReclamoResult<Value> {
        let req = serde_json::json!({"type": "lookup", "name": name});
        let v = self.send_and_recv(req).await?;
        Ok(v.get("value").cloned().unwrap_or(Value::Null))
    }

    async fn snapshot_state(&mut self) -> ReclamoResult<Value> {
        let req = serde_json::json!({"type": "snapshot"});
        let v = self.send_and_recv(req).await?;
        Ok(v.get("value").cloned().unwrap_or(Value::Null))
    }

    async fn shutdown(&mut self) -> ReclamoResult<()> {
        let _ = writeln_json(&mut self.stdin, &serde_json::json!({"type": "shutdown"})).await;
        let _ = self.child.wait().await;
        Ok(())
    }
}

async fn writeln_json<W: AsyncWriteExt + Unpin>(w: &mut W, v: &Value) -> ReclamoResult<()> {
    let s = serde_json::to_string(v).map_err(|e| ReclamoError::Repl(format!("json: {e}")))?;
    w.write_all(s.as_bytes())
        .await
        .map_err(|e| ReclamoError::Repl(format!("write: {e}")))?;
    w.write_all(b"\n")
        .await
        .map_err(|e| ReclamoError::Repl(format!("write: {e}")))?;
    w.flush()
        .await
        .map_err(|e| ReclamoError::Repl(format!("flush: {e}")))?;
    Ok(())
}

async fn read_line<R: AsyncBufReadExt + Unpin>(r: &mut R) -> ReclamoResult<String> {
    let mut line = String::new();
    r.read_line(&mut line)
        .await
        .map_err(|e| ReclamoError::Repl(format!("read: {e}")))?;
    if line.is_empty() {
        return Err(ReclamoError::Repl("worker closed stdout".into()));
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    // SubprocessRepl needs a real Python interpreter. These tests are
    // `#[ignore]`'d by default; CI/local invocation can opt in with
    // `cargo test -- --ignored`.
    use super::*;

    fn dummy_subcall() -> SubcallFn {
        std::sync::Arc::new(|prompt: String| {
            Box::pin(async move { Ok((format!("echo: {prompt}"), 1u64)) })
        })
    }

    #[tokio::test]
    #[ignore = "needs python3 on PATH"]
    async fn spawn_and_exec_round_trip() {
        let mut r = SubprocessRepl::spawn("hello", dummy_subcall()).await.unwrap();
        let result = r.execute("print('hi')").await.unwrap();
        assert!(result.ok);
        assert!(result.stdout.contains("hi"));
        r.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "needs python3 on PATH"]
    async fn spawn_exec_with_subcall_round_trip() {
        let mut r = SubprocessRepl::spawn("ctx", dummy_subcall()).await.unwrap();
        let result = r
            .execute("text = llm_query('ping')\nprint(text)")
            .await
            .unwrap();
        assert!(result.ok, "exec failed: stderr={}", result.stderr);
        assert!(result.stdout.contains("echo: ping"));
        assert_eq!(result.subcalls.len(), 1);
        assert_eq!(result.subcalls[0].response, "echo: ping");
        assert_eq!(result.tokens, 1);
        r.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "needs python3 on PATH"]
    async fn lookup_var_returns_answer_after_exec() {
        let mut r = SubprocessRepl::spawn("ctx", dummy_subcall()).await.unwrap();
        r.execute("commit('the answer is 42')")
            .await
            .unwrap();
        let v = r.lookup_var("answer").await.unwrap();
        // The bootstrap leaves `answer` as a dict with `content`; commit
        // writes the content. Be liberal: either a dict with content
        // matching "the answer is 42", or the raw string.
        let s = match &v {
            Value::Object(map) => map
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            Value::String(s) => s.clone(),
            _ => v.to_string(),
        };
        assert!(
            s.contains("the answer is 42"),
            "expected answer to contain 'the answer is 42', got {v:?}"
        );
        r.shutdown().await.unwrap();
    }
}
