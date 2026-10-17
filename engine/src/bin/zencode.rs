//! `zencode run "<task>"` — headless agent run against a `:free` model channel,
//! saving the session as NDJSON. PLAN step g2.
//!
//! Usage:
//!   zencode run "<task>" [--model M] [--cwd DIR] [--max-turns N] [--session-dir DIR]
//!
//! Env (all optional):
//!   ZENCODE_BASE_URL  OpenAI-compatible base URL (default: Kilo anonymous free tier)
//!   ZENCODE_API_KEY   Bearer <redacted> (default: none — anonymous free tier)
//!   ZENCODE_MODEL     Model id (default: a `:free` model)
//!
//! Only `:free` models are allowed; anything else is refused.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use zencode_engine::events::RecordingSink;
use zencode_engine::{run_task_with_sink, RunOptions};

const DEFAULT_BASE_URL: &str = "https://ai-gateway.kilo.ai/api/v1";
const DEFAULT_MODEL: &str = "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free";

fn usage() -> ! {
    eprintln!(
        "usage: zencode run \"<task>\" [--model M] [--cwd DIR] [--max-turns N] [--session-dir DIR]"
    );
    std::process::exit(2);
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    if cmd != "run" {
        usage();
    }
    let task = args.next().unwrap_or_default();
    if task.is_empty() || task.starts_with("--") {
        usage();
    }

    let mut model = env_or("ZENCODE_MODEL", DEFAULT_MODEL);
    let mut cwd = std::env::current_dir()?;
    let mut max_turns = 12usize;
    let mut session_dir: Option<PathBuf> = None;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().unwrap_or_else(|| usage()),
            "--cwd" => cwd = PathBuf::from(args.next().unwrap_or_else(|| usage())),
            "--max-turns" => {
                max_turns = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--session-dir" => {
                session_dir = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            _ => usage(),
        }
    }

    if !model.ends_with(":free") {
        anyhow::bail!("refusing non-free model '{model}': only `:free` models are allowed");
    }

    let mut opts = RunOptions::new(env_or("ZENCODE_BASE_URL", DEFAULT_BASE_URL), &model, &cwd);
    opts.api_key = std::env::var("ZENCODE_API_KEY").ok();
    opts.max_turns = max_turns;

    let sink = Arc::new(RecordingSink::new());
    let result = run_task_with_sink(&opts, &task, sink.clone()).await?;

    // Save the session as NDJSON.
    let dir = session_dir.unwrap_or_else(|| cwd.join(".zencode").join("sessions"));
    std::fs::create_dir_all(&dir)?;
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let path = dir.join(format!("session-{ts}.jsonl"));
    std::fs::write(&path, sink.to_ndjson())?;

    println!("result: {}", result.result);
    println!("turns: {}", result.num_turns);
    println!(
        "tokens: {} in / {} out",
        result.usage.input_tokens, result.usage.output_tokens
    );
    println!("cost_usd: {}", result.total_cost_usd);
    println!("session: {}", path.display());
    if result.is_error {
        std::process::exit(1);
    }
    Ok(())
}
