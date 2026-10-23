//! Parser for the ZenCode CLI's `stream-json` v1 output.
//!
//! The Desktop spawns the CLI as a subprocess
//! (`zen -p --output-format stream-json`) and consumes its stdout as
//! newline-delimited JSON. Every line carries `"v": 1`; the process exits with
//! 0 (success), 1 (error), 130 (interrupted) or 143 (terminated).
//!
//! Unknown line types are never dropped: they surface as
//! [`crate::events::DesktopEvent::Unknown`], mirroring this crate's
//! "surface, never silently change" rule for future variants.
//!
//! # Notes on the contract
//!
//! This module was first written from the CLI/Desktop contract v1 before the
//! authoritative `docs/stream-json-v1.md` (CLI team, zen 0.1.0) landed. The
//! open questions were reconciled against that
//! document on 2026-10-23: assistant `content` is a plain string (omitted when
//! the message only calls tools), tool calls arrive as OpenAI-style
//! `message.tool_calls` (never Claude-style `tool_use` blocks), the `tools`
//! list rides on `system/init`, `user` tool results are arrays of
//! `tool_result` blocks, and the `result` line carries the `model` name. All
//! five are handled below.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::events::{DesktopEvent, ToolCallInfo};

/// stream-json schema version this parser accepts. Lines with any other
/// `"v"` are rejected rather than misread.
pub const STREAM_JSON_VERSION: u64 = 1;

/// Outcome of a CLI subprocess run, interpreted from its exit code
/// (CLI↔Desktop contract v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliExit {
    /// `0` — the agent loop finished on its own.
    Success,
    /// `1` — the CLI reported an error (details are on its stderr).
    Error,
    /// `130` — interrupted by SIGINT (Ctrl-C / user stop).
    Interrupted,
    /// `143` — terminated by SIGTERM.
    Terminated,
    /// Anything else the contract does not define.
    Other(i32),
}

/// Interpret a CLI subprocess exit code per the v1 contract.
pub fn interpret_exit_code(code: i32) -> CliExit {
    match code {
        0 => CliExit::Success,
        1 => CliExit::Error,
        130 => CliExit::Interrupted,
        143 => CliExit::Terminated,
        other => CliExit::Other(other),
    }
}

/// A content block inside a stream-json message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        /// Raw tool input as the model produced it; kept verbatim.
        #[serde(default)]
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        /// Per docs/stream-json-v1.md, `content` is the model-visible text;
        /// arrays of blocks are normalized to text by [`tool_result_text`].
        #[serde(default)]
        content: Value,
        #[serde(default)]
        is_error: Option<bool>,
    },
    /// A block kind this parser does not know. Never dropped: the whole line
    /// still parses, and the block is skipped only when projecting to
    /// [`DesktopEvent`]s (the line itself is never lost).
    #[serde(other)]
    Unknown,
}

/// One OpenAI-style tool call on an `assistant` message's `tool_calls` field:
/// `{id, type: "function", function: {name, arguments}}`, where `arguments`
/// is a JSON-encoded string (docs/stream-json-v1.md, zen 0.1.0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    /// Always `"function"` in v1; kept for fidelity, never matched on.
    #[serde(default)]
    pub r#type: Option<String>,
    pub function: ToolFunction,
}

/// The `function` payload of an OpenAI-style tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    /// JSON-encoded argument string; passed through verbatim.
    #[serde(default)]
    pub arguments: String,
}

/// The `message` payload of `assistant` / `user` lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamMessage {
    #[serde(default)]
    pub role: Option<String>,
    /// `content` is a plain string on `assistant` lines (omitted when the
    /// message only calls tools) and an array of `tool_result` blocks on
    /// `user` lines. A string is normalized to a single `text` block here.
    #[serde(default, deserialize_with = "de_content_blocks")]
    pub content: Vec<ContentBlock>,
    /// OpenAI-style tool calls on `assistant` lines (`message.tool_calls`);
    /// omitted when the message calls no tools.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

