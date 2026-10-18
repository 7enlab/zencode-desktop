//! ZenCode Desktop engine: runs the ZenCode CLI agent loop (`zen-agent`) with the CLI's file and
//! shell tools (`zen-tools`) against any OpenAI-compatible endpoint. No agent core is rewritten
//! here; this crate only assembles the pieces.

pub mod budget;
pub mod checkpoints;
pub mod claims;
pub mod convergence;
pub mod cost_firewall;
pub mod events;
pub mod ipc_bridge;
pub mod scope;
pub mod session_store;
pub mod timeline;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use zen_agent::{Agent, AgentConfig, OpenAiClient};
use zen_core::{
    EventSink, HookRunner, LlmBackend, NoHooks, NullSink, PermissionChecker, PermissionDecision,
    RunResult, ToolContext, ToolRegistry,
};

use budget::{BudgetEnforcer, BudgetedBackend, BudgetedPermissions, TaskBudget};

use crate::events::RecordingSink;

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
    /// Hard per-task budget (R12). `None` (default) = no budget enforcement;
    /// `Some` wraps the LLM backend and permission checker so the task stops
    /// as soon as any limit is exceeded.
    pub budget: Option<TaskBudget>,
    /// Declared scope (R3). `None` = pre-approved headless run (previous behavior).
    pub scope: Option<scope::ScopeDecl>,
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
            budget: None,
            scope: None,
        }
    }
}

/// Headless run: tools are pre-approved (the caller decides the sandbox, e.g. a tempdir).
struct AllowAll;
impl PermissionChecker for AllowAll {
    fn check(&self, _: &str, _: &Value, _: bool, _: &ToolContext) -> PermissionDecision {
        PermissionDecision::Allow
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

/// Like [`run_task`], recording every agent event into `sink` and appending a
/// [`events::DesktopEvent::SessionEnd`] with cost accounting when the turn finishes.
pub async fn run_task_recorded(
    opts: &RunOptions,
    task: &str,
    sink: Arc<RecordingSink>,
) -> anyhow::Result<RunResult> {
    let result = run_task_with_sink(opts, task, sink.clone()).await?;
    sink.emit_session_end(&result);
    Ok(result)
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
    // Structural cost firewall: reject non-free models BEFORE any network
    // call (HTTP client construction, agent loop, fallbacks).
    cost_firewall::FreeOnlyPolicy::default().validate_all(&opts.model, &opts.fallback_models)?;
    if !opts.cwd.is_dir() {
        anyhow::bail!("cwd is not a directory: {}", opts.cwd.display());
    }
    // R3 scope guard: when a scope is declared, ambiguous declarations are
    // rejected up front and every mutating tool call is checked against it.
    let base_permissions: Arc<dyn PermissionChecker> = match &opts.scope {
        Some(decl) => {
            let reasons = scope::needs_approval(decl, &opts.cwd);
            if !reasons.is_empty() {
                let why: Vec<String> = reasons.iter().map(|r| r.message()).collect();
                anyhow::bail!(
                    "scope declaration needs explicit approval before starting:\n- {}",
                    why.join("\n- ")
                );
            }
            scope::emit_scope_declared(&sink, decl);
            Arc::new(scope::ScopeGuard::new(
                opts.cwd.clone(),
                decl.clone(),
                sink.clone(),
            ))
        }
        None => Arc::new(AllowAll),
    };
    let llm_inner: Arc<dyn LlmBackend> = Arc::new(OpenAiClient::new(
        opts.base_url.clone(),
        opts.api_key.clone(),
        sink.clone(),
    ));
    // R12: when a budget is set, enforce it around the model backend (token +
    // duration hard stop) and the permission checker (per-tool-call hard stop).
    // The budget wraps the R3 scope guard (if any), so both limits apply.
    let (llm, permissions): (Arc<dyn LlmBackend>, Arc<dyn PermissionChecker>) = match &opts.budget {
        Some(budget) => {
            let enforcer = BudgetEnforcer::new(budget.clone(), sink.clone());
            (
                Arc::new(BudgetedBackend::new(llm_inner, enforcer.clone())),
                Arc::new(BudgetedPermissions::new(base_permissions, enforcer)),
            )
        }
        None => (llm_inner, base_permissions),
    };
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
    let mut agent = Agent::new(
        llm,
        default_tools(),
        permissions,
        None,
        hooks,
        sink,
        ctx,
        config,
    );
    Ok(agent.run_turn(task).await)
}

/// Run one task with R4 Verified Claims tracking.
///
/// The agent's assertions ("tests pass", "build succeeds", ...) are registered as
/// claims in the returned [`claims::ClaimRegistry`]; tool results attach heuristic
/// evidence automatically. Callers with authoritative proof (e.g. the verify loop,
/// PR #32) should call [`claims::ClaimRegistry::attach_verify_evidence`].
/// The registry's [`claims::ClaimRegistry::counts`] feed the `SessionEnd` summary.
pub async fn run_task_with_claims(
    opts: &RunOptions,
    task: &str,
    sink: Arc<dyn EventSink>,
) -> anyhow::Result<(RunResult, Arc<claims::ClaimRegistry>)> {
    let registry = Arc::new(claims::ClaimRegistry::new());
    let claiming = Arc::new(claims::ClaimingSink::new(sink, registry.clone()));
    let result = run_task_with_sink(opts, task, claiming).await?;
    Ok((result, registry))
}
