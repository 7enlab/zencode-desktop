//! Versioned event schema the Desktop UI consumes. It is a stable projection of the CLI's
//! `zen_core::AgentEvent`, so a CLI bump can never silently change what the UI sees: the
//! contract tests (insta snapshots) fail first.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zen_core::{AgentEvent, EventSink};

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
    pub fn to_ndjson(&self) -> String {
        self.events()
            .iter()
            .map(|e| serde_json::to_string(e).expect("envelope serializes"))
            .collect::<Vec<_>>()
            .join("\n")
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
