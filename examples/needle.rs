//! Phase-2 smoke: drive the loop end-to-end against the real Python worker.
//!
//! What this proves:
//! - SubprocessRepl spawns and roundtrips JSON Lines with worker.py.
//! - worker.py's `commit()` writes `answer['content']`; the orchestrator's
//!   `lookup_var("answer")` sees it.
//! - The RLM loop completes via `FINAL_VAR(answer)`.
//!
//! Run with `python3` on PATH:
//!
//! ```sh
//! cargo run --example needle
//! ```
//!
//! The output should include the line `answer: Nairobi`.

use anyhow::Result;
use reclamo_anl::{
    completion, CapabilitySet, Completion, CompletionOpts, MockProvider, ModelProfile, Provider,
    ThinkingMode, Usage,
};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // Turn 1: commit the answer. Turn 2: finalize via FINAL_VAR. The
    // loop must execute turn 1's code (so the worker writes to
    // `answer['content']`) before resolving turn 2's FINAL.
    let commit = Completion {
        content: "```repl\ncommit(\"Nairobi\")\n```".into(),
        reasoning: None,
        tool_calls: vec![],
        stop_reason: "stop".into(),
        usage: Usage {
            input_tokens: Some(64),
            output_tokens: Some(16),
            total_tokens: Some(80),
        },
    };
    let finalize = Completion {
        content: "FINAL_VAR(answer)".into(),
        reasoning: None,
        tool_calls: vec![],
        stop_reason: "stop".into(),
        usage: Usage {
            input_tokens: Some(8),
            output_tokens: Some(8),
            total_tokens: Some(16),
        },
    };
    let provider: Arc<dyn Provider> = Arc::new(MockProvider::scripted(
        "mock-needle",
        vec![commit, finalize],
    ));
    let mut profile = ModelProfile::new("needle-mock", provider, 16_384);
    profile.thinking = ThinkingMode::Disabled;
    let _caps = CapabilitySet::openai_compat();

    // Use a context bigger than the mock profile's resident_kv (16,384 *
    // 3.5 chars) so the router takes the harness path.
    let context = "The capital of Kenya is Nairobi.\n".repeat(2_000);
    let query = "What is the capital of Kenya?".to_string();

    let opts = CompletionOpts::default();
    let result = completion(context, query, profile, opts).await?;

    println!("\n=== needle smoke ===");
    println!("answer: {}", result.answer);
    println!("mode:   {}", result.mode);
    println!("stop:   {}", result.stop_reason);
    println!("turns:  {}", result.turns);
    println!("tokens: {}", result.tokens);
    println!("seconds: {:.2}", result.seconds);

    assert!(
        result.answer.contains("Nairobi"),
        "expected 'Nairobi' in answer, got {:?}",
        result.answer
    );
    assert!(
        result.mode.starts_with("harness:"),
        "expected harness mode (context too big to route plain), got {:?}",
        result.mode
    );
    assert!(
        matches!(result.stop_reason.as_str(), "final_var"),
        "expected final_var stop, got {:?}",
        result.stop_reason
    );
    println!("OK");
    Ok(())
}
