//! CLI surface. Subcommands: `version`, `ping`, `run`.

use crate::completion::completion;
use crate::completion::{CompletionOpts, RunResult};
use crate::config::Profile;
use crate::error::{ReclamoError, ReclamoResult};
use crate::providers::{AnthropicProvider, MockProvider, OpenAICompatProvider};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Parser)]
#[command(name = "reclamo-anl", version = crate::VERSION, about = "Model-agnostic RLM harness (parallel to CryptoJones/ReCLamO-Harness).")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Print the version.
    Version,
    /// Hit a provider once. Sanity check that credentials + routing work.
    Ping {
        /// Profile TOML file.
        #[arg(long)]
        profile: PathBuf,
        /// Override provider (use `--provider mock` for a no-network check).
        #[arg(long)]
        provider: Option<String>,
    },
    /// Run the harness against a `--query` and `--context`.
    Run {
        /// Profile TOML file (or `--provider mock` for tests).
        #[arg(long)]
        profile: PathBuf,
        /// Context (path to file or `-` for stdin).
        #[arg(long)]
        context: String,
        /// The user query.
        #[arg(short, long)]
        query: String,
        /// Output the trajectory to this directory (defaults to ./runs).
        #[arg(long)]
        log_dir: Option<PathBuf>,
        /// Override max_iterations.
        #[arg(long)]
        max_iterations: Option<u32>,
    },
}

/// Entrypoint for the binary. See `main.rs`.
pub fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        match cli.command {
            Cmd::Version => println!("reclamo-anl {}", crate::VERSION),
            Cmd::Ping { profile, provider } => {
                cmd_ping(profile, provider).await?;
            }
            Cmd::Run { profile, context, query, log_dir, max_iterations } => {
                let opts = CompletionOpts {
                    max_iterations,
                    log_dir,
                    ..Default::default()
                };
                cmd_run(profile, context, query, opts).await?;
            }
        }
        Ok::<(), anyhow::Error>(())
    })
}

async fn cmd_ping(profile_path: PathBuf, provider_override: Option<String>) -> anyhow::Result<()> {
    let profile = Profile::from_path(&profile_path)
        .map_err(|e| anyhow::anyhow!("load profile: {e}"))?;
    let provider = build_provider(&profile, provider_override.as_deref()).await?;
    let caps = provider.capabilities();
    let model_id = provider.model_id();
    let reply = provider
        .complete(
            &[crate::providers::Message::system("Reply with one short sentence: pong."),
              crate::providers::Message::user("ping")],
            None,
            crate::providers::CompleteOpts::default(),
        )
        .await?;
    println!("provider: {} ({})", provider.name(), provider.model_id());
    println!("capabilities: thinking={}, tool_use={}, json_mode={}, prefix_cache={}, format={:?}",
        caps.supports_thinking, caps.supports_tool_use, caps.supports_json_mode,
        caps.supports_prefix_cache, caps.known_reasoning_format);
    println!("reply: {}", reply.content.trim());
    println!("stop: {}, tokens: {}", reply.stop_reason, reply.usage.total());
    let _ = (caps, model_id);
    Ok(())
}

async fn cmd_run(
    profile_path: PathBuf,
    context: String,
    query: String,
    opts: CompletionOpts,
) -> anyhow::Result<()> {
    let profile = Profile::from_path(&profile_path)
        .map_err(|e| anyhow::anyhow!("load profile: {e}"))?;
    let provider = build_provider(&profile, None).await?;
    let model_profile = profile.model_profile(provider);
    let context_text = read_context(&context).await?;
    let result = completion(context_text, query, model_profile, opts).await?;
    print_run_result(&result);
    Ok(())
}

fn print_run_result(r: &RunResult) {
    println!("\n--- answer ---\n{}\n--- meta ---\nmode: {}\nstop: {}\nturns: {}\nsub-calls: {}\ntokens: {}\nseconds: {:.2}\n",
        r.answer, r.mode, r.stop_reason, r.turns, r.subcalls, r.tokens, r.seconds);
}

async fn read_context(spec: &str) -> anyhow::Result<String> {
    if spec == "-" {
        let mut s = String::new();
        use std::io::Read;
        std::io::stdin().read_to_string(&mut s)?;
        return Ok(s);
    }
    Ok(tokio::fs::read_to_string(spec).await?)
}

/// Build the provider. `provider_override` (e.g. "mock") is used in tests
/// to skip network.
async fn build_provider(
    profile: &Profile,
    provider_override: Option<&str>,
) -> anyhow::Result<Arc<dyn crate::providers::Provider>> {
    let provider_name = provider_override.unwrap_or(&profile.provider);
    let key = match provider_name {
        "mock" => String::new(),
        _ => profile
            .resolve_api_key()
            .map_err(|e| anyhow::anyhow!("resolve api key: {e}"))?,
    };
    match provider_name {
        "anthropic" => {
            let base = profile
                .base_url
                .clone()
                .unwrap_or_else(|| crate::providers::anthropic::ANTHROPIC_API_URL.to_string());
            let p = AnthropicProvider::with_base(base, &profile.model, &key)
                .map_err(|e| anyhow::anyhow!("anthropic provider: {e}"))?;
            Ok(Arc::new(p))
        }
        "openai-compat" => {
            let base = profile
                .base_url
                .clone()
                .ok_or_else(|| anyhow::anyhow!("openai-compat requires --base-url"))?;
            let p = OpenAICompatProvider::new(base, &profile.model, &key)
                .map_err(|e| anyhow::anyhow!("openai-compat provider: {e}"))?;
            Ok(Arc::new(p))
        }
        "mock" => Ok(Arc::new(MockProvider::scripted(
            profile.model.clone(),
            vec![crate::providers::Completion {
                content: "(mock reply: pass)".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: crate::providers::Usage::default(),
            }],
        ))),
        other => Err(anyhow::anyhow!("unknown provider: {other}")),
    }
}

#[allow(dead_code)]
fn _force_use_reclamo_result() -> ReclamoResult<()> {
    Err::<(), _>(ReclamoError::Config("see cli.rs".into()))
}
