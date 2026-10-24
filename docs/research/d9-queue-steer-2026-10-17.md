# D9 — Message Queue + Steer (design)

**Date:** 2026-10-17 · **Status:** design only, for Z App (g6) · **Author:** ZD Leader

## Research

**What exists today.** Claude Code: typing while the agent runs queues the message; it is submitted as the next user turn after the current one finishes. `Esc` interrupts the in-flight response (partial work is discarded). Cursor: a stop button interrupts; follow-up input typed mid-run is queued similarly. OpenAI Codex CLI (rust-v0.163.0): supports steering mid-response — guidance injected without killing the run.

**Trade-offs.**
- *Interrupt* (Esc): immediate, but wastes every token spent on the partial turn, including tool calls already executed.
- *Queue*: preserves work, but the user waits for the full turn before their correction lands — bad when the agent is visibly going the wrong way.
- *Steer*: middle ground — inject guidance at the next tool-call boundary. The in-flight LLM call and current tool finish; the agent sees the new instruction at its next decision point. No token waste, near-immediate effect.

**Engine reality on `main` (verified 2026-10-17).** `run_task`/`run_task_with_sink` (`engine/src/lib.rs`) are strictly single-turn: they call `agent.run_turn(task)` once and return `RunResult`. There is no cancellation token, no inbound message channel, and no multi-turn loop. Queue + steer therefore need new engine support — this design proposes exactly that surface.

## Ideas

**1. Input queue (FIFO).** While a run is in flight, the UI keeps the input box enabled. Every submitted message becomes a `QueueEntry` appended to a per-run FIFO. When the current turn completes, the engine drains the queue: the first entry becomes the next turn's user message (multi-turn continuation). The UI shows queued items as "queued" chips the user can remove before they are consumed.

**2. Steer (inject at tool boundary).** A steer is a user message flagged `kind: "steer"`. Instead of waiting for turn end, the engine injects it into the conversation at the next tool-call boundary — i.e., after the current tool result returns, before the next LLM call, the steer text is appended as a user message. The model treats it as mid-course guidance ("actually, prefer X"), not a new task. The in-flight tool is never killed.

**3. Interrupt (Esc / stop).** Cancels the current turn. Implementation: a shared `AtomicBool` cancellation flag checked by the engine between tool calls and before each LLM call; the in-flight HTTP request is dropped. Queued entries survive the interrupt and are offered as the next turn's input ("interrupted — send queued message as new turn?"). Tool calls already executed are not rolled back (that is D5 checkpoints' job).

**Engine support needed (new API, not on main today).**
```rust
// sketch — proposed, not implemented
pub struct RunHandle { /* … */ }
pub async fn run_loop(
    opts: &RunOptions,
    inbound: mpsc::UnboundedReceiver<Inbound>, // UserMsg | Steer | Interrupt
    sink: Arc<dyn EventSink>,
) -> anyhow::Result<RunResult>
```
`Inbound` is drained at every tool boundary and turn boundary; `Interrupt` sets the cancellation flag. `run_task` stays as the single-turn convenience wrapper.

**Ordering guarantee:** within one run, the engine processes inbound messages in arrival order; a steer never overtakes an earlier queued user message.

## Results

**Schemas (JSON, v1 envelope-compatible).**
```json
{ "kind": "queue_entry", "id": "q_01", "text": "…", "enqueued_at_ms": 1792230200000 }
{ "kind": "steer",       "id": "s_01", "text": "…", "injected_at_boundary": "tool_result" }
{ "kind": "interrupt",   "id": "i_01", "reason": "user_esc" }
```

**Operation specs.**
- `enqueue(text)` — appends a `queue_entry`; returns its id. Allowed any time a run is in flight. UI may cap the queue (suggested: 10) and must allow removal before consumption.
- `steer(text)` — appends a `steer`; the engine injects it as a user message at the next tool-result boundary. If no tool is running (pure LLM streaming), it is injected before the next turn instead — never mid-token-stream.
- `interrupt(reason)` — sets the cancellation flag; the current turn ends at the next boundary with `SessionEnd{success:false, interrupted:true}` (new field, additive). Queued entries are preserved; the UI asks whether to start a new turn with the first queued message.

**Open questions for Z App.**
1. Should a steer also be visible in the transcript as a distinct `user_steer` event, or as a plain user message with a flag?
2. Interrupt during a destructive tool (Write/Edit/Bash): finish the tool or kill it mid-write? (Killing risks half-written files — D5 checkpoints mitigate.)
3. Queue cap and overflow policy: reject new input, or drop the oldest queued entry?
