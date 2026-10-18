//! R10 Smart Model Router: pick the best free model for a task, with fallback.
//!
//! ZenCode Desktop is free-only by default. The router turns that product
//! promise into *routing* behavior:
//!
//! - [`TaskClassifier`] maps a task description to a [`TaskKind`]
//!   (code, chat, reasoning, vision).
//! - [`ModelRegistry`] holds the curated list of *verified* free models
//!   (every entry is either `:free`-suffixed or on the cost firewall
//!   allowlist — the same rule [`cost_firewall`] enforces structurally).
//! - [`HealthTracker`] marks models dead after consecutive failures so the
//!   router stops sending traffic to them. The `freetoken-bots-daily-scan`
//!   cron refreshes liveness out of band; this tracker handles the
//!   in-session view.
//! - [`SmartRouter::route`] returns a [`RoutePlan`] (primary + ordered
//!   fallbacks) for one task. Every candidate is validated by
//!   [`cost_firewall::FreeOnlyPolicy`] *before* it can be used — a paid
//!   model can only appear when [`CostMode::BringYourOwnKey`] is active,
//!   and even then only ids the user explicitly configured.
//!
//! The router never performs network I/O itself: classification, registry
//! lookup, health bookkeeping and free-only validation are all pure. The
//! caller walks the returned plan and reports success/failure back via
//! [`SmartRouter::record_success`] / [`SmartRouter::record_failure`].

use std::collections::HashMap;
use std::fmt;

use crate::cost_firewall::{self, CostFirewallError, FreeOnlyPolicy};

/// How the user pays for inference.
///
/// [`CostMode::FreeOnly`] (the default) means the router may only emit
/// provably-free model ids. [`CostMode::BringYourOwnKey`] means the user
/// attached their own provider key and explicitly opted into paid models;
/// ZenCode still never bills — the user's provider does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CostMode {
    /// Only `:free` / allowlisted models. Structural, not a setting.
    #[default]
    FreeOnly,
    /// User-supplied key; user-chosen model ids allowed (user pays provider).
    BringYourOwnKey,
}

/// What kind of work a task needs. Drives model selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    /// Writing, editing, reviewing code.
    Code,
    /// Open-ended conversation, Q&A, summarization.
    Chat,
    /// Multi-step problem solving, planning, math, deep analysis.
    Reasoning,
    /// Image input / visual understanding.
    Vision,
}

impl fmt::Display for TaskKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskKind::Code => write!(f, "code"),
            TaskKind::Chat => write!(f, "chat"),
            TaskKind::Reasoning => write!(f, "reasoning"),
            TaskKind::Vision => write!(f, "vision"),
        }
    }
}

/// Heuristic task classifier. Keyword-based, deterministic, no I/O.
///
/// Vision wins over everything (image input is the hardest constraint),
/// then reasoning, then code, defaulting to chat.
pub struct TaskClassifier;

impl TaskClassifier {
    /// Classify a task description. Never fails; unknown tasks are [`TaskKind::Chat`].
    pub fn classify(task: &str) -> TaskKind {
        let t = task.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| t.contains(w));

        if has(&[
            "image",
            "screenshot",
            "picture",
            "photo",
            "diagram",
            "visual",
            "ocr",
            "describe this image",
            "what do you see",
        ]) {
            return TaskKind::Vision;
        }
        if has(&[
            "prove",
            "proof",
            "theorem",
            "algorithm complexity",
            "design a system",
            "architecture",
            "trade-off",
            "tradeoff",
            "step by step",
            "multi-step",
            "plan",
            "strategy",
            "analyze deeply",
            "root cause",
        ]) {
            return TaskKind::Reasoning;
        }
        if has(&[
            "code",
            "function",
            "bug",
            "fix",
            "refactor",
            "compile",
            "test",
            "cargo",
            "rust",
            "python",
            "javascript",
            "typescript",
            ".rs",
            ".py",
            ".js",
            ".ts",
            "pull request",
            "diff",
            "implement",
        ]) {
            return TaskKind::Code;
        }
        TaskKind::Chat
    }
}

/// One routable model: id + where to reach it + what it is good at.
///
/// `priority` is ascending: lower = preferred. Entries with equal priority
/// keep registry order (stable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEntry {
    /// Full model id as sent to the OpenAI-compatible endpoint.
    pub id: String,
    /// Human-readable provider name (for logs / UI).
    pub provider: String,
    /// Base URL of the OpenAI-compatible endpoint (without `/chat/completions`).
    pub base_url: String,
    /// Task kinds this model is known good at.
    pub kinds: Vec<TaskKind>,
    /// Lower = preferred.
    pub priority: u32,
}

