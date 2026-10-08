//! JSONL trajectory logger.
//!
//! Schema is close to v0.1's so the visualizer can point at either:
//!
//! - One metadata line at the start (model, profile, started_at, run_id).
//! - One line per root turn (turn index, messages sent, completion received,
//!   parsed result, exec results).
//! - One line per sub-call (which REPL call triggered it, the prompt, the
//!   response, the duration, tokens).
//!
//! One file per run, written as we go (so a long run doesn't lose work to a
//! crash). The default location is `./runs/<utc>/<run-id>.jsonl`; the CLI
//! and the eval rig can pass in any directory.

use crate::error::ReclamoResult;
use crate::providers::{Completion, Message};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct MetaLine {
    pub kind: &'static str, // "meta"
    pub run_id: String,
    pub started_at: String,
    pub model: String,
    pub provider: String,
    pub max_context: u64,
    pub max_iterations: u32,
    pub max_subcalls_per_run: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnLine {
    pub kind: &'static str, // "turn"
    pub run_id: String,
    pub turn: u32,
    pub max_iterations: u32,
    pub messages: Vec<Message>,
    pub completion: Completion,
    pub parsed_code_blocks: u32,
    pub parsed_final: Option<String>,
    pub reject_reason: Option<String>,
    pub cumulative_tokens: u64,
    pub cumulative_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubcallLine {
    pub kind: &'static str, // "subcall"
    pub run_id: String,
    pub turn: u32,
    pub call_id: String,
    pub prompt_chars: usize,
    pub response_chars: usize,
    pub batched: bool,
    pub duration_secs: f64,
    pub tokens: u64,
    pub truncated: bool,
}

pub struct RunLogger {
    inner: Mutex<Option<BufWriter<std::fs::File>>>,
    run_id: String,
    #[allow(dead_code)]
    started_at: String,
    path: PathBuf,
}

impl RunLogger {
    /// Open a new logger at `log_dir/<run-id>.jsonl`. Creates the dir.
    /// `model` and `provider` are reserved for header composition and are
    /// not used in the file path (the run_id is the uuid suffix).
    pub fn open(log_dir: &Path, _model: &str, _provider: &str) -> ReclamoResult<Self> {
        std::fs::create_dir_all(log_dir)?;
        let run_id = Uuid::new_v4().to_string();
        let started_at = chrono::Utc::now().to_rfc3339();
        let path = log_dir.join(format!("{run_id}.jsonl"));
        let f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            inner: Mutex::new(Some(BufWriter::new(f))),
            run_id,
            started_at,
            path,
        })
    }

    /// Open at an explicit file path (for tests).
    pub fn open_at(path: &Path) -> ReclamoResult<Self> {
        let f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        Ok(Self {
            inner: Mutex::new(Some(BufWriter::new(f))),
            run_id: Uuid::new_v4().to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
            path: path.to_path_buf(),
        })
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write one metadata line. Call once at start.
    pub fn meta(&self, m: MetaLine) -> ReclamoResult<()> {
        self.write(&m)
    }

    pub fn turn(&self, t: TurnLine) -> ReclamoResult<()> {
        self.write(&t)
    }

    pub fn subcall(&self, s: SubcallLine) -> ReclamoResult<()> {
        self.write(&s)
    }

    /// Flush and close the underlying writer.
    pub fn finish(&self) -> ReclamoResult<()> {
        let mut g = self.inner.lock().map_err(|e| {
            crate::error::ReclamoError::Config(format!("logger mutex poisoned: {e}"))
        })?;
        if let Some(w) = g.as_mut() {
            w.flush().map_err(|e| crate::error::ReclamoError::Config(format!("flush: {e}")))?;
        }
        Ok(())
    }

    fn write<T: Serialize>(&self, v: &T) -> ReclamoResult<()> {
        let mut g = self.inner.lock().map_err(|e| {
            crate::error::ReclamoError::Config(format!("logger mutex poisoned: {e}"))
        })?;
        let Some(w) = g.as_mut() else {
            return Ok(());
        };
        let s = serde_json::to_string(v).map_err(|e| {
            crate::error::ReclamoError::Config(format!("serialize log line: {e}"))
        })?;
        writeln!(w, "{s}").map_err(|e| {
            crate::error::ReclamoError::Config(format!("write log line: {e}"))
        })?;
        Ok(())
    }
}

/// Build a [`MetaLine`] from a profile and loop config.
pub fn make_meta(
    run_id: &str,
    started_at: &str,
    model: &str,
    provider: &str,
    max_context: u64,
    max_iterations: u32,
    max_subcalls_per_run: u32,
) -> MetaLine {
    MetaLine {
        kind: "meta",
        run_id: run_id.into(),
        started_at: started_at.into(),
        model: model.into(),
        provider: provider.into(),
        max_context,
        max_iterations,
        max_subcalls_per_run,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Usage;
    use tempfile::tempdir;

    #[test]
    fn open_writes_meta_line() {
        let dir = tempdir().unwrap();
        let l = RunLogger::open(dir.path(), "m", "anthropic").unwrap();
        l.meta(make_meta(l.run_id(), "t", "m", "anthropic", 100_000, 20, 64))
            .unwrap();
        l.finish().unwrap();
        let content = std::fs::read_to_string(l.path()).unwrap();
        assert!(content.contains("\"kind\":\"meta\""));
        assert!(content.contains("\"model\":\"m\""));
        assert!(content.contains("\"provider\":\"anthropic\""));
    }

    #[test]
    fn open_at_writes_finite_path() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("r.jsonl");
        let l = RunLogger::open_at(&p).unwrap();
        let c = Completion {
            content: "hi".into(),
            reasoning: None,
            tool_calls: vec![],
            stop_reason: "stop".into(),
            usage: Usage::default(),
        };
        l.turn(TurnLine {
            kind: "turn",
            run_id: l.run_id().into(),
            turn: 1,
            max_iterations: 20,
            messages: vec![],
            completion: c,
            parsed_code_blocks: 0,
            parsed_final: None,
            reject_reason: None,
            cumulative_tokens: 0,
            cumulative_seconds: 0.0,
        })
        .unwrap();
        l.finish().unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("\"kind\":\"turn\""));
        assert!(s.contains("\"content\":\"hi\""));
    }
}
