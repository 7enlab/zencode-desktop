//! ZenCode Desktop engine: runs the ZenCode CLI agent loop (`zen-agent`) with the CLI's file and
//! shell tools (`zen-tools`) against any OpenAI-compatible endpoint. No agent core is rewritten
//! here; this crate only assembles the pieces.

pub mod events;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use zen_agent::{Agent, AgentConfig, OpenAiClient};
use zen_core::{
    EventSink, HookRunner, NoHooks, NullSink, PermissionChecker, PermissionDecision, RunResult,
    ToolContext, ToolRegistry,
};

/// Options for one headless agent run.
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// OpenAI-compatible base URL (without `/chat/completions`).
    pub base_url: String,
    /// Bearer key; `None` for local gateways / relays that attach it themselves.
    pub api_key: Option<String>,
    pub model: String,
    pub fallback_models: Vec<String>,
    /// Working directory the tools operate in.
    pub cwd: PathBuf,
    pub max_turns: usize,
    pub system_prompt: String,
    /// Explicit opt-in to pre-approve every tool call (Read/Edit/Write/Bash).
    /// Default `false`: all tool calls are denied with a message telling the
    /// caller to opt in. Set to `true` only when the caller has sandboxed `cwd`
    /// (e.g. a tempdir) and accepts the risk.
    pub dangerous_allow_all: bool,
}

impl RunOptions {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: None,
            model: model.into(),
            fallback_models: vec![],
            cwd: cwd.into(),
            max_turns: 12,
            system_prompt: "You are ZenCode, a concise coding agent. Use the tools to complete the task, then reply with a short summary.".into(),
            dangerous_allow_all: false,
        }
    }
}

/// Pre-approves every tool call. Only used when the caller explicitly opts in via
/// [`RunOptions::dangerous_allow_all`]; the caller is responsible for sandboxing `cwd`.
struct AllowAll;
impl PermissionChecker for AllowAll {
    fn check(&self, _: &str, _: &Value, _: bool, _: &ToolContext) -> PermissionDecision {
        PermissionDecision::Allow
    }
}

/// Denies every tool call. This is the default: the caller must explicitly opt in to
/// [`RunOptions::dangerous_allow_all`] to run tools.
struct DenyAll;
impl PermissionChecker for DenyAll {
    fn check(&self, tool_name: &str, _: &Value, _: bool, _: &ToolContext) -> PermissionDecision {
        PermissionDecision::Deny(format!(
            "tool '{tool_name}' denied: RunOptions::dangerous_allow_all is false; \
             set it to true to pre-approve tool calls (only do this in a sandboxed cwd)"
        ))
    }
}

/// Registry with the CLI's Read, Edit, Write and Bash tools.
pub fn default_tools() -> ToolRegistry {
    let mut t = ToolRegistry::new();
    t.register(Arc::new(zen_tools::ReadTool));
    t.register(Arc::new(zen_tools::EditTool));
    t.register(Arc::new(zen_tools::WriteTool));
    t.register(Arc::new(zen_tools::BashTool));
    t
}

/// Run one task to completion and return the agent's result.
pub async fn run_task(opts: &RunOptions, task: &str) -> anyhow::Result<RunResult> {
    run_task_with_sink(opts, task, Arc::new(NullSink)).await
}

/// Like [`run_task`], streaming every agent event to `sink` (e.g. `events::RecordingSink`).
pub async fn run_task_with_sink(
    opts: &RunOptions,
    task: &str,
    sink: Arc<dyn EventSink>,
) -> anyhow::Result<RunResult> {
    if task.trim().is_empty() {
        anyhow::bail!("empty task");
    }
    if !opts.cwd.is_dir() {
        anyhow::bail!("cwd is not a directory: {}", opts.cwd.display());
    }
    if opts.base_url.trim().is_empty() {
        anyhow::bail!("base_url is empty");
    }
    if !opts.base_url.starts_with("http://") && !opts.base_url.starts_with("https://") {
        anyhow::bail!(
            "base_url must start with http:// or https://: {}",
            opts.base_url
        );
    }
    if opts.model.trim().is_empty() {
        anyhow::bail!("model is empty");
    }
    let llm = Arc::new(OpenAiClient::new(
        opts.base_url.clone(),
        opts.api_key.clone(),
        sink.clone(),
    ));
    let config = AgentConfig {
        model: opts.model.clone(),
        fallback_models: opts.fallback_models.clone(),
        system_prompt: opts.system_prompt.clone(),
        max_turns: opts.max_turns,
        auto_compact_threshold: None,
        ..Default::default()
    };
    let hooks: Arc<dyn HookRunner> = Arc::new(NoHooks);
    let ctx = ToolContext::new(opts.cwd.clone(), "engine");
    let checker: Arc<dyn PermissionChecker> = if opts.dangerous_allow_all {
        Arc::new(AllowAll)
    } else {
        Arc::new(DenyAll)
    };
    let mut agent = Agent::new(
        llm,
        default_tools(),
        checker,
        None,
        hooks,
        sink,
        ctx,
        config,
    );
    Ok(agent.run_turn(task).await)
}