impl ModelEntry {
    pub fn new(
        id: impl Into<String>,
        provider: impl Into<String>,
        base_url: impl Into<String>,
        kinds: Vec<TaskKind>,
        priority: u32,
    ) -> Self {
        Self {
            id: id.into(),
            provider: provider.into(),
            base_url: base_url.into(),
            kinds,
            priority,
        }
    }

    /// Does this entry claim to handle `kind`?
    pub fn handles(&self, kind: TaskKind) -> bool {
        self.kinds.contains(&kind)
    }
}

/// Curated registry of verified free models.
///
/// SOURCES (verified 2026-10-16/17, see memory):
/// - TokenRouter: `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free`
///   (verified :free id from live /v1/models)
/// - TokenHarbor: `deepseek-v4.1-flash:free`, `deepseek-v4-flash:free`,
///   `mimo-v2.6-flash:free`, `mimo-v2.5:free` (verified :free ids)
/// - OpenRouter: `nvidia/nemotron-3-ultra-550b-a55b:free` (Pi default)
/// - Experiential Labs: `deepseek-v4.1-flash-uncensored`
///   (verified via chat call, usage.cost = 0.0 — on the firewall allowlist)
/// - Kilo gateway: `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free`
///   (anonymous keyless, 200 req/hour/IP)
///
/// Every id here MUST satisfy [`cost_firewall::is_free_model`]; the
/// registry constructor debug-asserts this so a bad entry fails fast in
/// tests instead of silently routing paid traffic.
#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    models: Vec<ModelEntry>,
}

impl ModelRegistry {
    /// The built-in verified-free registry.
    pub fn verified_free() -> Self {
        let all_kinds = vec![TaskKind::Code, TaskKind::Chat, TaskKind::Reasoning];
        let code_chat = vec![TaskKind::Code, TaskKind::Chat];
        let models = vec![
            // Reasoning-capable first: best default for agent work.
            ModelEntry::new(
                "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
                "tokenrouter",
                "https://api.tokenrouter.com/v1",
                all_kinds.clone(),
                10,
            ),
            ModelEntry::new(
                "deepseek-v4.1-flash:free",
                "tokenharbor",
                "https://tokenharbor.ai/v1",
                all_kinds.clone(),
                20,
            ),
            ModelEntry::new(
                "nvidia/nemotron-3-ultra-550b-a55b:free",
                "openrouter",
                "https://openrouter.ai/api/v1",
                all_kinds.clone(),
                30,
            ),
            ModelEntry::new(
                "deepseek-v4-flash:free",
                "tokenharbor",
                "https://tokenharbor.ai/v1",
                all_kinds.clone(),
                40,
            ),
            ModelEntry::new(
                "mimo-v2.6-flash:free",
                "tokenharbor",
                "https://tokenharbor.ai/v1",
                code_chat.clone(),
                50,
            ),
            ModelEntry::new(
                "mimo-v2.5:free",
                "tokenharbor",
                "https://tokenharbor.ai/v1",
                code_chat.clone(),
                60,
            ),
            // Allowlisted (no :free suffix, verified $0.00 by actual call).
            ModelEntry::new(
                "deepseek-v4.1-flash-uncensored",
                "experientiallabs",
                "https://api.experientiallabs.ai/v1",
                all_kinds.clone(),
                70,
            ),
            // Anonymous keyless fallback (no key needed at all).
            ModelEntry::new(
                "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
                "kilo",
                "https://ai-gateway.kilo.ai/api/v1",
                all_kinds,
                80,
            ),
        ];
        let registry = Self { models };
        debug_assert!(registry.all_free(), "registry contains a non-free model id");
        registry
    }

    /// Build a registry from explicit entries (e.g. user-configured BYOK models).
    pub fn custom(models: Vec<ModelEntry>) -> Self {
        Self { models }
    }

    fn all_free(&self) -> bool {
        self.models
            .iter()
            .all(|m| cost_firewall::is_free_model(&m.id))
    }

    /// All entries, in registry order.
    pub fn models(&self) -> &[ModelEntry] {
        &self.models
    }

