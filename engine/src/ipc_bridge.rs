//! Engine → Tauri IPC bridge (g6).
//!
//! Implements the engine side of the D1/D2 designs (`docs/research/d1-stream-emitter-2026-10-17.md`,
//! `docs/research/d2-ipc-design-2026-10-17.md`):
//!
//! - [`IpcEnvelope`] — the wire format the Tauri `Channel<Envelope>` carries:
//!   `{"v":1,"seq":42,"type":"text_delta","text":"Hello, "}`.
//! - [`BridgeSink`] — an `EventSink` that converts each `AgentEvent` into an
//!   [`IpcEnvelope`] with a monotonically increasing `seq`, stores them, and
//!   serializes to NDJSON (the single source of truth for persistence).
//! - [`BridgeSink::get_log_since`] — reconnect replay for the D2 `get_log_since`
//!   Tauri command.
//! - [`detect_gaps`] — frontend-side helper: find missing `seq`s in what was received.
//!
//! No Tauri dependency: this is the pure engine side. The Tauri command layer
//! (one `Channel` per run, rAF batching) lives in `desktop/` and consumes this
//! module's NDJSON / `get_log_since` output.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zen_core::{AgentEvent, EventSink};

use crate::events::{parse_ndjson, DesktopEvent, Envelope, SCHEMA_VERSION};

/// The wire envelope: schema version, per-run sequence, event type, event payload.
///
/// Serializes to exactly the D2 wire format:
/// `{"v":1,"seq":42,"type":"text_delta","text":"Hello, "}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcEnvelope {
    pub v: u32,
    pub seq: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(flatten)]
    pub payload: serde_json::Value,
}

impl IpcEnvelope {
    /// Build from a [`DesktopEvent`], splitting the `type` tag from the payload.
    pub fn new(seq: u64, event: &DesktopEvent) -> anyhow::Result<Self> {
        let mut value = serde_json::to_value(event)
            .map_err(|e| anyhow::anyhow!("event not serializable: {e}"))?;
        let obj = value
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("event serialized to non-object"))?;
        let event_type = obj
            .remove("type")
            .and_then(|t| t.as_str().map(str::to_owned))
            .ok_or_else(|| anyhow::anyhow!("event missing type tag"))?;
        Ok(IpcEnvelope {
            v: SCHEMA_VERSION,
            seq,
            event_type,
            payload: serde_json::Value::Object(obj.clone()),
        })
    }

    /// The snake_case type tag of a [`DesktopEvent`] (e.g. `"text_delta"`).
    pub fn event_type_of(event: &DesktopEvent) -> String {
        match event {
            DesktopEvent::AssistantMessage { .. } => "assistant_message",
            DesktopEvent::ToolResult { .. } => "tool_result",
            DesktopEvent::TextDelta { .. } => "text_delta",
            DesktopEvent::ApiRetry { .. } => "api_retry",
            DesktopEvent::CompactBoundary { .. } => "compact_boundary",
            DesktopEvent::PermissionDenied { .. } => "permission_denied",
            DesktopEvent::SystemMessage { .. } => "system_message",
        }
        .to_owned()
    }

    /// Reconstruct the [`DesktopEvent`] by merging the type tag back in.
    pub fn to_event(&self) -> anyhow::Result<DesktopEvent> {
        let mut value = self.payload.clone();
        let obj = value
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("payload is not an object"))?;
        obj.insert(
            "type".to_owned(),
            serde_json::Value::String(self.event_type.clone()),
        );
        serde_json::from_value(value).map_err(|e| anyhow::anyhow!("bad event payload: {e}"))
    }

    /// Convert to the engine's [`Envelope`] (used by the NDJSON log).
    pub fn to_envelope(&self) -> anyhow::Result<Envelope> {
        Ok(Envelope {
            v: self.v,
            seq: self.seq,
            event: self.to_event()?,
        })
    }

    /// Build from an [`Envelope`].
    pub fn from_envelope(env: &Envelope) -> anyhow::Result<Self> {
        IpcEnvelope::new(env.seq, &env.event)
    }
}

/// Push-side sink for one agent run: assigns gapless `seq`s, stores every
/// envelope, and serves reconnect replays. Feed it to `run_task_with_sink`
/// (optionally alongside a `BroadcastSink` per the D1 design).
pub struct BridgeSink {
    seq: AtomicU64,
    log: Mutex<Vec<IpcEnvelope>>,
}

impl BridgeSink {
    pub fn new() -> Self {
        BridgeSink {
            seq: AtomicU64::new(0),
            log: Mutex::new(Vec::new()),
        }
    }

