//! Typed errors. Every limit trigger (`max_iterations`, `max_timeout`,
//! `max_tokens`, `max_errors`) returns `ReclamoError::LimitHit { ... }` with a
//! `partial_answer` so the forced-finish path can recover (or report).

use thiserror::Error;

/// Top-level result alias.
pub type ReclamoResult<T> = std::result::Result<T, ReclamoError>;

/// All error variants the harness surfaces. Loops use `LimitHit` uniformly.
#[derive(Debug, Error)]
pub enum ReclamoError {
    /// One of the four limits fired. `partial_answer` may be present.
    #[error("limit hit ({kind}): {message}")]
    LimitHit {
        /// Which limit fired: `iterations`, `timeout`, `tokens`, `errors`.
        kind: &'static str,
        /// Human-readable detail.
        message: String,
        /// Best answer recovered from the REPL before the limit fired.
        partial_answer: Option<String>,
    },

    /// Provider returned an error or non-2xx HTTP.
    #[error("provider {provider} failed: {message}")]
    Provider {
        /// Provider name (e.g. `anthropic`, `openai-compat`).
        provider: String,
        /// Human-readable detail.
        message: String,
    },

    /// REPL subprocess died or returned malformed JSON.
    #[error("repl error: {0}")]
    Repl(String),

    /// Context could not be loaded (file missing, encoding, etc.).
    #[error("context error: {0}")]
    Context(String),

    /// Configuration is invalid (missing required key, profile not found, ...).
    #[error("config error: {0}")]
    Config(String),

    /// Profile specified a model/provider combo that did not match any registered
    /// adapter.
    #[error("profile not found: {0}")]
    ProfileNotFound(String),

    /// Filesystem / I/O error (logger, profile loader, ...).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl ReclamoError {
    /// Convenience for limit triggered from the loop.
    pub fn limit(kind: &'static str, message: impl Into<String>) -> Self {
        Self::LimitHit { kind, message: message.into(), partial_answer: None }
    }

    /// Convenience for limit triggered from the loop with a partial answer.
    pub fn limit_with(kind: &'static str, message: impl Into<String>, partial: impl Into<String>) -> Self {
        Self::LimitHit {
            kind,
            message: message.into(),
            partial_answer: Some(partial.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_carries_partial_answer() {
        let e = ReclamoError::limit_with("iterations", "max turns", "best so far");
        match e {
            ReclamoError::LimitHit { partial_answer: Some(p), .. } => assert_eq!(p, "best so far"),
            _ => panic!("expected LimitHit with partial_answer"),
        }
    }

    #[test]
    fn limit_without_partial_answer() {
        let e = ReclamoError::limit("timeout", "deadline");
        match e {
            ReclamoError::LimitHit { partial_answer: None, .. } => {}
            _ => panic!("expected LimitHit without partial_answer"),
        }
    }
}