    /// Entries handling `kind`, sorted by ascending priority (stable).
    pub fn for_kind(&self, kind: TaskKind) -> Vec<&ModelEntry> {
        let mut out: Vec<&ModelEntry> = self.models.iter().filter(|m| m.handles(kind)).collect();
        out.sort_by_key(|m| m.priority);
        out
    }
}

/// Liveness of one model as seen by this router instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthState {
    /// Working (or never tried).
    #[default]
    Healthy,
    /// Some recent failures, still worth trying after healthier options.
    Degraded,
    /// Consecutively failed too often — skipped until it recovers.
    Dead,
}

/// In-session health bookkeeping. Pure: no timers, no I/O.
///
/// A model becomes [`HealthState::Degraded`] after 1 failure and
/// [`HealthState::Dead`] after [`HealthTracker::DEAD_AFTER`] consecutive
/// failures. Any success resets the counter to [`HealthState::Healthy`].
#[derive(Debug, Clone, Default)]
pub struct HealthTracker {
    failures: HashMap<String, u32>,
}

impl HealthTracker {
    /// Consecutive failures before a model is considered dead.
    pub const DEAD_AFTER: u32 = 3;

    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful call: the model is healthy again.
    pub fn record_success(&mut self, model_id: &str) {
        self.failures.remove(model_id);
    }

    /// Record a failed call (429 / 5xx / timeout / network error).
    /// Returns the new [`HealthState`].
    pub fn record_failure(&mut self, model_id: &str) -> HealthState {
        let n = self.failures.entry(model_id.to_string()).or_insert(0);
        *n = n.saturating_add(1);
        self.state(model_id)
    }

    /// Current state without mutating.
    pub fn state(&self, model_id: &str) -> HealthState {
        match self.failures.get(model_id).copied().unwrap_or(0) {
            0 => HealthState::Healthy,
            n if n >= Self::DEAD_AFTER => HealthState::Dead,
            _ => HealthState::Degraded,
        }
    }

    /// Forget everything (e.g. after the daily liveness scan refreshes).
    pub fn reset(&mut self) {
        self.failures.clear();
    }
}

/// What the router decided for one task: try `primary` first, then each of
/// `fallbacks` in order. All entries are validated for the active
/// [`CostMode`] — see [`SmartRouter::route`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePlan {
    pub kind: TaskKind,
    pub primary: ModelEntry,
    pub fallbacks: Vec<ModelEntry>,
}

impl RoutePlan {
    /// Primary + fallbacks as one ordered attempt list.
    pub fn attempt_order(&self) -> Vec<&ModelEntry> {
        std::iter::once(&self.primary)
            .chain(self.fallbacks.iter())
            .collect()
    }

    /// All model ids in attempt order.
    pub fn model_ids(&self) -> Vec<&str> {
        self.attempt_order().iter().map(|m| m.id.as_str()).collect()
    }
}

/// Router errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterError {
    /// No candidate survived health filtering + cost-mode validation.
    NoModelAvailable { kind: TaskKind, mode: CostMode },
    /// A candidate failed free-only validation (only possible in
    /// [`CostMode::FreeOnly`]; the offending id is reported).
    PaidModelBlocked {
        model: String,
        source: CostFirewallError,
    },
}

impl fmt::Display for RouterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouterError::NoModelAvailable { kind, mode } => write!(
                f,
                "smart router: no {kind} model available in {mode:?} mode"
            ),
            RouterError::PaidModelBlocked { model, source } => {
                write!(f, "smart router blocked '{model}': {source}")
            }
        }
    }
}

impl std::error::Error for RouterError {}

/// The R10 router: classify → filter by health → validate cost mode → plan.
#[derive(Debug)]
pub struct SmartRouter {
    registry: ModelRegistry,
    health: HealthTracker,
    policy: FreeOnlyPolicy,
    mode: CostMode,
}

impl SmartRouter {
    /// Build a router over the verified-free registry in [`CostMode::FreeOnly`].
    pub fn free_only() -> Self {
        Self {
            registry: ModelRegistry::verified_free(),
            health: HealthTracker::new(),
            policy: FreeOnlyPolicy::default(),
            mode: CostMode::FreeOnly,
        }
    }

    /// Build a router over a custom registry (BYOK setups).
    pub fn with_registry(registry: ModelRegistry, mode: CostMode) -> Self {
        Self {
            registry,
            health: HealthTracker::new(),
            policy: FreeOnlyPolicy::default(),
            mode,
        }
    }

    pub fn mode(&self) -> CostMode {
        self.mode
    }

