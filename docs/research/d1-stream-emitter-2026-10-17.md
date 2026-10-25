# D1 — StreamEmitter: engine→UI event streaming (design)

Design for the trait the Desktop UI (Tauri, g6) uses to receive engine events live.
Docs-only; no implementation. Targets `origin/main` (schema v1, no SessionEnd yet).

## 1. Research

### What the engine already provides
- `DesktopEvent` (`engine/src/events.rs`): versioned (`SCHEMA_VERSION = 1`) UI-facing
  projection of `zen_core::AgentEvent`. Insta contract tests fail first if a CLI bump
  changes what the UI sees.
- `Envelope { v, seq, event }`: NDJSON-ready; `seq` assigned by the sink.
- `RecordingSink`: implements `zen_core::EventSink::emit(&self, ev: AgentEvent)`,
  collects envelopes in memory, `to_ndjson()` for persistence.
- `run_task_with_sink(opts, task, sink: Arc<dyn EventSink>)`: the agent loop pushes
  every event into the sink synchronously.

Gap: `RecordingSink` is pull-based (the UI reads `events()` after the run). A live
chat UI needs push — each event forwarded to the frontend the moment it happens.

### Tauri 2 streaming primitives
- `tauri::ipc::Channel<T: Serialize>`: created in JS, passed into a Rust command;
  `channel.send(value)` pushes to the frontend. One channel per command invocation —
  a natural fit for one agent run. `send` returning `Err` is the disconnect signal.
- `app.emit("event", payload)`: global broadcast; simpler, but every window gets
  every run's events (needs run-id filtering) and offers no backpressure signal.

Recommendation: one `Channel<Envelope>` per run.

## 2. Ideas

### The trait — minimal, object-safe, synchronous
```rust
pub trait StreamEmitter: Send + Sync {
    /// Forward one UI-facing event. Must never panic, and must not block the
    /// agent loop for more than ~1ms in the common case.
    fn emit(&self, event: DesktopEvent);
}
```
It takes `DesktopEvent` (not `Envelope`): sequencing stays the emitter's job, so
every UI consumer sees gapless `seq` even when events fan out to several emitters.

### Sequencing
`ChannelEmitter` owns an `AtomicU64`; on `emit` it builds
`Envelope { v: SCHEMA_VERSION, seq: counter.fetch_add(1), event }` and forwards it.
`RecordingSink` gets a small companion method so it can share the path:
`RecordingSink::record(&self, event: DesktopEvent) -> Envelope` (assigns `seq`,
stores, returns the envelope); `EventSink::emit` is refactored to call it.

### Composition — one sink for the agent loop
```rust
pub struct BroadcastSink { emitters: Vec<Arc<dyn StreamEmitter>> }

impl EventSink for BroadcastSink {
    fn emit(&self, ev: AgentEvent) {
        let de = DesktopEvent::from(ev);          // convert once
        for e in &self.emitters { e.emit(de.clone()); }
    }
}
```
`run_task_with_sink` keeps its signature (`Arc<dyn EventSink>`): the Tauri layer
passes `BroadcastSink::new().with(recording).with(channel_emitter)` and gets both
the NDJSON log and the live stream with zero engine changes.

### TextDelta batching
Deltas arrive per token; forwarding each over IPC floods the frontend.
`BatchingEmitter` wraps any emitter: it accumulates `TextDelta` text and flushes
when the buffer reaches 200 chars, 50ms elapse, or a non-delta event arrives
(flush-before-other). Non-delta events pass through immediately. Lossless — the
full text always arrives in the terminal `AssistantMessage`; only intermediate
rendering is coalesced.

### Backpressure and disconnect
- Agent task → forwarder: `std::sync::mpsc::sync_channel(512)` of envelopes,
  drained by a dedicated thread into the Tauri `Channel`.
- Full queue: `TextDelta` is merged into the pending batch (drop-and-coalesce);
  any other event blocks up to 100ms, then is logged and dropped. The agent loop
  never deadlocks on the UI.
- `Channel::send` returning `Err` sets an `AtomicBool` disconnected flag; later
  emits become no-ops. The agent run continues unaffected.

## 3. Results

Concrete deliverables for the implementer (Z App, g6):
1. `pub trait StreamEmitter: Send + Sync { fn emit(&self, event: DesktopEvent); }`
   in `engine/src/stream.rs`, plus `ChannelEmitter`, `BatchingEmitter`,
   `BroadcastSink`, and `RecordingSink::record` as sketched above.
2. Wiring: `run_task_with_sink` unchanged; the Tauri command builds the
   `BroadcastSink` per run.

### Open questions for Z App
1. One `Channel` per run vs one global event bus with `run_id` filtering? I
   recommend per-run channels; flag if multi-window needs the bus.
2. Should TextDelta batching live in Rust (`BatchingEmitter`) or purely in the
   frontend (rAF)? Both is harmless — where do you want the 50ms/200-char knobs?
3. `SessionEnd`/cost events (unmerged PR #4) are outside schema v1 — extend this
   design to v1+SessionEnd now, or keep D1 to main's schema and follow up?
