//! Subprocess REPL — JSON Lines over stdin/stdout to `python3 -I worker.py`.
//!
//! Phase 2 will activate this. Phase 1 builds and unit-tests the in-memory
//! variant only (avoids needing Python on the host for `cargo test`).
//!
//! Protocol (JSON Lines):
//!
//! - Parent writes (init): `{"type":"init","context":"...","context_format":"raw"}`
//! - Parent writes (exec):  `{"type":"exec","id":"...","code":"..."}`
//! - Parent writes (lookup):`{"type":"lookup","id":"...","name":"answer"}`
//! - Parent writes (snapshot):`{"type":"snapshot","id":"..."}`
//! - Parent writes (shutdown):`{"type":"shutdown"}`
//! - Worker writes: `{"type":"ready"}` after init
//! - Worker writes: `{"type":"result","id":"...","ok":true,"stdout":"...","stderr":"..."}`
//!                  or `{"type":"error","id":"...","message":"..."}`

use self::subprocess_protocol::{Line, Request, Response};
use super::{ReplClient, ReplExecResult};
use crate::error::{ReclamoError, ReclamoResult};
use async_trait::async_trait;
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use uuid::Uuid;

const WORKER_PY: &str = include_str!("worker.py");

/// Phase-2 stub. The harness will switch `default_client` to this once
/// worker.py is stable against the protocol frozen here. Currently
/// unimplemented: spawning it would write `worker.py` to a tempfile and
/// `python3 -I <path>` it.
pub struct SubprocessRepl {
    child: Child,
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
}

impl SubprocessRepl {
    /// Spawn the worker. Writes the bundled worker.py to a temp file once,
    /// then `python3 -I <path>`. Caller is responsible for `shutdown`.
    pub async fn spawn(context: &str) -> ReclamoResult<Self> {
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
        // Make sure the child has no key material (extra defense — the
        // v0.1 design carries no secrets into the REPL).
        cmd.env_remove("ANTHROPIC_API_KEY");
        cmd.env_remove("OPENAI_API_KEY");
        let mut child = cmd
            .spawn()
            .map_err(|e| ReclamoError::Repl(format!("spawn python: {e}")))?;
        let mut stdin = child.stdin.take().ok_or_else(|| ReclamoError::Repl("no stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| ReclamoError::Repl("no stdout".into()))?;
        let mut reader = BufReader::new(stdout);

        // Send init.
        let init = serde_json::json!({
            "type": "init",
            "context": context,
            "context_format": "raw",
        });
        writeln_json(&mut stdin, &init).await?;
        // Read "ready".
        let line = read_line(&mut reader).await?;
        let _: Line = serde_json::from_str(&line)
            .map_err(|e| ReclamoError::Repl(format!("bad init reply: {e}; line={line}")))?;
        Ok(Self { child, stdin, reader })
    }

    async fn write_request(&mut self, req: Request) -> ReclamoResult<String> {
        let id = req.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut with_id = req;
        with_id.id = Some(id.clone());
        writeln_json(&mut self.stdin, &serde_json::to_value(&with_id).unwrap_or_default()).await?;
        // Loop until we see a response with our id.
        loop {
            let line = read_line(&mut self.reader).await?;
            let parsed: Response = serde_json::from_str(&line)
                .map_err(|e| ReclamoError::Repl(format!("bad reply: {e}; line={line}")))?;
            if parsed.id.as_deref() == Some(id.as_str()) {
                return Ok(line);
            }
            // Drop and try again.
        }
    }
}

#[async_trait]
impl ReplClient for SubprocessRepl {
    async fn execute(&mut self, code: &str) -> ReclamoResult<ReplExecResult> {
        let req = Request { kind: "exec".into(), id: None, code: Some(code.into()), name: None };
        let raw = self.write_request(req).await?;
        let resp: Response = serde_json::from_str(&raw)
            .map_err(|e| ReclamoError::Repl(format!("bad reply: {e}; raw={raw}")))?;
        Ok(resp.into_exec_result())
    }

    async fn lookup_var(&mut self, name: &str) -> ReclamoResult<Value> {
        let req = Request { kind: "lookup".into(), id: None, code: None, name: Some(name.into()) };
        let raw = self.write_request(req).await?;
        let resp: Response = serde_json::from_str(&raw)
            .map_err(|e| ReclamoError::Repl(format!("bad reply: {e}; raw={raw}")))?;
        Ok(resp.value.unwrap_or(Value::Null))
    }

    async fn snapshot_state(&mut self) -> ReclamoResult<Value> {
        let req = Request { kind: "snapshot".into(), id: None, code: None, name: None };
        let raw = self.write_request(req).await?;
        let resp: Response = serde_json::from_str(&raw)
            .map_err(|e| ReclamoError::Repl(format!("bad reply: {e}; raw={raw}")))?;
        Ok(resp.value.unwrap_or(Value::Null))
    }

    async fn shutdown(&mut self) -> ReclamoResult<()> {
        let _ = writeln_json(&mut self.stdin, &serde_json::json!({"type": "shutdown"})).await;
        let _ = self.child.wait().await;
        Ok(())
    }
}

async fn writeln_json<W: AsyncWriteExt + Unpin>(w: &mut W, v: &Value) -> ReclamoResult<()> {
    let s = serde_json::to_string(v).map_err(|e| ReclamoError::Repl(format!("json: {e}")))?;
    w.write_all(s.as_bytes()).await.map_err(|e| ReclamoError::Repl(format!("write: {e}")))?;
    w.write_all(b"\n").await.map_err(|e| ReclamoError::Repl(format!("write: {e}")))?;
    w.flush().await.map_err(|e| ReclamoError::Repl(format!("flush: {e}")))?;
    Ok(())
}

async fn read_line<R: AsyncBufReadExt + Unpin>(r: &mut R) -> ReclamoResult<String> {
    let mut line = String::new();
    r.read_line(&mut line).await.map_err(|e| ReclamoError::Repl(format!("read: {e}")))?;
    if line.is_empty() {
        return Err(ReclamoError::Repl("worker closed stdout".into()));
    }
    Ok(line)
}

mod subprocess_protocol {
    //! Wire-format types for the JSON Lines protocol.
    #[allow(unused_imports)]
    use crate::repl::ReplExecResult;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    #[derive(Debug, Serialize, Deserialize)]
    #[allow(dead_code)]
    pub struct Line(pub serde_json::Value);

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Request {
        #[serde(rename = "type")]
        pub kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub name: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Response {
        #[serde(rename = "type")]
        pub kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub ok: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub stdout: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub stderr: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub value: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub message: Option<String>,
    }

    impl Response {
        pub fn into_exec_result(self) -> ReplExecResult {
            let stdout = self.stdout.clone().unwrap_or_default();
            let made_progress = !stdout.trim().is_empty();
            ReplExecResult {
                ok: self.ok.unwrap_or(false),
                stdout,
                stderr: self.stderr.unwrap_or_default(),
                error: self.error.or(self.message),
                tokens: 0,
                subcalls: vec![],
                made_progress,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // SubprocessRepl needs a real Python interpreter. These tests are
    // `#[ignore]`'d by default; CI/local invocation can opt in with
    // `cargo test -- --ignored`.
    use super::*;

    #[tokio::test]
    #[ignore = "needs python3 on PATH"]
    async fn spawn_and_exec_round_trip() {
        let mut r = SubprocessRepl::spawn("hello").await.unwrap();
        let result = r.execute("print('hi')").await.unwrap();
        assert!(result.ok);
        assert!(result.stdout.contains("hi"));
        r.shutdown().await.unwrap();
    }
}
