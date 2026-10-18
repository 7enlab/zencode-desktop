# Engine API guide (`zencode_engine`)

> Verified against `origin/main` plus open PRs **#4** (session + `zencode` CLI +
> `SessionEnd`), **#6** (prompt budget), **#8** (pre-tool-use hooks), **#9**
> (read tracking), **#23** (input validation). Field and function names are taken
> from those branches' actual source.

## Overview

The `engine/` crate is a thin assembly layer: it wires the ZenCode CLI's agent loop
(`zen-agent`), file/shell tools (`zen-tools`), and core types (`zen-core`) — all pulled
as git dependencies pinned to one CLI rev — against any OpenAI-compatible endpoint.
No agent core is rewritten here; the crate only assembles the pieces.

Crate layout:

```text
engine/
  src/lib.rs          RunOptions, default_tools(), run_task(), run_task_with_sink()
  src/events.rs       Versioned DesktopEvent schema + RecordingSink (the UI contract)
  src/prompt_budget.rs  (PR #6) g4 token budget: chars/4 estimate + fail-fast guard
  src/hooks.rs          (PR #8) PreToolUseHook trait + ordered HookChain
  src/read_tracking.rs  (PR #9) P3 read-before-write defense-in-depth
  src/bin/zencode.rs    (PR #4) `zencode run` / `zencode eval` CLI
  tests/fake_llm.rs            Integration tests against a scripted localhost LLM
  tests/snapshots/             insta contract snapshot for the event stream
```

Git dependencies (`engine/Cargo.toml`): `zen-core`, `zen-agent`, `zen-tools` from
`https://github.com/atzenn/zencode` at a pinned rev.

## Quick start

```rust
use std::sync::Arc;
use zencode_engine::{events::RecordingSink, run_task_with_sink, RunOptions};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut opts = RunOptions::new(
        "https://ai-gateway.kilo.ai/api/v1", // OpenAI-compatible base URL
        "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
        "/tmp/workdir",                      // cwd the tools operate in
    );
    opts.dangerous_allow_all = true; // opt in: caller sandboxes cwd (e.g. tempdir)
    let sink = Arc::new(RecordingSink::new());
    let result = run_task_with_sink(&opts, "Create hello.txt with: hi", sink.clone()).await?;
    println!("success: {}", result.success);
    println!("{}", sink.to_ndjson()?); // versioned event log, one JSON object per line
    Ok(())
}
```

Notes: `run_task` needs no key for keyless free tiers (`api_key: None`); pass
`opts.api_key = Some(...)` for providers that need a bearer token. Tool approval is
**deny-by-default** — set `dangerous_allow_all = true` only when you sandbox `cwd`.

## Core types

**`RunOptions`** (all fields `pub`):