fn de_content_blocks<'de, D>(d: D) -> Result<Vec<ContentBlock>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::Array(items) => items
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom),
        Value::String(text) => Ok(vec![ContentBlock::Text { text }]),
        Value::Null => Ok(vec![]),
        other => Err(serde::de::Error::custom(format!(
            "message.content must be an array of blocks or a string, got {other}"
        ))),
    }
}

/// Token usage attached to a `result` line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StreamUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

/// One stream-json line's payload, keyed by its `type` tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CliStreamEvent {
    System {
        subtype: String,
        #[serde(default)]
        session_id: Option<String>,
        /// Tool names offered to the model, in registry order (on `init`).
        #[serde(default)]
        tools: Vec<String>,
        #[serde(default)]
        model: Option<String>,
    },
    Assistant {
        message: StreamMessage,
    },
    User {
        message: StreamMessage,
    },
    Result {
        subtype: String,
        #[serde(default)]
        is_error: bool,
        /// Final text the CLI reports for the run.
        #[serde(default)]
        result: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        total_cost_usd: Option<f64>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        num_turns: Option<u64>,
        #[serde(default)]
        usage: StreamUsage,
        /// The model that served this turn (present on every `result` line
        /// per docs/stream-json-v1.md).
        #[serde(default)]
        model: Option<String>,
    },
    /// A line type this parser does not know. The raw `type` tag is kept on
    /// [`CliLine::line_type`]; the event surfaces as `DesktopEvent::Unknown`.
    #[serde(other)]
    Unknown,
}

/// One parsed stream-json line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CliLine {
    /// Schema version; always [`STREAM_JSON_VERSION`] after [`parse_stream_json`].
    pub v: u64,
    /// Raw `type` tag as emitted by the CLI (kept so unknown line types can
    /// be surfaced instead of dropped).
    pub line_type: String,
    /// The typed payload.
    pub event: CliStreamEvent,
}

impl<'de> Deserialize<'de> for CliLine {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Value::deserialize(d)?;
        let v = raw
            .get("v")
            .and_then(Value::as_u64)
            .ok_or_else(|| serde::de::Error::custom("stream-json line is missing a numeric \"v\""))?;
        let line_type = raw
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let event = serde_json::from_value(raw).map_err(serde::de::Error::custom)?;
        Ok(CliLine { v, line_type, event })
    }
}

/// Parse CLI stream-json v1 output (NDJSON). Fails fast on the first bad line:
/// invalid JSON, a missing/unsupported `"v"`, or a structurally invalid event.
/// Blank lines are ignored.
pub fn parse_stream_json(s: &str) -> anyhow::Result<Vec<CliLine>> {
    let mut out = Vec::new();
    for (i, raw_line) in s.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let raw: Value = serde_json::from_str(line)
            .map_err(|e| anyhow::anyhow!("stream-json line {}: invalid JSON: {e}", i + 1))?;
        match raw.get("v").and_then(Value::as_u64) {
            Some(STREAM_JSON_VERSION) => {}
            other => anyhow::bail!(
                "stream-json line {}: unsupported schema version {other:?} (expected v={STREAM_JSON_VERSION})",
                i + 1
            ),
        }
        out.push(
            serde_json::from_value::<CliLine>(raw)
                .map_err(|e| anyhow::anyhow!("stream-json line {}: bad event: {e}", i + 1))?,
        );
    }
    Ok(out)
}

