//! R12 Hard Budget Authority (v0.4.0 "Trust" roadmap): per-task token, tool-call
//! and duration budgets with a hard stop.
//!
//! The community's unanimous advice for runaway agents is "set hard spend
//! limits". For ZenCode — where every model is `:free` — the budget stretches
//! free-tier quotas and kills runaway loops before they burn turns.
//!
//! Wiring (see [`wire`] and `run_task_with_sink` in `lib.rs`):
//! - [`BudgetedBackend`] wraps the [`LlmBackend`]: it records prompt/completion
//!   tokens after every model call and refuses the next call once any budget is
//!   exceeded. The agent loop turns that refusal into an error result, so the
//!   task stops — no further model calls, no further turns.
//! - [`BudgetedPermissions`] wraps the [`PermissionChecker`]: it counts every
//!   attempted tool call and denies (rather than runs) calls past the limit.
//! - On the first exceedance, a `budget_exceeded` [`SystemMessage`] event is
//!   emitted to the session sink, so the session log records exactly what
//!   stopped the task.
//!
//! [`LlmBackend`]: zen_core::LlmBackend
//! [`PermissionChecker`]: zen_core::PermissionChecker
//! [`SystemMessage`]: zen_core::AgentEvent::SystemMessage

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use zen_core::{
    AgentEvent, ChatRequest, ChatResponse, EventSink, LlmBackend, PermissionChecker,
    PermissionDecision, ToolContext,
};

/// `AgentEvent::SystemMessage` `event` name recorded when a budget trips.
/// UI code matches on this string to render the stop banner.
pub const BUDGET_EXCEEDED_EVENT: &str = "budget_exceeded";

/// Which budget dimension tripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetDimension {
    InputTokens,
    OutputTokens,
    ToolCalls,
    Duration,
}

impl fmt::Display for BudgetDimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BudgetDimension::InputTokens => write!(f, "input token"),
            BudgetDimension::OutputTokens => write!(f, "output token"),
            BudgetDimension::ToolCalls => write!(f, "tool call"),
            BudgetDimension::Duration => write!(f, "duration"),
        }
    }
}

/// Returned by [`BudgetTracker::check`] when a limit is over.
/// Displayed to the user verbatim, e.g.
/// `Task stopped: output token budget 50,000 exceeded (used 50,012)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetExceeded {
    pub dimension: BudgetDimension,
    pub limit: u64,
    pub used: u64,
}

impl fmt::Display for BudgetExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Task stopped: {} budget {} exceeded (used {})",
            self.dimension,
            fmt_int(self.limit),
            fmt_int(self.used)
        )
    }
}

impl std::error::Error for BudgetExceeded {}

/// Thousands separators: 50012 -> "50,012".
fn fmt_int(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while n > 0 {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push((b'0' + (n % 10) as u8) as char);
        n /= 10;
        i += 1;
    }
    out.iter().rev().collect()
}

/// Per-task hard budget. A limit of `0` means "none allowed"; use
/// [`TaskBudget::unlimited`] to disable a dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBudget {
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_tool_calls: u32,
    pub max_duration_secs: u64,
}

impl Default for TaskBudget {
    fn default() -> Self {
        Self {
            max_input_tokens: 200_000,
            max_output_tokens: 50_000,
            max_tool_calls: 100,
            max_duration_secs: 600,
        }
    }
}

impl TaskBudget {
    /// No effective limits (every dimension at its type maximum).
    pub fn unlimited() -> Self {
        Self {
            max_input_tokens: u64::MAX,
            max_output_tokens: u64::MAX,
            max_tool_calls: u32::MAX,
            max_duration_secs: u64::MAX,
        }
    }
}

/// Live counters for one task. Not `Sync` by itself — share via
/// [`BudgetEnforcer`].
#[derive(Debug)]
pub struct BudgetTracker {
    budget: TaskBudget,
    started: Instant,
    input_tokens: u64,
    output_tokens: u64,
    tool_calls: u32,
}

impl BudgetTracker {
    pub fn new(budget: TaskBudget) -> Self {
        Self::new_with_start(budget, Instant::now())
    }

