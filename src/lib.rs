//! ReCLamO-Analysis — model-agnostic recursive language model harness.
//!
//! Companion to `CryptoJones/ReCLamO-Harness` (Qwen-tuned v0.1).
//!
//! Public surface:
//!
//! - [`completion`] — top-level entry. Routes by size, then either calls
//!   plain or hands off to the loop.
//! - [`router`] — plain-vs-harness routing.
//! - [`rlm`] — the loop with all five NEXT-STEPS fixes folded in.
//! - [`providers`] — `Provider` trait; `AnthropicProvider`,
//!   `OpenAICompatProvider`, `MockProvider`.
//! - [`repl`] — subprocess REPL wrapper around the Python worker.
//! - [`logger`] — JSONL trajectory.
//! - [`config`] — dataclass config + TOML profile parsing.
//! - [`cli`] — `reclamo-anl {run, ping, version}`.

#![deny(rust_2018_idioms)]
// Documentation is nice-to-have on internal modules — promote specific
// items to `pub` with `///` as they become part of the public surface.
#![allow(missing_docs)]

pub mod cli;
pub mod completion;
pub mod config;
pub mod error;
pub mod logger;
pub mod parsing;
pub mod prompts;
pub mod providers;
pub mod repl;
pub mod rlm;
pub mod router;

pub use completion::{completion, CompletionOpts, RunResult};
pub use config::{LoopConfig, Profile};
pub use error::{ReclamoError, ReclamoResult};
pub use providers::{
    AnthropicProvider, CapabilitySet, Completion, Message, MockProvider, ModelProfile, OpenAICompatProvider,
    Provider, Role, Usage,
};
pub use repl::{ReplClient, ReplExecResult};

/// Library version (matches `Cargo.toml`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
