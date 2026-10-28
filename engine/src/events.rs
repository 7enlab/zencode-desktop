//! Versioned event schema the Desktop UI consumes. It is a stable projection of the CLI's
//! `zen_core::AgentEvent`, so a CLI bump can never silently change what the UI sees: the
//! contract tests (insta snapshots) fail first.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zen_core::{AgentEvent, EventSink, RunResult};

/// Bump on any breaking change to [`DesktopEvent`] (and re-review the snapshots).
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    /// Raw JSON arguments string as the model produced it.
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesktopEvent {
    AssistantMessage {
        text: Option<String>,
        tool_calls: Vec<ToolCallInfo>,
        parent_tool_use_id: Option<String>,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        parent_tool_use_id: Option<String>,
    },
    TextDelta {
        text: String,
    },
    ApiRetry {
        attempt: u32,
        max_retries: u32,
        retry_delay_ms: u64,
        error_status: Option<u16>,
        error: String,
    },
    CompactBoundary {
        trigger: String,
        pre_tokens: u64,
    },
    PermissionDenied {
        tool_name: String,
        tool_use_id: String,
    },
    SystemMessage {
        event: String,
        text: String,
    },
    /// A future `AgentEvent` variant this engine does not know yet. `AgentEvent` is
    /// `#[non_exhaustive]`, so new variants must never silently change what the UI sees:
    /// they surface here with their variant name instead of being dropped.
    Unknown {
        variant: String,
    },
    /// End of a session, with cost accounting. Mirrors Claude Code's `result`
    /// stream-json line; feeds the RateGate quota UI (D8).
    SessionEnd {
        success: bool,
        result: String,
        num_turns: u64,
        total_cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
        model: String,
        duration_ms: u64,
        /// R4 Verified Claims summary (see `crate::claims`).
        claims_verified: u64,
        claims_unverified: u64,
        claims_contradicted: u64,
    },
}

/// One line of the Desktop event log: `{"v":1, "seq":N, ...event}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub seq: u64,
    #[serde(flatten)]
    pub event: DesktopEvent,
}

impl From<AgentEvent> for DesktopEvent {
    fn from(ev: AgentEvent) -> Self {
        match ev {
            AgentEvent::Assistant {
                message,
                parent_tool_use_id,
            } => DesktopEvent::AssistantMessage {
                text: message.content.filter(|c| !c.is_empty()),
                tool_calls: message
                    .tool_calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| ToolCallInfo {
                        id: t.id,
                        name: t.function.name,
                        arguments: t.function.arguments,
                    })
                    .collect(),
                parent_tool_use_id,
            },
            AgentEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
                parent_tool_use_id,
                // New in CLI rev 290154c: parallel-call marker, not part of
                // the Desktop v1 contract, so intentionally not surfaced.
                parallel: _,
            } => DesktopEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
                parent_tool_use_id,
            },
            AgentEvent::TextDelta { text } => DesktopEvent::TextDelta { text },
            AgentEvent::ApiRetry {
                attempt,
                max_retries,
                retry_delay_ms,
                error_status,
                error,
            } => DesktopEvent::ApiRetry {
                attempt,
                max_retries,
                retry_delay_ms,
                error_status,
                error,
            },
            AgentEvent::CompactBoundary {
                trigger,
                pre_tokens,
            } => DesktopEvent::CompactBoundary {
                trigger,
                pre_tokens,
            },
            // The tool input is deliberately dropped: it may hold secrets.
            AgentEvent::PermissionDenied(d) => DesktopEvent::PermissionDenied {
                tool_name: d.tool_name,
                tool_use_id: d.tool_use_id,
            },
            AgentEvent::SystemMessage { event, text } => {
                DesktopEvent::SystemMessage { event, text }
            }
            // `AgentEvent` is #[non_exhaustive]: a future variant must surface, never vanish.
            unhandled => DesktopEvent::Unknown {
                variant: format!("{:?}", unhandled)
                    .split([' ', '('])
                    .next()
                    .unwrap_or("Unknown")
                    .to_string(),
            },
        }
    }
}

/// Collects events as versioned NDJSON-ready envelopes (the UI bridge forwards each one).
#[derive(Default)]
pub struct RecordingSink {
    inner: Mutex<Vec<Envelope>>,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn events(&self) -> Vec<Envelope> {
        self.inner.lock().unwrap().clone()
    }
    /// One JSON object per line.
    pub fn to_ndjson(&self) -> anyhow::Result<String> {
        let mut out = String::new();
        for e in self.events().iter() {
            let line = serde_json::to_string(e)?;
            out.push_str(&line);
            out.push('\n');
        }
        if out.ends_with('\n') {
            out.pop();
        }
        Ok(out)
    }

    /// Record the end of a session with cost accounting (P4: feeds the RateGate UI).
    /// Claim counts are reported as zero; use [`Self::emit_session_end_with_claims`]
    /// when an R4 claim ledger is available.
    pub fn emit_session_end(&self, result: &RunResult) {
        self.emit_session_end_with_claims(result, &crate::claims::ClaimCounts::default());
    }

    /// Record the end of a session with cost accounting (P4: feeds the RateGate UI)
    /// plus the R4 Verified Claims summary counts.
    pub fn emit_session_end_with_claims(
        &self,
        result: &RunResult,
        counts: &crate::claims::ClaimCounts,
    ) {
        let mut g = self.inner.lock().unwrap();
        let seq = g.len() as u64;
        g.push(Envelope {
            v: SCHEMA_VERSION,
            seq,
            event: DesktopEvent::SessionEnd {
                success: !result.is_error,
                result: result.result.clone(),
                num_turns: result.num_turns as u64,
                total_cost_usd: result.total_cost_usd,
                input_tokens: result.usage.input_tokens,
                output_tokens: result.usage.output_tokens,
                model: result.model.clone(),
                duration_ms: result.duration_ms,
                claims_verified: counts.verified,
                claims_unverified: counts.unverified,
                claims_contradicted: counts.contradicted,
            },
        });
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, ev: AgentEvent) {
        let mut g = self.inner.lock().unwrap();
        let seq = g.len() as u64;
        g.push(Envelope {
            v: SCHEMA_VERSION,
            seq,
            event: ev.into(),
        });
    }
}

/// Parse an NDJSON log; rejects lines from a newer schema instead of misreading them.
pub fn parse_ndjson(s: &str) -> anyhow::Result<Vec<Envelope>> {
    let mut out = vec![];
    for (i, line) in s.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let raw: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| anyhow::anyhow!("line {}: invalid JSON: {e}", i + 1))?;
        let v = raw.get("v").and_then(|v| v.as_u64()).unwrap_or(0);
        if v == 0 || v > SCHEMA_VERSION as u64 {
            anyhow::bail!("line {}: unsupported schema version {v}", i + 1);
        }
        out.push(
            serde_json::from_value(raw)
                .map_err(|e| anyhow::anyhow!("line {}: bad event: {e}", i + 1))?,
        );
    }
    Ok(out)
}