    /// Constructor with an explicit start time (useful for testing duration).
    pub fn new_with_start(budget: TaskBudget, started: Instant) -> Self {
        Self {
            budget,
            started,
            input_tokens: 0,
            output_tokens: 0,
            tool_calls: 0,
        }
    }

    pub fn budget(&self) -> &TaskBudget {
        &self.budget
    }

    pub fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    pub fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    pub fn tool_calls(&self) -> u32 {
        self.tool_calls
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Add one model call's usage.
    pub fn record_llm_usage(&mut self, input_tokens: u64, output_tokens: u64) {
        self.input_tokens = self.input_tokens.saturating_add(input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(output_tokens);
    }

    /// Count one attempted tool call.
    pub fn record_tool_call(&mut self) {
        self.tool_calls = self.tool_calls.saturating_add(1);
    }

    /// `Ok` while every dimension is within its limit; otherwise the first
    /// tripped dimension (input, output, tool calls, duration — in that order).
    pub fn check(&self) -> Result<(), BudgetExceeded> {
        let b = &self.budget;
        if self.input_tokens > b.max_input_tokens {
            return Err(BudgetExceeded {
                dimension: BudgetDimension::InputTokens,
                limit: b.max_input_tokens,
                used: self.input_tokens,
            });
        }
        if self.output_tokens > b.max_output_tokens {
            return Err(BudgetExceeded {
                dimension: BudgetDimension::OutputTokens,
                limit: b.max_output_tokens,
                used: self.output_tokens,
            });
        }
        if self.tool_calls > b.max_tool_calls {
            return Err(BudgetExceeded {
                dimension: BudgetDimension::ToolCalls,
                limit: u64::from(b.max_tool_calls),
                used: u64::from(self.tool_calls),
            });
        }
        let elapsed = self.elapsed_secs();
        if elapsed > b.max_duration_secs {
            return Err(BudgetExceeded {
                dimension: BudgetDimension::Duration,
                limit: b.max_duration_secs,
                used: elapsed,
            });
        }
        Ok(())
    }
}

/// Shared enforcement state: the tracker plus the session sink the
/// `budget_exceeded` event is recorded on. The event is emitted exactly once
/// per task, no matter how many wrappers observe the trip.
pub struct BudgetEnforcer {
    tracker: Mutex<BudgetTracker>,
    sink: Arc<dyn EventSink>,
    notified: Mutex<bool>,
}

impl fmt::Debug for BudgetEnforcer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = self.tracker.lock().unwrap();
        f.debug_struct("BudgetEnforcer")
            .field("input_tokens", &t.input_tokens())
            .field("output_tokens", &t.output_tokens())
            .field("tool_calls", &t.tool_calls())
            .field("elapsed_secs", &t.elapsed_secs())
            .field("notified", &self.notified.lock().unwrap())
            .finish()
    }
}

impl BudgetEnforcer {
    pub fn new(budget: TaskBudget, sink: Arc<dyn EventSink>) -> Arc<Self> {
        Arc::new(Self {
            tracker: Mutex::new(BudgetTracker::new(budget)),
            sink,
            notified: Mutex::new(false),
        })
    }

    /// Check the budget; on the first trip, record the `budget_exceeded`
    /// event in the session log before returning the error.
    pub fn check(&self) -> Result<(), BudgetExceeded> {
        let err = self.tracker.lock().unwrap().check().err();
        match err {
            None => Ok(()),
            Some(e) => {
                let mut notified = self.notified.lock().unwrap();
                if !*notified {
                    *notified = true;
                    self.sink.emit(AgentEvent::SystemMessage {
                        event: BUDGET_EXCEEDED_EVENT.to_string(),
                        text: e.to_string(),
                    });
                }
                Err(e)
            }
        }
    }

    pub fn record_llm_usage(&self, input_tokens: u64, output_tokens: u64) {
        self.tracker
            .lock()
            .unwrap()
            .record_llm_usage(input_tokens, output_tokens);
    }