    /// Switch cost mode at runtime (e.g. user attached / removed their key).
    pub fn set_mode(&mut self, mode: CostMode) {
        self.mode = mode;
    }

    /// Route one task to a primary + fallback plan.
    ///
    /// Steps: classify the task → take registry entries for that kind in
    /// priority order → drop [`HealthState::Dead`] models (degraded ones
    /// sink to the end) → in [`CostMode::FreeOnly`], validate every
    /// surviving candidate with [`FreeOnlyPolicy`] (a failure here is a
    /// [`RouterError::PaidModelBlocked`] bug-signal, never silent).
    pub fn route(&self, task: &str) -> Result<RoutePlan, RouterError> {
        let kind = TaskClassifier::classify(task);
        let mut candidates = self.registry.for_kind(kind);

        // Health filter: drop dead, demote degraded to the end (stable).
        candidates.retain(|m| self.health.state(&m.id) != HealthState::Dead);
        candidates.sort_by_key(|m| (self.health.state(&m.id) != HealthState::Healthy, m.priority));

        // Deduplicate by model id (keep highest-priority entry): health is
        // tracked per id, so a second entry with the same id would share
        // the first's fate anyway.
        let mut seen_ids = std::collections::HashSet::new();
        candidates.retain(|m| seen_ids.insert(m.id.clone()));

        // Cost-mode gate: FreeOnly validates every candidate up front.
        if self.mode == CostMode::FreeOnly {
            for m in &candidates {
                if let Err(e) = self.policy.validate(&m.id) {
                    return Err(RouterError::PaidModelBlocked {
                        model: m.id.clone(),
                        source: e,
                    });
                }
            }
        }

        let mut iter = candidates.into_iter();
        match iter.next() {
            Some(primary) => Ok(RoutePlan {
                kind,
                primary: (*primary).clone(),
                fallbacks: iter.map(|m| (*m).clone()).collect(),
            }),
            None => Err(RouterError::NoModelAvailable {
                kind,
                mode: self.mode,
            }),
        }
    }

    /// Report a successful call on `model_id` (resets its failure counter).
    pub fn record_success(&mut self, model_id: &str) {
        self.health.record_success(model_id);
    }

    /// Report a failed call (429 / 5xx / timeout). Returns the new state.
    pub fn record_failure(&mut self, model_id: &str) -> HealthState {
        self.health.record_failure(model_id)
    }

    /// Current health view (for UI / diagnostics).
    pub fn health_of(&self, model_id: &str) -> HealthState {
        self.health.state(model_id)
    }

    /// Forget in-session health (e.g. after the daily scan refreshes).
    pub fn reset_health(&mut self) {
        self.health.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_all_free() {
        let r = ModelRegistry::verified_free();
        assert!(!r.models().is_empty());
        for m in r.models() {
            assert!(
                cost_firewall::is_free_model(&m.id),
                "registry entry is not provably free: {}",
                m.id
            );
        }
    }

    #[test]
    fn classifier_code() {
        assert_eq!(
            TaskClassifier::classify("fix the null pointer bug in main.rs"),
            TaskKind::Code
        );
        assert_eq!(
            TaskClassifier::classify("refactor this function to be async"),
            TaskKind::Code
        );
        assert_eq!(
            TaskClassifier::classify("write a python script that parses logs"),
            TaskKind::Code
        );
    }

    #[test]
    fn classifier_reasoning() {
        assert_eq!(
            TaskClassifier::classify("prove that this algorithm is O(n log n)"),
            TaskKind::Reasoning
        );
        assert_eq!(
            TaskClassifier::classify("design a system architecture for 1M users, trade-offs?"),
            TaskKind::Reasoning
        );
    }

    #[test]
    fn classifier_vision_wins() {
        // Image input beats code keywords.
        assert_eq!(
            TaskClassifier::classify("look at this screenshot and fix the bug in the code"),
            TaskKind::Vision
        );
    }

    #[test]
    fn classifier_chat_default() {
        assert_eq!(
            TaskClassifier::classify("hello, how are you today?"),
            TaskKind::Chat
        );
        assert_eq!(TaskClassifier::classify(""), TaskKind::Chat);
    }

    #[test]
    fn route_returns_priority_ordered_plan() {
        let router = SmartRouter::free_only();
        let plan = router.route("fix the bug").unwrap();
        assert_eq!(plan.kind, TaskKind::Code);
        // Priority ascending.
        let ids = plan.model_ids();
        assert!(ids.len() >= 2, "need primary + at least one fallback");
        assert_eq!(ids[0], "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free");
        // No duplicates in the plan.
        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            assert!(seen.insert(*id), "duplicate model in plan: {id}");
        }
    }

