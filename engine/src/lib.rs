//! ZenCode Desktop engine: runs the ZenCode CLI agent loop (`zen-agent`) with the CLI's file and
//! shell tools (`zen-tools`) against any OpenAI-compatible endpoint. No agent core is rewritten
//! here; this crate only assembles the pieces.

pub mod budget;
pub mod checkpoints;
pub mod claims;
pub mod config_import;
pub mod convergence;
pub mod cost_firewall;
pub mod events;
pub mod ipc_bridge;
pub mod release_signing;
pub mod scope;
pub mod session_store;
pub mod smart_router;
pub mod sovereign;
pub mod stream_json;
pub mod telemetry;
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
    /// Explicit opt-in to pre-approve every tool call (Read/Edit/Write/Bash).
    /// Default `false`: all tool calls are denied with a message telling the
    /// caller to opt in. Set to `true` only when the caller has sandboxed `cwd`
    /// (e.g. a tempdir) and accepts the risk.
    pub dangerous_allow_all: bool,
    /// R14 zero-telemetry log. `None` (default) = the engine still arms the
    /// guard and records into an internal log that is dropped with the run;
    /// `Some` = the caller keeps the [`telemetry::NetworkLog`] afterwards
    /// (the desktop Network tab passes one in).
    pub telemetry_log: Option<Arc<telemetry::NetworkLog>>,
    /// R6 convergence guard (0.3-7). `None` (default) = off. `Some(policy)` =
    /// detect repair loops; the first detection forces a re-plan (the next
    /// tool call is refused with the re-plan prompt), a second one stops the
    /// task. See [`convergence::ConvergenceMonitor`].
    pub convergence: Option<convergence::ConvergencePolicy>,
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
            dangerous_allow_all: false,
            telemetry_log: None,
            convergence: None,
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
    // M2 (OCR review): reject malformed options with clear errors up front.
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
    // Structural cost firewall: reject non-free models BEFORE any network
    // call (HTTP client construction, agent loop, fallbacks).
    cost_firewall::FreeOnlyPolicy::default().validate_all(&opts.model, &opts.fallback_models)?;
    // R14 zero-telemetry guard: the ONLY host this run may contact is the
    // user-configured model endpoint. Armed at the same enforcement point as
    // the R1 cost firewall, before any network call is made.
    let telemetry_guard = telemetry::TelemetryGuard::from_base_url(&opts.base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    telemetry::emit_telemetry_ready(&sink, &telemetry_guard);
    let telemetry_log = opts.telemetry_log.clone().unwrap_or_default();
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
        // M1 (OCR review): without a declared scope, tools run only when the
        // caller explicitly opts in; the default denies every tool call.
        None if opts.dangerous_allow_all => Arc::new(AllowAll),
        None => Arc::new(DenyAll),
    };
    // R14: wrap the model backend so every chat call is recorded in the
    // auditable network log. The telemetry wrap is innermost (closest to the
    // HTTP client); the R12 budget wrap stays outside so a budget stop still
    // prevents the call from being logged as sent.
    let llm_inner: Arc<dyn LlmBackend> = Arc::new(telemetry::TelemetryBackend::new(
        Arc::new(OpenAiClient::new(
            opts.base_url.clone(),
            opts.api_key.clone(),
            sink.clone(),
        )),
        telemetry_guard,
        telemetry_log,
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
    // R6 convergence guard (opt-in): observes the agent's events, marks round
    // boundaries at each model call, and gates tool calls. It wraps outermost
    // so a forced re-plan or stop applies before budget and scope checks.
    let (llm, permissions, sink): (
        Arc<dyn LlmBackend>,
        Arc<dyn PermissionChecker>,
        Arc<dyn EventSink>,
    ) = match &opts.convergence {
        Some(policy) => {
            let monitor = convergence::ConvergenceMonitor::new(policy.clone(), 1, sink.clone())?;
            (
                Arc::new(convergence::ConvergenceBackend::new(llm, monitor.clone())),
                Arc::new(convergence::ConvergencePermissions::new(
                    permissions,
                    monitor.clone(),
                )),
                Arc::new(convergence::ConvergenceSink::new(sink, monitor)),
            )
        }
        None => (llm, permissions, sink),
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