    /// All envelopes so far, in `seq` order.
    pub fn envelopes(&self) -> Vec<IpcEnvelope> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Envelopes with `seq > since`, for the D2 `get_log_since` Tauri command.
    /// The UI replays the persisted NDJSON log, then polls this in "follow" mode
    /// until the run's terminal event arrives.
    pub fn get_log_since(&self, since: u64) -> Vec<IpcEnvelope> {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|e| e.seq > since)
            .cloned()
            .collect()
    }

    /// The whole run as NDJSON (one envelope per line) — the single source of
    /// truth the frontend persists for resume.
    pub fn to_ndjson(&self) -> String {
        self.envelopes()
            .iter()
            .map(|e| serde_json::to_string(e).expect("envelope serializes"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Rebuild a sink from a persisted NDJSON log; the next emitted envelope
    /// continues the sequence after the highest stored `seq`.
    pub fn from_ndjson(s: &str) -> anyhow::Result<Self> {
        let envelopes: Vec<Envelope> = parse_ndjson(s)?;
        let mut log: Vec<IpcEnvelope> = envelopes
            .iter()
            .map(IpcEnvelope::from_envelope)
            .collect::<Result<_, _>>()?;
        log.sort_by_key(|e| e.seq);
        let next = log.last().map(|e| e.seq + 1).unwrap_or(0);
        Ok(BridgeSink {
            seq: AtomicU64::new(next),
            log: Mutex::new(log),
        })
    }
}

impl Default for BridgeSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSink for BridgeSink {
    fn emit(&self, ev: AgentEvent) {
        let event = DesktopEvent::from(ev);
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        // The conversion is infallible for events we just built; on the
        // impossible error path we still advance seq to keep it gapless.
        if let Ok(env) = IpcEnvelope::new(seq, &event) {
            self.log.lock().unwrap_or_else(|e| e.into_inner()).push(env);
        }
    }
}

/// Find missing sequence numbers in what the frontend received.
///
/// `received` is the list of `seq`s the UI has seen (any order, may contain
/// duplicates). Returns the sorted missing seqs in `[0, max(received)]`.
/// The webview IPC bridge delivers channel messages in order, so a non-empty
/// result means silent corruption — surface it as a loud, debuggable warning
/// (D2: "one integer comparison").
pub fn detect_gaps(received: &[u64]) -> Vec<u64> {
    let Some(&max) = received.iter().max() else {
        return vec![];
    };
    let seen: std::collections::HashSet<u64> = received.iter().copied().collect();
    (0..=max).filter(|s| !seen.contains(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(text: &str) -> DesktopEvent {
        DesktopEvent::TextDelta {
            text: text.to_owned(),
        }
    }

    #[test]
    fn envelope_splits_type_from_payload() {
        let env = IpcEnvelope::new(42, &delta("Hello, ")).unwrap();
        assert_eq!(env.v, SCHEMA_VERSION);
        assert_eq!(env.seq, 42);
        assert_eq!(env.event_type, "text_delta");
        assert_eq!(env.payload["text"], "Hello, ");
        // Wire format matches D2 exactly: {"v":1,"seq":42,"type":"text_delta",...}
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains(r#""type":"text_delta""#), "{json}");
        assert!(!json.contains("event_type"), "{json}");
    }

    #[test]
    fn envelope_roundtrip() {
        let event = DesktopEvent::ToolResult {
            tool_use_id: "tu_1".into(),
            content: "ok".into(),
            is_error: false,
            parent_tool_use_id: None,
        };
        let env = IpcEnvelope::new(7, &event).unwrap();
        assert_eq!(env.to_event().unwrap(), event);
        assert_eq!(env.to_envelope().unwrap().seq, 7);
    }

    #[test]
    fn event_type_of_covers_all_variants() {
        // Every DesktopEvent variant must map to a non-empty type tag.
        let variants = vec![
            DesktopEvent::AssistantMessage {
                text: None,
                tool_calls: vec![],
                parent_tool_use_id: None,
            },
            delta("x"),
            DesktopEvent::ApiRetry {
                attempt: 1,
                max_retries: 3,
                retry_delay_ms: 100,
                error_status: None,
                error: "e".into(),
            },
            DesktopEvent::CompactBoundary {
                trigger: "t".into(),
                pre_tokens: 1,
            },
            DesktopEvent::PermissionDenied {
                tool_name: "bash".into(),
                tool_use_id: "u".into(),
            },
            DesktopEvent::SystemMessage {
                event: "e".into(),
                text: "t".into(),
            },
        ];
        for v in &variants {
            let tag = IpcEnvelope::event_type_of(v);
            assert!(!tag.is_empty());
            // ...and the tag must match what serde produced.
            assert_eq!(IpcEnvelope::new(0, v).unwrap().event_type, tag);
        }
    }

    #[test]
    fn bridge_sink_seq_is_monotonic() {
        let sink = BridgeSink::new();
        for i in 0..5 {
            sink.emit(AgentEvent::TextDelta {
                text: format!("t{i}"),
            });
        }
        let envs = sink.envelopes();
        let seqs: Vec<u64> = envs.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn bridge_sink_ndjson_roundtrip() {
        let sink = BridgeSink::new();
        sink.emit(AgentEvent::TextDelta { text: "a".into() });
        sink.emit(AgentEvent::TextDelta { text: "b".into() });
        let ndjson = sink.to_ndjson();
        assert_eq!(ndjson.lines().count(), 2);

        let restored = BridgeSink::from_ndjson(&ndjson).unwrap();
        assert_eq!(restored.envelopes().len(), 2);
        // Sequence continues after restore — no seq reuse.
        restored.emit(AgentEvent::TextDelta { text: "c".into() });
        let seqs: Vec<u64> = restored.envelopes().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
    }

    #[test]
    fn get_log_since_returns_tail() {
        let sink = BridgeSink::new();
        for i in 0..5 {
            sink.emit(AgentEvent::TextDelta {
                text: format!("t{i}"),
            });
        }
        let tail = sink.get_log_since(2);
        let seqs: Vec<u64> = tail.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![3, 4]);
        assert!(sink.get_log_since(99).is_empty());
    }

    #[test]
    fn detect_gaps_finds_missing() {
        assert_eq!(detect_gaps(&[]), Vec::<u64>::new());
        assert_eq!(detect_gaps(&[0, 1, 2]), Vec::<u64>::new());
        assert_eq!(detect_gaps(&[0, 2, 5]), vec![1, 3, 4]);
        // Duplicates and disorder are fine.
        assert_eq!(detect_gaps(&[3, 1, 3, 0]), vec![2]);
    }

    #[test]
    fn bridge_sink_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BridgeSink>();
    }
}