| Field | Type | Default (`RunOptions::new`) |
|---|---|---|
| `base_url` | `String` | required — non-empty, must start with `http://`/`https://` |
| `api_key` | `Option<String>` | `None` |
| `model` | `String` | required — non-empty |
| `fallback_models` | `Vec<String>` | `vec![]` |
| `cwd` | `PathBuf` | required — must be an existing directory |
| `max_turns` | `usize` | `12` |
| `system_prompt` | `String` | `"You are ZenCode, a concise coding agent. …"` |
| `dangerous_allow_all` (PR #23) | `bool` | `false` — deny all tool calls unless `true` |
| `pre_tool_use_hooks` (PR #8) | `Vec<Arc<dyn PreToolUseHook>>` | `vec![]` |

**`run_task(opts, task) -> anyhow::Result<RunResult>`** — run one task to completion.
Events go to a `NullSink` (discarded). Rejects empty tasks and non-directory `cwd`.

**`run_task_with_sink(opts, task, sink: Arc<RecordingSink>)`** (PR #4) — same, but
streams every agent event into the sink and appends a `DesktopEvent::SessionEnd` with
cost accounting when the turn finishes.

**`default_tools() -> ToolRegistry`** — the CLI's `Read`, `Edit`, `Write`, `Bash` tools.

**`default_tracked_tools() -> (ToolRegistry, ReadTracker)`** (PR #9) — same tools
routed through the read-tracking layer; prefer this for agent runs.

**`events::RecordingSink`** — collects events as versioned envelopes:
`events()` returns `Vec<Envelope>`; `to_ndjson()` returns `anyhow::Result<String>`
(PR #23: no more panics on serialization failure).

**`events::DesktopEvent`** — the UI contract, a stable projection of the CLI's
`AgentEvent` (converted via `From<AgentEvent>`). Variants: `AssistantMessage`,
`ToolResult`, `TextDelta`, `ApiRetry`, `CompactBoundary`, `PermissionDenied`,
`SystemMessage`, `Unknown { variant }`, and `SessionEnd` (PR #4: `success`, `result`,
`num_turns`, `total_cost_usd`, `input_tokens`, `output_tokens`, `model`,
`duration_ms`). Each log line is `{"v":1,"seq":N,"type":"…",…}`.

**Versioning:** `events::SCHEMA_VERSION` is `1`; bump it on any breaking change and
re-review the insta snapshots. `parse_ndjson()` rejects lines with a missing (`v: 0`)
or newer version instead of misreading them — a CLI rev bump can never silently
change what the UI sees; the contract tests fail first.

## New modules

**`prompt_budget` (PR #6, g4).** Keeps the static prompt (system prompt + tool
definitions) under a token budget, using the chars/4 estimate.

```rust
use zencode_engine::prompt_budget::{ensure_within_budget, PROMPT_BUDGET_TOKENS};

// ~1200 token budget (chars/4 heuristic); fail fast before the run starts.
let used = ensure_within_budget(&opts.system_prompt, &tools)?; // Err(OverBudget) if > budget
```

Key items: `estimate_tokens(&str)`, `tool_definition_tokens(&ToolDefinition)`,
`static_prompt_tokens(&str, &ToolRegistry)`, `ensure_within_budget(...) ->
Result<usize, OverBudget>`, `OverBudget { used, budget }`. `run_task_with_sink`
enforces this automatically — a bloated prompt fails before the first turn.

**`hooks` (PR #8, P2).** Engine-native pre-tool-use interception: each tool is
wrapped so every call first passes through your hooks.

```rust
use std::sync::Arc;
use zencode_engine::hooks::{HookChain, PreToolUseHook, PreToolUseDecision};
// impl PreToolUseHook for MyPolicy { async fn before_tool_call(...) -> ... }
let mut chain = HookChain::new();
chain.push(Arc::new(MyPolicy));
opts.pre_tool_use_hooks = vec![Arc::new(MyPolicy)]; // Vec<Arc<dyn PreToolUseHook>>
```

Key items: `PreToolUseHook` (async trait, `before_tool_call(tool_name, args, ctx)`),
`PreToolUseDecision::{Allow, Deny(String), Modify(Value)}`, `NoOpHook` (always allow),
`HookChain::{new, push, len, is_empty, run}` (first `Deny` wins; `Modify` feeds the
next hook), `HookedTool::{new, inner}`, `wrap_with_hooks(registry, &chain)`. The
rate-limit policy itself is intentionally not here — it awaits atzenn/zencode#10.

**`read_tracking` (PR #9, P3).** Defense-in-depth read-before-write at the engine
layer, mirroring the CLI's `ToolContext.read_files` semantics.

```rust
use zencode_engine::read_tracking::ReadTracker;
let (tools, tracker) = zencode_engine::default_tracked_tools();
// tracker.was_read(&cwd, "src/main.rs") -> bool
```

Key items: `ReadTracker::{new, record_read, was_read, len, is_empty}` (thread-safe,
canonicalized paths), `TrackingReadTool::{new, tracker}` (wraps Read), `ReadGuardedTool`
(wraps Edit/Write; refuses with `"has not been read in this session"`), `GuardMode::{Always, IfExists}`
(Write to a *new* file is allowed), `tracked_tools() -> (ToolRegistry, ReadTracker)`,
`NOT_READ_MSG`.

## `zencode` CLI (PR #4, engine/src/bin/zencode.rs)

```bash
zencode run "Create hello.txt with: hi" [--model M] [--cwd DIR] [--max-turns N] [--session-dir DIR]
zencode eval [--model M] [--max-turns N]
```

`run` executes against a `:free` model only (non-`:free` models are refused) and saves
the session NDJSON to `<cwd>/.zencode/sessions/session-<ts>.jsonl`. `eval` runs 10
small file tasks and prints a metrics table. Env overrides: `ZENCODE_BASE_URL`,
`ZENCODE_API_KEY`, `ZENCODE_MODEL`.

## Validation & safety

| Check | Where | Behavior |
|---|---|---|
| Empty `base_url`, or missing `http(s)://` prefix | `run_task_with_sink` (PR #23) | bail before any network call |
| Empty `model` | `run_task_with_sink` (PR #23) | bail |
| Tool approval | PR #23 | `dangerous_allow_all=false` → every call denied with a message naming the opt-in flag |
| Prompt budget (g4) | PR #6 | `ensure_within_budget` fails fast if static prompt > 1200 tokens |
| Read-before-write | PR #9 | `ReadGuardedTool` refuses unwritten reads; `Write` to new files allowed |

## Testing

```bash
cd engine && cargo test
```

`tests/fake_llm.rs` runs the full agent loop (HTTP client → agent → real Read/Edit
tools) against a scripted OpenAI-compatible server on localhost — no network, no key.
The 6 tests cover: Read→Edit through real tools, Bash in `cwd`, empty-task/bad-cwd
rejection, server-error surfacing, the v1 event-stream contract (insta snapshot), and
rejection of newer/missing schema versions.

To update the contract snapshot after an intentional schema change:
`cargo insta review` (or `INSTA_UPDATE=always cargo test`), then commit the `.snap` file.
