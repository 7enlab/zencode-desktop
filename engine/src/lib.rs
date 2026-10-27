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

#[derive(Clone, Debug)]
pub struct RunOptions {

    pub base_url: String,

    pub api_key: Option<String>,
    pub model: String,
    pub fallback_models: Vec<String>,

    pub cwd: PathBuf,
    pub max_turns: usize,
    pub system_prompt: String,

    pub budget: Option<TaskBudget>,

    pub scope: Option<scope::ScopeDecl>,

    pub dangerous_allow_all: bool,

    pub telemetry_log: Option<Arc<telemetry::NetworkLog>>,

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

struct AllowAll;
impl PermissionChecker for AllowAll {
    fn check(&self, _: &str, _: &Value, _: bool, _: &ToolContext) -> PermissionDecision {
        PermissionDecision::Allow
    }
}

struct DenyAll;
impl PermissionChecker for DenyAll {
    fn check(&self, tool_name: &str, _: &Value, _: bool, _: &ToolContext) -> PermissionDecision {
        PermissionDecision::Deny(format!(
            "tool '{tool_name}' denied: RunOptions::dangerous_allow_all is false; \
             set it to true to pre-approve tool calls (only do this in a sandboxed cwd)"
        ))
    }
}

pub fn default_tools() -> ToolRegistry {
    let mut t = ToolRegistry::new();
    t.register(Arc::new(zen_tools::ReadTool));
    t.register(Arc::new(zen_tools::EditTool));
    t.register(Arc::new(zen_tools::WriteTool));
    t.register(Arc::new(zen_tools::BashTool));
    t
}

pub async fn run_task(opts: &RunOptions, task: &str) -> anyhow::Result<RunResult> {
    run_task_with_sink(opts, task, Arc::new(NullSink)).await
}

pub async fn run_task_recorded(
    opts: &RunOptions,
    task: &str,
    sink: Arc<RecordingSink>,
) -> anyhow::Result<RunResult> {
    let result = run_task_with_sink(opts, task, sink.clone()).await?;
    sink.emit_session_end(&result);
    Ok(result)
}

pub async fn run_task_with_sink(
    opts: &RunOptions,
    task: &str,
    sink: Arc<dyn EventSink>,
) -> anyhow::Result<RunResult> {
    if task.trim().is_empty() {
        anyhow::bail!("empty task");
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

    cost_firewall::FreeOnlyPolicy::default().validate_all(&opts.model, &opts.fallback_models)?;

    let telemetry_guard = telemetry::TelemetryGuard::from_base_url(&opts.base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    telemetry::emit_telemetry_ready(&sink, &telemetry_guard);
    let telemetry_log = opts.telemetry_log.clone().unwrap_or_default();
    if !opts.cwd.is_dir() {
        anyhow::bail!("cwd is not a directory: {}", opts.cwd.display());
    }

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

        None if opts.dangerous_allow_all => Arc::new(AllowAll),
        None => Arc::new(DenyAll),
    };

    let llm_inner: Arc<dyn LlmBackend> = Arc::new(telemetry::TelemetryBackend::new(
        Arc::new(OpenAiClient::new(
            opts.base_url.clone(),
            opts.api_key.clone(),
            sink.clone(),
        )),
        telemetry_guard,
        telemetry_log,
    ));

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
