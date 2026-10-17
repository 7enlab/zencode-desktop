//! g2 smoke: run the engine against a REAL free model (Kilo anonymous tier,
//! no key needed) on a trivial file task. Proves the agent loop works
//! end-to-end outside the fake-localhost tests.
//!
//! Usage: `cargo run --example g2_smoke`
//! Costs: a handful of requests on the anonymous free tier (200 req/hour/IP).

use std::sync::Arc;

use zencode_engine::events::RecordingSink;
use zencode_engine::{run_task_with_sink, RunOptions};

const BASE_URL: &str = "https://ai-gateway.kilo.ai/api/v1";
const MODEL: &str = "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join("zencode-g2-smoke");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;

    let mut opts = RunOptions::new(BASE_URL, MODEL, &dir);
    opts.api_key = None; // anonymous free tier
    opts.max_turns = 8;

    let sink = Arc::new(RecordingSink::new());
    let task = "Create a file named hello.txt in the working directory containing exactly this text on one line: hello zencode";
    println!("task: {task}");

    let result = run_task_with_sink(&opts, task, sink.clone()).await?;
    println!("run result: {result:?}");

    let content = std::fs::read_to_string(dir.join("hello.txt"))?;
    println!("hello.txt: {content:?}");
    assert_eq!(content.trim(), "hello zencode", "file content mismatch");

    let events = sink.events();
    println!("events recorded: {}", events.len());
    println!("G2 SMOKE OK");
    Ok(())
}
