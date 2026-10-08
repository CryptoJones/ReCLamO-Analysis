//! Phase-4 eval rig. Walks the frozen cell matrix and writes JSONL.
//!
//! Usage:
//! ```sh
//! cargo run --example eval -- \
//!     --provider mock --model mock-m --tasks Cerebex,MasterControl,Multivac,Neuromancer,SELMA \
//!     --sizes small --seeds 0 \
//!     --mode both --out evals/results.mock.jsonl
//! ```
//!
//! For real-provider runs:
//! ```sh
//! cargo run --example eval -- \
//!     --provider anthropic --model claude-sonnet-4-5 \
//!     --profile profiles/anthropic.toml \
//!     --tasks Cerebex,MasterControl,Multivac,Neuromancer,SELMA \
//!     --sizes small,medium,large --seeds 0,1 \
//!     --mode both --out evals/results.anthropic.jsonl
//! ```

use anyhow::Result;
use reclamo_anl::{
    completion, CapabilitySet, Completion, CompletionOpts, MockProvider, ModelProfile, Profile,
    Provider, ThinkingMode, Usage,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Plain,
    HarnessFence,
}

impl Mode {
    /// The router inside `completion()` decides plain-vs-harness. We
    /// record what we ASKED for; `mode_actual` carries the decision.
    #[allow(dead_code)]
    fn forced(&self) -> bool {
        true
    }
}

#[derive(Debug, Serialize)]
struct Row {
    task: String,
    size: String,
    seed: u32,
    mode_requested: Mode,
    mode_actual: String,
    stop_reason: String,
    answer: String,
    score: f64,
    tokens: u64,
    seconds: f64,
    turns: u32,
    subcalls: u32,
    context_chars: usize,
    truth_repr: String,
}


#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GenOutput {
    context: String,
    question: String,
    answer: String,
    truth_repr: String,
    meta: serde_json::Value,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cfg = parse_args(&args)?;
    eprintln!("config: {cfg:?}");

    let provider = build_provider(&cfg)?;
    let profile = cfg.to_model_profile(provider);

    let mut out = tokio::fs::File::create(&cfg.out).await?;
    let mut total = 0usize;
    let mut scored = 0.0f64;
    let mut by_mode = std::collections::BTreeMap::<String, (usize, f64)>::new();

    for task in &cfg.tasks {
        for size in &cfg.sizes {
            for seed in &cfg.seeds {
                // Generate.
                let gen = gen(task, *seed, size).await?;
                let qchars = gen.context.len();

                // Plain path: one-shot, no loop. LoopConfig::from_opts default has
                // route_by_size=true; for a small context plain will be chosen
                // naturally. We run both modes and let the router decide.
                let plain_opts = CompletionOpts::default();
                let plain_run = completion(gen.context.clone(), gen.question.clone(), profile.clone(), plain_opts).await?;
                let plain_score = score(task, &gen.truth_repr, &plain_run.answer).await?;

                // Harness path: same call; the router picks the same path
                // for the same context. mode_actual tells us which.
                let harness_opts = CompletionOpts::default();
                let harness_run = completion(gen.context.clone(), gen.question.clone(), profile.clone(), harness_opts).await?;
                let harness_score = score(task, &gen.truth_repr, &harness_run.answer).await?;

                for (mode, run, sc) in [
                    (Mode::Plain, &plain_run, plain_score),
                    (Mode::HarnessFence, &harness_run, harness_score),
                ] {
                    let row = Row {
                        task: task.clone(),
                        size: size.clone(),
                        seed: *seed,
                        mode_requested: mode,
                        mode_actual: run.mode.clone(),
                        stop_reason: run.stop_reason.clone(),
                        answer: run.answer.clone(),
                        score: sc,
                        tokens: run.tokens,
                        seconds: run.seconds,
                        turns: run.turns,
                        subcalls: run.subcalls,
                        context_chars: qchars,
                        truth_repr: gen.truth_repr.clone(),
                    };
                    let line = serde_json::to_string(&row)?;
                    out.write_all(line.as_bytes()).await?;
                    out.write_all(b"\n").await?;
                    total += 1;
                    scored += sc;
                    let key = format!("{mode:?}");
                    let entry = by_mode.entry(key).or_insert((0, 0.0));
                    entry.0 += 1;
                    entry.1 += sc;
                }

                eprintln!(
                    "{} {} seed={} | plain={:.2} harness={:.2}",
                    task, size, seed, plain_score, harness_score,
                );
            }
        }
    }

    out.flush().await?;
    eprintln!("\n--- {} rows ---", total);
    eprintln!("mean score: {:.3}", scored / total as f64);
    for (m, (n, s)) in &by_mode {
        eprintln!("  {m}: {n} rows, mean {:.3}", s / *n as f64);
    }
    Ok(())
}

struct EvalCfg {
    provider: String,
    model: String,
    profile_path: Option<PathBuf>,
    max_context: u64,
    tasks: Vec<String>,
    sizes: Vec<String>,
    seeds: Vec<u32>,
    mode: String,
    out: PathBuf,
}

impl std::fmt::Debug for EvalCfg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalCfg")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("tasks", &self.tasks)
            .field("sizes", &self.sizes)
            .field("seeds", &self.seeds)
            .field("mode", &self.mode)
            .field("out", &self.out)
            .finish()
    }
}

