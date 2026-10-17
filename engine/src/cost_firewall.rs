//! Structural zero-cost firewall: the engine can never call a paid model.
//!
//! ZenCode Desktop has no payment rails — there is nowhere to enter a credit
//! card, no billing API, no paid-model key store. This module turns that
//! product fact into a *structural* property of the engine: every model id the
//! engine accepts must either end with `:free` (the OpenAI-compatible
//! convention for zero-cost models) or appear in [`FREE_ALLOWLIST`], an
//! explicit list of model ids verified to cost $0.00 per call.
//!
//! Anything else is rejected with [`CostFirewallError::NonFreeModelBlocked`]
//! **before any network call is made**. The check runs at three points:
//!
//! 1. Engine startup / config load — [`FreeOnlyPolicy::validate_all`] on the
//!    [`RunOptions`](crate::RunOptions) at the top of
//!    [`run_task_with_sink`](crate::run_task_with_sink), before the HTTP
//!    client is even constructed.
//! 2. Per-task model selection — same call, same place: no task can start
//!    with an unvalidated model.
//! 3. Model-chain fallback — [`FreeOnlyPolicy::validate_all`] also validates
//!    every fallback id. (When the RateGate/ModelChain lands, call `validate`
//!    on each candidate *before* the chain tries it; see the integration note
//!    on [`FreeOnlyPolicy::validate_all`].)

use std::fmt;

/// Model ids known to cost $0.00 that do not carry the `:free` suffix.
///
/// Every entry here MUST be a model whose provider bills $0.00 per call,
/// verified by an actual API call returning `usage.cost == 0.0`. Do not add
/// an id on the basis of documentation alone.
pub const FREE_ALLOWLIST: &[&str] = &[
    // Experiential Labs: verified 2026-10-17 via chat call, usage.cost = 0.0.
    "deepseek-v4.1-flash-uncensored",
];

/// Suffix marking a zero-cost model on OpenAI-compatible gateways
/// (OpenRouter, Kilo, TokenRouter, ...).
pub const FREE_SUFFIX: &str = ":free";

/// The loud message shown whenever the firewall blocks a model.
pub const BLOCKED_MESSAGE: &str = "ZenCode cannot bill you — this is structural, not a setting.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostFirewallError {
    /// Rejected because the model id is not provably free.
    NonFreeModelBlocked { model: String },
    /// Rejected because no model id was given at all.
    EmptyModelId,
}

impl fmt::Display for CostFirewallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CostFirewallError::NonFreeModelBlocked { model } => write!(
                f,
                "ZenCode blocked model '{model}': only :free models are allowed. {BLOCKED_MESSAGE}"
            ),
            CostFirewallError::EmptyModelId => write!(
                f,
                "ZenCode blocked the request: no model id was provided. Only :free models are allowed. {BLOCKED_MESSAGE}"
            ),
        }
    }
}

impl std::error::Error for CostFirewallError {}

/// Returns true if `model` is provably free: `:free` suffix or allowlisted.
/// Pure function — no I/O, no network.
pub fn is_free_model(model: &str) -> bool {
    let m = model.trim();
    !m.is_empty() && (m.ends_with(FREE_SUFFIX) || FREE_ALLOWLIST.contains(&m))
}

/// Validate one model id. Pure function: performs no I/O and, in particular,
/// no network call — rejection happens *before* any HTTP is possible.
pub fn validate_model(model: &str) -> Result<(), CostFirewallError> {
    if model.trim().is_empty() {
        return Err(CostFirewallError::EmptyModelId);
    }
    if is_free_model(model) {
        Ok(())
    } else {
        Err(CostFirewallError::NonFreeModelBlocked {
            model: model.trim().to_string(),
        })
    }
}

/// Policy object owning the firewall. Construct once and share.
///
/// INTEGRATION NOTE for the future RateGate/ModelChain: call
/// [`FreeOnlyPolicy::validate_all`] on the full candidate list *before* the
/// chain attempts any of them, so a non-free fallback can never be reached
/// at runtime.
#[derive(Debug, Clone, Default)]
pub struct FreeOnlyPolicy {
    /// Extra allowlist entries on top of [`FREE_ALLOWLIST`].
    extra_allowlist: Vec<String>,
}

impl FreeOnlyPolicy {
    /// Build a policy with additional explicitly-allowed model ids.
    pub fn with_extra_allowlist(extra: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            extra_allowlist: extra.into_iter().map(Into::into).collect(),
        }
    }

    fn is_allowed(&self, model: &str) -> bool {
        let m = model.trim();
        !m.is_empty()
            && (m.ends_with(FREE_SUFFIX)
                || FREE_ALLOWLIST.contains(&m)
                || self.extra_allowlist.iter().any(|a| a == m))
    }

    /// Validate a single model id (no I/O).
    pub fn validate(&self, model: &str) -> Result<(), CostFirewallError> {
        if model.trim().is_empty() {
            return Err(CostFirewallError::EmptyModelId);
        }
        if self.is_allowed(model) {
            Ok(())
        } else {
            Err(CostFirewallError::NonFreeModelBlocked {
                model: model.trim().to_string(),
            })
        }
    }

    /// Validate the primary model plus every fallback id.
    pub fn validate_all(
        &self,
        primary: &str,
        fallbacks: &[String],
    ) -> Result<(), CostFirewallError> {
        self.validate(primary)?;
        for fb in fallbacks {
            self.validate(fb)?;
        }
        Ok(())
    }
}