    #[test]
    fn dead_models_are_skipped() {
        let mut router = SmartRouter::free_only();
        let first = router.route("fix the bug").unwrap();
        let dead_id = first.primary.id.clone();
        for _ in 0..HealthTracker::DEAD_AFTER {
            router.record_failure(&dead_id);
        }
        assert_eq!(router.health_of(&dead_id), HealthState::Dead);
        let second = router.route("fix the bug").unwrap();
        assert_ne!(second.primary.id, dead_id);
        assert!(
            !second.model_ids().contains(&dead_id.as_str()),
            "dead model must not appear anywhere in the plan"
        );
    }

    #[test]
    fn degraded_models_sink_to_end() {
        let mut router = SmartRouter::free_only();
        let first = router.route("hello").unwrap();
        let top_id = first.primary.id.clone();
        router.record_failure(&top_id); // 1 failure -> degraded
        assert_eq!(router.health_of(&top_id), HealthState::Degraded);
        let second = router.route("hello").unwrap();
        assert_ne!(second.primary.id, top_id);
        // Still present as a fallback (not dropped, just demoted).
        assert!(second.model_ids().contains(&top_id.as_str()));
    }

    #[test]
    fn success_resets_failures() {
        let mut router = SmartRouter::free_only();
        let plan = router.route("hello").unwrap();
        let id = plan.primary.id.clone();
        router.record_failure(&id);
        router.record_failure(&id);
        assert_eq!(router.health_of(&id), HealthState::Degraded);
        router.record_success(&id);
        assert_eq!(router.health_of(&id), HealthState::Healthy);
    }

    #[test]
    fn free_only_blocks_paid_registry() {
        let registry = ModelRegistry::custom(vec![ModelEntry::new(
            "gpt-4o-paid",
            "some-provider",
            "https://example.com/v1",
            vec![TaskKind::Chat],
            1,
        )]);
        let router = SmartRouter::with_registry(registry, CostMode::FreeOnly);
        match router.route("hello") {
            Err(RouterError::PaidModelBlocked { model, .. }) => {
                assert_eq!(model, "gpt-4o-paid");
            }
            other => panic!("expected PaidModelBlocked, got {other:?}"),
        }
    }

    #[test]
    fn byok_allows_custom_models() {
        let registry = ModelRegistry::custom(vec![ModelEntry::new(
            "my-custom-model",
            "my-provider",
            "https://example.com/v1",
            vec![TaskKind::Chat],
            1,
        )]);
        let router = SmartRouter::with_registry(registry, CostMode::BringYourOwnKey);
        let plan = router.route("hello").unwrap();
        assert_eq!(plan.primary.id, "my-custom-model");
    }

    #[test]
    fn empty_registry_is_no_model_available() {
        let router = SmartRouter::with_registry(ModelRegistry::custom(vec![]), CostMode::FreeOnly);
        match router.route("hello") {
            Err(RouterError::NoModelAvailable { kind, mode }) => {
                assert_eq!(kind, TaskKind::Chat);
                assert_eq!(mode, CostMode::FreeOnly);
            }
            other => panic!("expected NoModelAvailable, got {other:?}"),
        }
    }

    #[test]
    fn all_dead_is_no_model_available() {
        let mut router = SmartRouter::free_only();
        // Kill every chat-capable model.
        let ids: Vec<String> = router
            .route("hello")
            .unwrap()
            .model_ids()
            .iter()
            .map(|s| s.to_string())
            .collect();
        for id in &ids {
            for _ in 0..HealthTracker::DEAD_AFTER {
                router.record_failure(id);
            }
        }
        assert!(router.route("hello").is_err());
    }

    #[test]
    fn set_mode_switches() {
        let mut router = SmartRouter::free_only();
        assert_eq!(router.mode(), CostMode::FreeOnly);
        router.set_mode(CostMode::BringYourOwnKey);
        assert_eq!(router.mode(), CostMode::BringYourOwnKey);
    }

    #[test]
    fn attempt_order_primary_first() {
        let router = SmartRouter::free_only();
        let plan = router.route("summarize this").unwrap();
        let order = plan.attempt_order();
        assert_eq!(order[0].id, plan.primary.id);
        assert_eq!(order.len(), 1 + plan.fallbacks.len());
    }
}
