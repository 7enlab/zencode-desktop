# D2 — IPC event flow: engine → Tauri frontend (design)

Design for the IPC bridge carrying live engine events to the Desktop UI (Tauri 2, g6).
Docs-only; no implementation. Companion to D1 (engine-side `StreamEmitter`, PR #13);
this doc starts where D1 ends — at the Tauri `Channel` boundary.

## 1. Research

### Tauri 2 Rust→frontend streaming options

| Mechanism | Scoping | Ordering | Backpressure | Fit |
|---|---|---|---|---|
| `tauri::ipc::Channel<T>` | one channel per command call | ordered per channel | `send` fails only on disconnect | ✅ one agent run = one channel |
| `app.emit("evt", payload)` | global broadcast, all windows | ordered per emitter | none | needs `run_id` filtering in payload |
| `emit_to` / `Window::emit` | per-window broadcast | ordered per emitter | none | same filtering problem as `emit` |

`Channel` is the right primitive: it is created in JS, passed into the Rust command,
and `channel.send(value)` pushes to the frontend. A `send` error is the disconnect
signal. Global `emit` would force every window to receive every run's events and
filter by `run_id` — workable, but strictly worse than one channel per run.

### Why the frontend still needs batching

D1's `BatchingEmitter` coalesces `TextDelta` in Rust (200 chars / 50 ms). That is not
enough on its own: a burst of coalesced messages still triggers one JS callback per
message, and touching the DOM per callback causes layout thrash. The frontend line
of defense is `requestAnimationFrame`: collect pending envelopes, apply them to the
store in a single paint. Two independent knobs (Rust 50 ms, rAF 16 ms) is defense in
depth, not duplication.

### Gap detection

D1's `Envelope { v, seq, event }` already carries a per-run sequence number. The
webview IPC bridge delivers channel messages in order, so a gap should never happen —
but the check is one integer comparison and turns a silent corruption into a loud,
debuggable warning.

## 2. Ideas

### One `Channel<Envelope>` per run

The JS side creates the channel and invokes the command; the channel callback closes
over the `run_id`, so the envelope itself needs no `run_id` field — D1's wire format
(`{v, seq, type, ...}`) is unchanged, and the NDJSON log stays the single source of
truth for persistence.

### Frontend pipeline

`onmessage` → validate `v` (reject `> SCHEMA_VERSION`, same rule as `parse_ndjson`) →
check `seq` against `expectedSeq` (warn on gap, keep going) → push to `pending` →
schedule one `requestAnimationFrame` flush → `store.applyBatch` (single DOM update) →
append the batch to the persisted NDJSON (for resume).

### Reconnect / resume

If the window reloads mid-run, the `Channel` is dead but the run continues (D1's
disconnect policy: later emits become no-ops). On reload the UI replays the persisted
NDJSON log and marks the stream *detached*. A tiny additive Tauri command closes the
gap with no engine change:

```rust
#[tauri::command]
fn get_log_since(run_id: String, since_seq: u64) -> Result<Vec<Envelope>, String>
```

The UI polls it in "follow" mode until the run's `SessionEnd` (schema v1+…) arrives.

## 3. Results

### Envelope JSON (wire format, schema v1)

```json
{"v":1,"seq":42,"type":"text_delta","text":"Hello, "}
{"v":1,"seq":43,"type":"assistant_message","text":"Done.","tool_calls":[{"id":"tu_1","name":"bash","arguments":"{\"cmd\":\"ls\"}"}],"parent_tool_use_id":null}
{"v":1,"seq":44,"type":"tool_result","tool_use_id":"tu_1","content":"file1\nfile2","is_error":false,"parent_tool_use_id":null}
```

### Wiring sketch (pseudo-code, not compiled)

```rust
// Rust — Tauri command (one channel per run, D1's BroadcastSink plugs in unchanged)
#[tauri::command]
async fn run_agent(prompt: String, on_event: Channel<Envelope>) -> Result<RunSummary, String> {
    let recording = Arc::new(RecordingSink::new());
    let live = Arc::new(BatchingEmitter::new(ChannelEmitter::new(on_event)));
    let sink = Arc::new(BroadcastSink::new().with(recording.clone()).with(live));
    run_task_with_sink(RunOptions::default(), &prompt, sink).await.map_err(|e| e.to_string())?;
    Ok(RunSummary { ndjson: recording.to_ndjson() })
}
```

```ts
// Frontend — rAF-batched consumer with gap detection
import { Channel, invoke } from '@tauri-apps/api/core';

const onEvent = new Channel<Envelope>();
let expectedSeq = 0, rafScheduled = false;
const pending: Envelope[] = [];

onEvent.onmessage = (env) => {
  if (env.v > SCHEMA_VERSION) return;                       // same rule as parse_ndjson
  if (env.seq !== expectedSeq) console.warn('event gap', expectedSeq, '->', env.seq);
  expectedSeq = env.seq + 1;
  pending.push(env);
  if (!rafScheduled) { rafScheduled = true; requestAnimationFrame(flush); }
};

function flush() {
  rafScheduled = false;
  const batch = pending.splice(0);
  store.applyBatch(batch);   // one DOM update per frame
  appendNdjson(batch);       // persist for resume
}

await invoke('run_agent', { prompt, onEvent });
```

### Open questions for Hark 2

1. Large payloads: a `ToolResult` can be megabytes (spill files). Send whole over the
   channel, or truncate on the wire with a "view full" pointer into the NDJSON log?
2. Second window mid-run: attach a second `ChannelEmitter` to the in-flight
   `BroadcastSink` (D1 supports adding emitters), or replay NDJSON + follow mode?
3. Batching knobs: keep both Rust `BatchingEmitter` (50 ms/200 chars) and frontend
   rAF, or pick one side? Where should the tuning live?
