//! `MockProvider` — deterministic scripted responses for tests.
//!
//! Tests register a list of scripted completions; each call to `complete`
//! pops the next one. When the list is exhausted, the mock returns
//! `Provider("mock: exhausted")`.
//!
//! Tests can also register a function-based responder for state-machine
//! scenarios (e.g. "first call says `print(x)`, second says `FINAL_VAR(x)`").

use super::base::{CapabilitySet, CompleteOpts, Completion, Message, Provider, Role};
use crate::error::{ReclamoError, ReclamoResult};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Boxed scripted completion.
pub type Scripted = std::sync::Arc<dyn Fn(&[Message]) -> Completion + Send + Sync>;

/// Boxed async scripted completion (used to model slow/hung providers
/// for the `max_timeout` tests — a sync closure can't `await`).
pub type AsyncScripted =
    std::sync::Arc<dyn Fn(&[Message]) -> futures::future::BoxFuture<'static, Completion> + Send + Sync>;

enum Script {
    Sequential(Vec<Completion>),
    Function(Scripted),
    AsyncFunction(AsyncScripted),
}

pub struct MockProvider {
    model: String,
    inner: Mutex<Script>,
    caps: CapabilitySet,
}

impl MockProvider {
    /// Build a mock that returns the given completions in order.
    pub fn scripted(model: impl Into<String>, completions: Vec<Completion>) -> Self {
        Self {
            model: model.into(),
            inner: Mutex::new(Script::Sequential(completions)),
            caps: CapabilitySet::openai_compat(),
        }
    }

    /// Build a mock that delegates each call to a function (for stateful
    /// tests).
    pub fn function(model: impl Into<String>, f: Scripted) -> Self {
        Self {
            model: model.into(),
            inner: Mutex::new(Script::Function(f)),
            caps: CapabilitySet::openai_compat(),
        }
    }

    /// Build a mock that delegates each call to an async function.
    /// Needed for tests that simulate a slow/hung provider so the
    /// `max_timeout` deadline can actually trip the root call (#7).
    /// The sync `function` builder can't `await`, so the timeout
    /// tests use this.
    pub fn function_async(model: impl Into<String>, f: AsyncScripted) -> Self {
        Self {
            model: model.into(),
            inner: Mutex::new(Script::AsyncFunction(f)),
            caps: CapabilitySet::openai_compat(),
        }
    }

    /// Build a mock that returns the same completion forever (e.g. a
    /// canned `FINAL_VAR(answer)` for loop smoke tests).
    pub fn looped(model: impl Into<String>, completion: Completion) -> Self {
        Self::function(model, {
            let c = completion.clone();
            Arc::new(move |_| c.clone())
        })
    }

    /// Override the capability set the mock claims to support.
    pub fn with_capabilities(mut self, caps: CapabilitySet) -> Self {
        self.caps = caps;
        self
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn capabilities(&self) -> CapabilitySet {
        self.caps
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    async fn complete(
        &self,
        messages: &[Message],
        _tools: Option<&[serde_json::Value]>,
        _opts: CompleteOpts,
    ) -> ReclamoResult<Completion> {
        // Take the script choice out of the mutex before any await.
        // `MutexGuard` is `!Send` and `complete` is async; holding the
        // guard across an `await` (e.g. the AsyncFunction branch)
        // would fail to compile.
        enum Choice {
            Seq(Completion),
            Fn(Scripted),
            AsyncFn(AsyncScripted),
            Exhausted,
        }
        let choice = {
            let mut g = self.inner.lock().map_err(|e| ReclamoError::Provider {
                provider: "mock".into(),
                message: format!("mutex poisoned: {e}"),
            })?;
            match &mut *g {
                Script::Sequential(v) => {
                    if v.is_empty() {
                        Choice::Exhausted
                    } else {
                        Choice::Seq(v.remove(0))
                    }
                }
                Script::Function(f) => Choice::Fn(f.clone()),
                Script::AsyncFunction(f) => Choice::AsyncFn(f.clone()),
            }
        };
        match choice {
            Choice::Seq(c) => Ok(c),
            Choice::Fn(f) => Ok(f(messages)),
            Choice::AsyncFn(f) => Ok(f(messages).await),
            Choice::Exhausted => Err(ReclamoError::Provider {
                provider: "mock".into(),
                message: "exhausted".into(),
            }),
        }
    }
}

/// Convenience: extract the last user-message text from `messages`, dropping
/// any system messages. Useful for assertions on what the loop sent.
pub fn last_user_text(messages: &[Message]) -> Option<&str> {
    messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .map(|m| m.content.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::Usage;

    #[tokio::test]
    async fn scripted_returns_in_order() {
        let mock = MockProvider::scripted(
            "mock-1",
            vec![
                Completion { content: "first".into(), reasoning: None, tool_calls: vec![], stop_reason: "stop".into(), usage: Usage::default() },
                Completion { content: "second".into(), reasoning: None, tool_calls: vec![], stop_reason: "stop".into(), usage: Usage::default() },
            ],
        );
        let r1 = mock.complete(&[], None, CompleteOpts::default()).await.unwrap();
        let r2 = mock.complete(&[], None, CompleteOpts::default()).await.unwrap();
        assert_eq!(r1.content, "first");
        assert_eq!(r2.content, "second");
    }

    #[tokio::test]
    async fn exhaustion_is_error() {
        let mock = MockProvider::scripted("m", vec![]);
        let err = mock.complete(&[], None, CompleteOpts::default()).await.unwrap_err();
        assert!(matches!(err, ReclamoError::Provider { .. }));
    }

    #[test]
    fn last_user_text_finds_user() {
        let msgs = vec![
            Message::system("sys"),
            Message::user("first user"),
            Message::assistant("ans"),
            Message::user("second user"),
        ];
        assert_eq!(last_user_text(&msgs), Some("second user"));
    }
}