/// Normalize a `tool_result` content value to display text.
fn tool_result_text(content: &Value) -> String {
    match content {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                Value::String(s) => Some(s.clone()),
                Value::Object(map) => map.get("text").and_then(Value::as_str).map(str::to_owned),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

/// Project one parsed CLI line onto the Desktop's stable event schema.
/// There are no ignorable lines here: even unknown line types come back as
/// [`DesktopEvent::Unknown`], so a CLI change can never silently vanish.
pub fn to_desktop_events(line: &CliLine) -> Vec<DesktopEvent> {
    match &line.event {
        CliStreamEvent::System {
            subtype,
            session_id,
            tools,
            model,
        } => {
            let mut text = format!("subtype={subtype}");
            if let Some(id) = session_id {
                text.push_str(&format!(" session={id}"));
            }
            if let Some(m) = model {
                text.push_str(&format!(" model={m}"));
            }
            if !tools.is_empty() {
                text.push_str(&format!(" tools={}", tools.len()));
            }
            vec![DesktopEvent::SystemMessage {
                event: format!("cli_system:{subtype}"),
                text,
            }]
        }
        CliStreamEvent::Assistant { message } => {
            let text: String = message
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            // OpenAI-style `message.tool_calls` is the real v1 shape; the
            // Claude-style `tool_use` blocks below are tolerated only
            // (the v1 CLI never emits them).
            let mut tool_calls: Vec<ToolCallInfo> = message
                .tool_calls
                .iter()
                .map(|tc| ToolCallInfo {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                })
                .collect();
            tool_calls.extend(message.content.iter().filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => Some(ToolCallInfo {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: input.to_string(),
                }),
                _ => None,
            }));
            vec![DesktopEvent::AssistantMessage {
                text: if text.is_empty() { None } else { Some(text) },
                tool_calls,
                parent_tool_use_id: None,
            }]
        }
        CliStreamEvent::User { message } => message
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => Some(DesktopEvent::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: tool_result_text(content),
                    is_error: is_error.unwrap_or(false),
                    parent_tool_use_id: None,
                }),
                _ => None,
            })
            .collect(),
        CliStreamEvent::Result {
            subtype,
            is_error,
            result,
            total_cost_usd,
            duration_ms,
            num_turns,
            usage,
            model,
            ..
        } => {
            vec![DesktopEvent::SessionEnd {
                success: !is_error && subtype == "success",
                result: result.clone().unwrap_or_default(),
                num_turns: num_turns.unwrap_or(0),
                total_cost_usd: total_cost_usd.unwrap_or(0.0),
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                model: model.clone().unwrap_or_default(),
                duration_ms: duration_ms.unwrap_or(0),
                claims_verified: 0,
                claims_unverified: 0,
                claims_contradicted: 0,
            }]
        }
        CliStreamEvent::Unknown => vec![DesktopEvent::Unknown {
            variant: format!("cli_stream:{}", line.line_type),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_string_content_becomes_text_block() {
        let line: CliLine = serde_json::from_str(
            r#"{"v":1,"type":"assistant","message":{"role":"assistant","content":"xin chào"}}"#,
        )
        .unwrap();
        match line.event {
            CliStreamEvent::Assistant { message } => {
                assert_eq!(
                    message.content,
                    vec![ContentBlock::Text {
                        text: "xin chào".to_owned()
                    }]
                );
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn exit_codes_follow_contract_v1() {
        assert_eq!(interpret_exit_code(0), CliExit::Success);
        assert_eq!(interpret_exit_code(1), CliExit::Error);
        assert_eq!(interpret_exit_code(130), CliExit::Interrupted);
        assert_eq!(interpret_exit_code(143), CliExit::Terminated);
        assert_eq!(interpret_exit_code(2), CliExit::Other(2));
    }

    #[test]
    fn rejects_wrong_schema_version() {
        let err = parse_stream_json("{\"v\":2,\"type\":\"system\",\"subtype\":\"init\"}").unwrap_err();
        assert!(err.to_string().contains("unsupported schema version"), "{err}");
    }

    #[test]
    fn rejects_invalid_json() {
        let err = parse_stream_json("{\"v\":1, not json").unwrap_err();
        assert!(err.to_string().contains("invalid JSON"), "{err}");
    }
}