    /// Count one attempted tool call, then enforce.
    pub fn record_tool_call(&self) -> Result<(), BudgetExceeded> {
        self.tracker.lock().unwrap().record_tool_call();
        self.check()
    }

    /// Snapshot of the counters (for status UIs).
    pub fn snapshot(&self) -> (u64, u64, u32, u64) {
        let t = self.tracker.lock().unwrap();
        (
            t.input_tokens(),
            t.output_tokens(),
            t.tool_calls(),
            t.elapsed_secs(),
        )
    }
}

/// [`LlmBackend`] wrapper: records token usage and hard-stops the task at the
/// next model call once any budget is exceeded. The agent loop surfaces the
/// refusal as an `error_during_execution` result whose message is the
/// [`BudgetExceeded`] display string.
pub struct BudgetedBackend {
    inner: Arc<dyn LlmBackend>,
    enforcer: Arc<BudgetEnforcer>,
}

impl BudgetedBackend {
    pub fn new(inner: Arc<dyn LlmBackend>, enforcer: Arc<BudgetEnforcer>) -> Self {
        Self { inner, enforcer }
    }
}

#[async_trait]
impl LlmBackend for BudgetedBackend {
    async fn chat(&self, req: ChatRequest) -> anyhow::Result<ChatResponse> {
        self.enforcer
            .check()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let resp = self.inner.chat(req).await?;
        self.enforcer
            .record_llm_usage(resp.usage.prompt_tokens, resp.usage.completion_tokens);
        self.enforcer
            .check()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(resp)
    }
}

/// [`PermissionChecker`] wrapper: counts each attempted tool call and *denies*
/// (instead of running) calls past `max_tool_calls`. The denial message is the
/// [`BudgetExceeded`] display string, so the model sees why it was stopped.
pub struct BudgetedPermissions {
    inner: Arc<dyn PermissionChecker>,
    enforcer: Arc<BudgetEnforcer>,
}

impl BudgetedPermissions {
    pub fn new(inner: Arc<dyn PermissionChecker>, enforcer: Arc<BudgetEnforcer>) -> Self {
        Self { inner, enforcer }
    }
}

