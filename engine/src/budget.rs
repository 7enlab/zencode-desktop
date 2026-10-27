use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use zen_core::{
    AgentEvent, ChatRequest, ChatResponse, EventSink, LlmBackend, PermissionChecker,
    PermissionDecision, ToolContext,
};

pub const BUDGET_EXCEEDED_EVENT: &str = "budget_exceeded";

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
    pub fn unlimited() -> Self {
        Self {
            max_input_tokens: u64::MAX,
            max_output_tokens: u64::MAX,
            max_tool_calls: u32::MAX,
            max_duration_secs: u64::MAX,
        }
    }
}

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

    pub fn record_llm_usage(&mut self, input_tokens: u64, output_tokens: u64) {
        self.input_tokens = self.input_tokens.saturating_add(input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(output_tokens);
    }

    pub fn record_tool_call(&mut self) {
        self.tool_calls = self.tool_calls.saturating_add(1);
    }

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

    pub fn record_tool_call(&self) -> Result<(), BudgetExceeded> {
        self.tracker.lock().unwrap().record_tool_call();
        self.check()
    }

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
        t.record_llm_usage(100, 50);
        t.record_tool_call();
        t.record_tool_call();
        t.record_tool_call();
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
        let Some(start) = Instant::now().checked_sub(Duration::from_secs(601)) else {
            return;
        };
        let t = BudgetTracker::new_with_start(small_budget(), start);
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
        enforcer.record_llm_usage(0, 500);
        assert!(enforcer.check().is_err());
        assert!(enforcer.check().is_err());
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

        assert_eq!(sink.system_messages().len(), 1);
    }
}