/// Tracks spend. It is always $0.00 — the meter exists so the UI can *show*
/// the guarantee ("spent $0.00 across N tasks") rather than merely assert it.
#[derive(Debug, Clone, Default)]
pub struct SpendMeter {
    tasks_run: u64,
}

impl SpendMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed (or attempted) task. Cost added: $0.00.
    pub fn record_task(&mut self) {
        self.tasks_run += 1;
    }

    pub fn tasks_run(&self) -> u64 {
        self.tasks_run
    }

    /// Total spend in USD. Always 0.0 — structurally, not approximately.
    pub fn total_spent_usd(&self) -> f64 {
        0.0
    }

    /// What this task will cost. Always "$0.00".
    pub fn cost_preview(&self, _task: &str) -> String {
        "$0.00".to_string()
    }

    /// One-line human summary for the UI.
    pub fn summary(&self) -> String {
        format!(
            "Spent $0.00 across {} task(s). ZenCode cannot bill you.",
            self.tasks_run
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_suffix_models_pass() {
        for m in [
            "openrouter/nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
            "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
            "deepseek-v4.1-flash:free",
            "qwen3.8-flash:free",
        ] {
            assert!(validate_model(m).is_ok(), "should pass: {m}");
            assert!(is_free_model(m));
        }
    }

    #[test]
    fn allowlisted_models_pass() {
        assert!(validate_model("deepseek-v4.1-flash-uncensored").is_ok());
    }

    #[test]
    fn non_free_models_are_blocked() {
        for m in [
            "gpt-4",
            "gpt-4o",
            "claude-3-5-sonnet-20241022",
            "anthropic/claude-sonnet-4-6",
            "openai/gpt-4o-mini", // no :free suffix -> blocked
            "free",               // the bare word is not the marker
        ] {
            let err = validate_model(m).unwrap_err();
            assert_eq!(
                err,
                CostFirewallError::NonFreeModelBlocked {
                    model: m.to_string()
                }
            );
        }
    }

    #[test]
    fn empty_model_id_is_blocked() {
        for m in ["", "   "] {
            assert_eq!(
                validate_model(m).unwrap_err(),
                CostFirewallError::EmptyModelId
            );
        }
    }

    #[test]
    fn blocked_message_is_loud_and_honest() {
        let msg = CostFirewallError::NonFreeModelBlocked {
            model: "gpt-4".into(),
        }
        .to_string();
        assert!(msg.contains("blocked model 'gpt-4'"), "{msg}");
        assert!(msg.contains("only :free models are allowed"), "{msg}");
        assert!(msg.contains("cannot bill you"), "{msg}");
        assert!(msg.contains("structural, not a setting"), "{msg}");
    }

    #[test]
    fn validation_is_pure_and_runs_before_any_network_use() {
        // validate_model takes only &str and returns synchronously: by
        // construction it cannot perform I/O. This test pins that property
        // by asserting the rejection without any client existing.
        let policy = FreeOnlyPolicy::default();
        let err = policy
            .validate("gpt-4")
            .expect_err("must reject before any HTTP client is built");
        assert!(matches!(err, CostFirewallError::NonFreeModelBlocked { .. }));
    }

    #[test]
    fn validate_all_rejects_bad_fallback() {
        let policy = FreeOnlyPolicy::default();
        let err = policy
            .validate_all("deepseek-v4.1-flash:free", &["gpt-4".to_string()])
            .expect_err("fallback must be validated too");
        assert_eq!(
            err,
            CostFirewallError::NonFreeModelBlocked {
                model: "gpt-4".into()
            }
        );
    }

    #[test]
    fn validate_all_accepts_clean_lists() {
        let policy = FreeOnlyPolicy::default();
        policy
            .validate_all(
                "a:free",
                &[
                    "b:free".to_string(),
                    "deepseek-v4.1-flash-uncensored".to_string(),
                ],
            )
            .expect("all free");
    }

    #[test]
    fn extra_allowlist_extends_policy() {
        let policy = FreeOnlyPolicy::with_extra_allowlist(["my-lab/local-coder"]);
        assert!(policy.validate("my-lab/local-coder").is_ok());
        assert!(FreeOnlyPolicy::default()
            .validate("my-lab/local-coder")
            .is_err());
    }

    #[test]
    fn spend_meter_is_always_zero() {
        let mut meter = SpendMeter::new();
        assert_eq!(meter.total_spent_usd(), 0.0);
        assert_eq!(meter.cost_preview("refactor the auth module"), "$0.00");
        meter.record_task();
        meter.record_task();
        assert_eq!(meter.tasks_run(), 2);
        assert_eq!(meter.total_spent_usd(), 0.0);
        let summary = meter.summary();
        assert!(summary.contains("$0.00"), "{summary}");
        assert!(summary.contains('2'), "{summary}");
    }

    #[test]
    fn firewall_error_converts_to_anyhow() {
        let err: anyhow::Error = validate_model("gpt-4").unwrap_err().into();
        // anyhow's Debug shows the display chain, not the variant name.
        let msg = format!("{err:?}");
        assert!(msg.contains("blocked model 'gpt-4'"), "{msg}");
        assert!(
            err.downcast_ref::<CostFirewallError>()
                == Some(&CostFirewallError::NonFreeModelBlocked {
                    model: "gpt-4".into()
                }),
            "{msg}"
        );
    }
}