impl PermissionChecker for BudgetedPermissions {
    fn check(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        read_only: bool,
        ctx: &ToolContext,
    ) -> PermissionDecision {
        match self.enforcer.record_tool_call() {
            Ok(()) => self.inner.check(tool_name, input, read_only, ctx),
            Err(e) => PermissionDecision::Deny(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct VecSink {
        events: Mutex<Vec<AgentEvent>>,
    }

    impl VecSink {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: Mutex::new(vec![]),
            })
        }

        fn system_messages(&self) -> Vec<(String, String)> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    AgentEvent::SystemMessage { event, text } => {
                        Some((event.clone(), text.clone()))
                    }
                    _ => None,
                })
                .collect()
        }
    }

    impl EventSink for VecSink {
        fn emit(&self, ev: AgentEvent) {
            self.events.lock().unwrap().push(ev);
        }
    }

    fn small_budget() -> TaskBudget {
        TaskBudget {
            max_input_tokens: 100,
            max_output_tokens: 50,
            max_tool_calls: 3,
            max_duration_secs: 600,
        }
    }

    #[test]
    fn defaults_are_sane() {
        let b = TaskBudget::default();
        assert_eq!(b.max_input_tokens, 200_000);
        assert_eq!(b.max_output_tokens, 50_000);
        assert_eq!(b.max_tool_calls, 100);
        assert_eq!(b.max_duration_secs, 600);
    }

    #[test]
    fn small_usage_is_allowed() {
        let mut t = BudgetTracker::new(small_budget());
        t.record_llm_usage(100, 50); // exactly at the limits
        t.record_tool_call();
        t.record_tool_call();
        t.record_tool_call(); // exactly at the limit
        assert!(t.check().is_ok());
    }

    #[test]
    fn input_tokens_exceeded() {
        let mut t = BudgetTracker::new(small_budget());
        t.record_llm_usage(101, 0);
        assert_eq!(
            t.check().unwrap_err(),
            BudgetExceeded {
                dimension: BudgetDimension::InputTokens,
                limit: 100,
                used: 101,
            }
        );
    }

    #[test]
    fn output_tokens_exceeded() {
        let mut t = BudgetTracker::new(small_budget());
        t.record_llm_usage(0, 51);
        assert_eq!(
            t.check().unwrap_err(),
            BudgetExceeded {
                dimension: BudgetDimension::OutputTokens,
                limit: 50,
                used: 51,
            }
        );
    }

    #[test]
    fn tool_calls_exceeded() {
        let mut t = BudgetTracker::new(small_budget());
        for _ in 0..4 {
            t.record_tool_call();
        }
        assert_eq!(
            t.check().unwrap_err(),
            BudgetExceeded {
                dimension: BudgetDimension::ToolCalls,
                limit: 3,
                used: 4,
            }
        );
    }

    #[test]
    fn duration_exceeded() {
        let t = BudgetTracker::new_with_start(
            small_budget(),
            Instant::now() - Duration::from_secs(601),
        );
        let e = t.check().unwrap_err();
        assert_eq!(e.dimension, BudgetDimension::Duration);
        assert_eq!(e.limit, 600);
        assert!(e.used >= 601);
    }

    #[test]
    fn counters_saturate_instead_of_wrapping() {
        let mut t = BudgetTracker::new(TaskBudget::unlimited());
        t.record_llm_usage(u64::MAX, u64::MAX);
        t.record_llm_usage(1, 1);
        assert_eq!(t.input_tokens(), u64::MAX);
        assert_eq!(t.output_tokens(), u64::MAX);
        assert!(t.check().is_ok());
    }

    #[test]
    fn display_message_matches_spec_example() {
        let e = BudgetExceeded {
            dimension: BudgetDimension::OutputTokens,
            limit: 50_000,
            used: 50_012,
        };
        assert_eq!(
            e.to_string(),
            "Task stopped: output token budget 50,000 exceeded (used 50,012)"
        );
    }

    #[test]
    fn enforcer_records_event_exactly_once() {
        let sink = VecSink::new();
        let enforcer = BudgetEnforcer::new(small_budget(), sink.clone());
        enforcer.record_llm_usage(0, 500); // trip output budget
        assert!(enforcer.check().is_err());
        assert!(enforcer.check().is_err()); // second trip: no second event
        let msgs = sink.system_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, BUDGET_EXCEEDED_EVENT);
        assert!(msgs[0].1.contains("output token budget 50 exceeded"));
    }

    #[test]
    fn enforcer_stays_silent_within_budget() {
        let sink = VecSink::new();
        let enforcer = BudgetEnforcer::new(small_budget(), sink.clone());
        enforcer.record_llm_usage(10, 5);
        assert!(enforcer.check().is_ok());
        assert!(sink.system_messages().is_empty());
    }

    #[test]
    fn budgeted_permissions_denies_past_limit() {
        struct AllowAll;
        impl PermissionChecker for AllowAll {
            fn check(
                &self,
                _: &str,
                _: &serde_json::Value,
                _: bool,
                _: &ToolContext,
            ) -> PermissionDecision {
                PermissionDecision::Allow
            }
        }

        let sink = VecSink::new();
        let enforcer = BudgetEnforcer::new(small_budget(), sink.clone());
        let perms = BudgetedPermissions::new(Arc::new(AllowAll), enforcer);
        let ctx = ToolContext::new(std::env::temp_dir(), "test");
        let args = serde_json::json!({});
        for _ in 0..3 {
            assert!(matches!(
                perms.check("Read", &args, true, &ctx),
                PermissionDecision::Allow
            ));
        }
        match perms.check("Read", &args, true, &ctx) {
            PermissionDecision::Deny(msg) => {
                assert!(msg.contains("tool call budget 3 exceeded"), "{msg}")
            }
            d => panic!("expected Deny, got {d:?}"),
        }
        // The trip recorded the session event.
        assert_eq!(sink.system_messages().len(), 1);
    }
}