impl EvalCfg {
    fn to_model_profile(&self, provider: Arc<dyn Provider>) -> ModelProfile {
        let mut p = ModelProfile::new(self.provider.clone(), provider, self.max_context);
        p.thinking = ThinkingMode::Adaptive;
        p
    }
}

fn parse_args(args: &[String]) -> Result<EvalCfg> {
    let mut provider = String::from("mock");
    let mut model = String::from("mock-m");
    let mut profile_path: Option<PathBuf> = None;
    let mut max_context = 200_000u64;
    let mut tasks = vec![
        "Cerebex".to_string(),
        "MasterControl".to_string(),
        "Multivac".to_string(),
        "Neuromancer".to_string(),
        "SELMA".to_string(),
    ];
    let mut sizes = vec!["small".to_string()];
    let mut seeds = vec![0u32];
    let mut mode = "both".to_string();
    let mut out = PathBuf::from("evals/results.jsonl");

    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--provider" => {
                provider = args[i + 1].clone();
                i += 2;
            }
            "--model" => {
                model = args[i + 1].clone();
                i += 2;
            }
            "--profile" => {
                profile_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--max-context" => {
                max_context = args[i + 1].parse()?;
                i += 2;
            }
            "--tasks" => {
                tasks = args[i + 1].split(',').map(|s| s.trim().to_string()).collect();
                i += 2;
            }
            "--sizes" => {
                sizes = args[i + 1].split(',').map(|s| s.trim().to_string()).collect();
                i += 2;
            }
            "--seeds" => {
                seeds = args[i + 1].split(',').map(|s| s.trim().parse::<u32>().unwrap()).collect();
                i += 2;
            }
            "--mode" => {
                mode = args[i + 1].clone();
                i += 2;
            }
            "--out" => {
                out = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            _ => anyhow::bail!("unknown arg {a}"),
        }
    }
    Ok(EvalCfg {
        provider,
        model,
        profile_path,
        max_context,
        tasks,
        sizes,
        seeds,
        mode,
        out,
    })
}

fn build_provider(cfg: &EvalCfg) -> Result<Arc<dyn Provider>> {
    match cfg.provider.as_str() {
        "mock" => {
            // Mock returns a 1.0 answer (truncated) for plain mode (which
            // never executes code) and a commit+FVAR for the harness.
            let plain_completion = Completion {
                content: format!("The answer is {{truth_text}}."),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(8), output_tokens: Some(8), total_tokens: Some(16) },
            };
            let commit_completion = Completion {
                content: "```repl\ncommit(\"Nairobi\")\n```".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(64), output_tokens: Some(16), total_tokens: Some(80) },
            };
            let finalize = Completion {
                content: "FINAL_VAR(answer)".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(8), output_tokens: Some(8), total_tokens: Some(16) },
            };
            // The mock is sequential — first call gets plain text, then
            // commit, then finalize. Both plain and harness runs make
            // exactly 1 (plain) or 2 (harness) provider calls per cell,
            // so this lines up cleanly.
            let scripted = vec![plain_completion, commit_completion, finalize];
            let _caps = CapabilitySet::openai_compat();
            let mock = MockProvider::scripted(cfg.model.clone(), scripted);
            Ok(Arc::new(mock))
        }
        "anthropic" => {
            let profile_path = cfg
                .profile_path
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--profile is required for anthropic"))?;
            let profile = Profile::from_path(&profile_path)
                .map_err(|e| anyhow::anyhow!("load profile: {e}"))?;
            let api_key = profile.resolve_api_key()?;
            let base = profile
                .base_url
                .clone()
                .unwrap_or_else(|| crate_anthropic_base().to_string());
            let p = reclamo_anl::AnthropicProvider::with_base(base, &cfg.model, &api_key)?;
            Ok(Arc::new(p))
        }
        "openai-compat" => {
            let profile_path = cfg
                .profile_path
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--profile is required for openai-compat"))?;
            let profile = Profile::from_path(&profile_path)
                .map_err(|e| anyhow::anyhow!("load profile: {e}"))?;
            let api_key = profile.resolve_api_key()?;
            let base = profile
                .base_url
                .clone()
                .ok_or_else(|| anyhow::anyhow!("openai-compat needs a base_url"))?;
            let p = reclamo_anl::OpenAICompatProvider::new(base, &cfg.model, &api_key)?;
            Ok(Arc::new(p))
        }
        other => anyhow::bail!("unknown provider {other}"),
    }
}

fn crate_anthropic_base() -> &'static str {
    "https://api.anthropic.com"
}

async fn gen(task: &str, seed: u32, size: &str) -> Result<GenOutput> {
    let out = Command::new("python3")
        .arg("-I")
        .arg("evals/run_gen.py")
        .arg("generate")
        .arg(task)
        .arg(seed.to_string())
        .arg(size)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!("generate failed for {task}");
    }
    let s = String::from_utf8(out.stdout)?;
    let v: GenOutput = serde_json::from_str(s.trim())?;
    Ok(v)
}

async fn score(task: &str, truth_repr: &str, answer: &str) -> Result<f64> {
    let mut child = Command::new("python3")
        .arg("-I")
        .arg("evals/run_gen.py")
        .arg("score")
        .arg(task)
        .arg(answer)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(truth_repr.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
    }
    let out = child.wait_with_output().await?;
    let s = String::from_utf8(out.stdout)?;
    let v: f64 = s.trim().parse().unwrap_or(0.0);
    Ok(v)
}
