# R2: Time-Travel / Undo-Everything UI

## What

A visual, scrubable timeline of every agent action in a session. Each entry is
one plain-language row - "Edited `src/main.rs`", "Ran `cargo test`", "Blocked:
Bash needed your approval" - with the files touched and a short outcome. Any
entry covered by an auto-checkpoint gets **"↩ Undo just this"**; every
checkpoint in the session's shadow repo gets **"⟲ Restore to this point"**.

## Architecture

```
session NDJSON log ──▶ timeline::build_timeline() ──▶ Vec<TimelineEntry>
        │                        │
        │                        ▼ Tauri command `get_timeline`
        │                   ┌──────────────┐
        └──────────────────▶│ timeline.js  │──▶ renders #timeline-panel
                            └──────────────┘
workdir/.zencode/ ──▶ CheckpointManager::list() ──▶ checkpoints section
checkpoints/              │
                          ▼ Tauri command `restore_checkpoint`
                    CheckpointManager::restore() ──▶ safety checkpoint first
```

### Backend (`engine/src/timeline.rs`)

`build_timeline(&[Envelope]) -> Vec<TimelineEntry>`:

- `AssistantMessage.tool_calls` → one entry per tool call. Kind from the tool
  name (`Write`→`write`, `Edit`→`edit`, `Bash`→`bash`, `Read`→`read`).
  File paths parsed from common argument keys
  (`path`, `file`, `file_path`, `filePath`, `filename`).
- `ToolResult` (matched by `tool_use_id`) → status `ok`/`error` + detail from
  the first line of the result content.
- `PermissionDenied` → a `blocked` entry ("the agent did not run this").
- `SystemMessage` with `event = "checkpoint"` → linkage only (see below).

### Checkpoint linkage convention

The v1 event schema carries no checkpoint field, so the engine links them with
a system message:

```text
event = "checkpoint"
text  = "checkpoint <short-sha> before seq <n>"
```

`<n>` is the envelope `seq` of the action the checkpoint was taken for (D5:
auto-checkpoint before every Write/Edit/Bash). The timeline builder parses
these and sets `checkpoint_id` + `undoable` on the matching entry. Malformed
notes are ignored, never fatal.

When PR #31's auto-checkpoint hook lands in the run loop, it should emit this
message right after each snapshot.

### Tauri commands (`desktop/src-tauri/src/main.rs`)

- `get_timeline(session_id)` → `TimelineResponse { session_id, entries,
  checkpoints }`. Reads `<sessions>/<id>.ndjson` via `parse_ndjson`, builds
  entries, and lists the session workspace's shadow-repo checkpoints (workspace
  from the session metadata's `cwd`; missing workspace → empty checkpoint
  list, never an error). Session id is validated against path traversal.
- `restore_checkpoint(session_id, checkpoint_id)` → `RestoreOutcome
  { restored_files, safety_checkpoint_id }`. `CheckpointManager::restore`
  auto-snapshots first, so every restore is itself undoable. Checkpoint ids
  must be 4–40 hex chars.

### Frontend (`desktop/src/timeline.js`)

- **Live mode**: `invoke("get_timeline")` / `invoke("restore_checkpoint")`.
- **Mock mode**: replays `timeline-demo.ndjson` (schema-correct v1 events)
  through a JS port of `build_timeline`, with two demo checkpoints. Undo in
  mock mode is simulated.
- Tabs: `Chat` / `⏪ Timeline` in the chat pane; the timeline lazy-loads when
  its tab is opened and refreshes after a restore.
- Both destructive actions ask for confirmation and explain the safety
  checkpoint in plain language.

## What it does NOT do

- Per-event wall-clock timestamps: the v1 event schema has no timestamp field,
  so entries show "Step N of M". A future schema bump could add them.
- True single-action surgical undo: "undo just this" restores the checkpoint
  taken *before* that action, which also reverts anything the agent did
  between the checkpoint and the action. The UI says "reverts the files via
  checkpoint …", not "reverts only this edit".

## Verification

- `cargo test --lib timeline` - 6 tests (entry building, checkpoint linkage,
  malformed notes, blocked/error states, arg parsing, serialization).
- `cargo clippy --locked --all-targets -- -D warnings` - clean.
- `cargo fmt --check` - clean.
- `node --check` on `timeline.js` and `app.js` - clean.
