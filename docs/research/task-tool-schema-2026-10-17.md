# P5 - Background Task tool: schema design (2026-10-17)

> Status: **design only, no implementation**. Target: post-v0.3.0.
> Source: P5 from `claude-code-learnings-2026-10-17.md` (§1.6 subagents, §2 pattern #2).
> Convention: this page designs the contract; implementation PRs come later.

---

## 1. Research

### 1.1 What Claude Code does

- The `Task` tool launches a subagent with its own context window, system prompt, limited toolset, and permissions. Only a summary returns to the parent.
- Built-in types: **Explore** (read-only codebase search), **Plan** (research for plan mode), **General-purpose**.
- The model picks the subagent from its `description`. Cost control: route cheap work to a cheap model.
- Linkage: subagent messages carry `parent_tool_use_id` = the spawning tool call's ID (nested tree).
- Background execution exists for Bash (`run_in_background` + `BashOutput` poll by id + `KillShell`); the same launch/poll/kill shape transfers to Task.

Sources: https://code.claude.com/docs/en/sub-agents (primary); `claude-code-learnings-2026-10-17.md` §1.2, §1.6.

### 1.2 What the CLI already has (verified in source, rev `04b742e`)

| Piece | Location | What it does |
|---|---|---|
| `TaskTool` | `zen-mcp/src/task.rs` | Foreground-only `Task { description, prompt, subagent_type }`; unknown type → error listing valid names |
| `SubAgentExecutor` | `zen-agent/src/subagent.rs` | Runs a subagent: fresh `Agent`, empty history, depth+1, tool allow-list/`disallowedTools`, model `inherit`, skill preloading, `SubagentStart`/`SubagentStop` hooks |
| `AgentDefinition` | `zen-core/src/lib.rs` | name, description, tools, model, system_prompt, disallowed_tools, permission_mode, max_turns, skills |
| `parent_tool_use_id` | `zen-agent/src/subagent.rs` | Every subagent event carries the spawning Task call ID; builds the nested tree |
| Depth guard | `zen-agent/src/subagent.rs` | `max_depth` (default 1); `Task` removed from tool set at max depth - no infinite recursion |
| Background-job registry pattern | `zen-tools/src/jobs.rs` | `Bash { background: true }` → job id; `JobOutput` polls; `JobKill` stops; process-wide registry, 30k-char output cap |

### 1.3 The gap

The CLI's `Task` is **foreground-only**: the parent turn blocks until the subagent finishes. There is no `run_in_background`, no task registry, no `TaskList` / `TaskResult` / `TaskCancel`, and no Desktop-side lifecycle events. The Desktop engine must add all of that; the execution core (`SubAgentExecutor`) is reused as-is.

---

## 2. Ideas

### 2.1 Design principles

1. **Mirror the Bash background shape.** The CLI already taught the model the launch → poll → kill pattern (`Bash`/`BashOutput`/`KillShell`). `Task`/`TaskResult`/`TaskCancel` reuse the same mental model, so the model needs no new concept.
2. **Reuse, don't reimplement.** Background tasks are spawned through the existing `SubAgentRunner`; only the *scheduling* (registry, polling, cancellation) is new.
3. **Free-only by construction.** A `model` override that doesn't end in `:free` is rejected before any request is spent.
4. **Bounded concurrency.** Background subagents each burn requests; an explicit cap protects the daily free-tier budget.
5. **Observable.** Every lifecycle transition emits a `DesktopEvent` so the Desktop UI can render a task list without polling internals.

### 2.2 Why not just foreground Task?

Exploration is the biggest context hog (learnings §1.6, pattern #2). A foreground subagent still blocks the parent turn; a background one lets the parent continue working and collect the result later - the highest "work per request" ratio for free-tier users.

---

## 3. Results - the schema

### 3.1 `Task` tool definition

Name: `Task`. Description: "Delegate a self-contained task to a sub-agent, optionally in the background."

Parameters (`docs/research/task-tool.schema.json` carries the machine-readable version):

| Field | Type | Required | Default | Notes |
|---|---|---|---|---|
| `description` | string | yes | - | Short (3–5 word) label; shown in UI task list. Max 120 chars. |
| `prompt` | string | yes | - | Full, self-contained instruction for the subagent. |
| `subagent_type` | string | no | `"general"` | Enum: `"explore"` (read-only), `"code"` (read+write), `"general"`. Maps to `AgentDefinition` names. |
| `run_in_background` | boolean | no | `false` | `true` → return immediately with `task_id`. |
| `model` | string | no | inherit | Override model for this subagent. **Must end with `:free`**; anything else is rejected before any request is made. Empty or `"inherit"` = parent model. |

`subagent_type` mapping (Desktop-side):

| `subagent_type` | Tools | Permission posture |
|---|---|---|
| `explore` | Read, Glob-equivalent (read-only set) | read-only |
| `code` | Read, Edit, Write, Bash | same as parent session |
| `general` | inherit parent tool set | inherit parent |

### 3.2 Return values

**Foreground** (`run_in_background: false`): blocks; returns the subagent's final text plus accounting.

```json
{
  "task_id": "task-9f3a2c1d",
  "status": "completed",
  "result": "<subagent final text>",
  "num_turns": 4,
  "total_cost_usd": 0.0,
  "input_tokens": 1200,
  "output_tokens": 300
}
```

On subagent error: `status: "failed"`, `result` carries the error text (same convention as CLI `TaskTool::call`).

**Background** (`run_in_background: true`): returns immediately.

```json
{ "task_id": "task-9f3a2c1d", "status": "running" }
```

### 3.3 Task management protocol

Three companion tools, registered alongside `Task` only when background execution is enabled.

**`TaskList`** - no parameters. Returns all known tasks, newest first:

```json
{
  "tasks": [
    {
      "task_id": "task-9f3a2c1d",
      "description": "Explore auth module",
      "subagent_type": "explore",
      "status": "running",
      "started_at_ms": 1792227000000,
      "num_turns": 2
    }
  ]
}
```

**`TaskResult`** - parameters: `{ "task_id": string (required), "wait_ms": integer (optional, default 0, max 600000) }`.

- If the task is still running and `wait_ms > 0`: block up to `wait_ms` for completion (mirrors `JobOutput`).
- If running and `wait_ms == 0`: return `{ task_id, status: "running", partial_result: "<text so far or null>" }`.
- If finished: return the full foreground-style result object (§3.2).

**`TaskCancel`** - parameters: `{ "task_id": string (required) }`. Returns `{ "task_id": "...", "status": "cancelled" }`.

### 3.4 Lifecycle & error cases

`task_id` format: `task-` + 8 lowercase hex chars (e.g. `task-9f3a2c1d`). Unique per engine process.

Statuses: `running` → `completed` | `failed` | `cancelled` (terminal; no transitions out).

| Case | Behavior |
|---|---|
| Unknown `task_id` | Error: `unknown task_id '<id>'. Use TaskList to see active tasks.` |
| Cancel a finished task | Error: `task '<id>' already <status>; nothing to cancel.` |
| `TaskResult` on a cancelled task | Returns the cancelled status with whatever partial result exists. |
| Concurrent background task limit | Default max **4** running. Beyond that the `Task` call fails fast: `background task limit reached (4); wait with TaskResult or cancel with TaskCancel.` |
| `model` without `:free` suffix | Rejected before any request: `model '<m>' is not a :free model; Task requires free-only models.` |
| Unknown `subagent_type` | Error listing valid names (same convention as CLI `TaskTool`). |
| Engine shutdown with running tasks | Best effort: running tasks are cancelled; a `DesktopEvent::TaskUpdate { status: "cancelled", reason: "engine shutdown" }` is emitted per task. |
| Result retention | Finished task records are kept for the session; `TaskList` never returns more than the last 50. |

### 3.5 DesktopEvent additions (for later implementation)

```text
DesktopEvent::TaskUpdate {
    task_id: String,
    description: String,
    status: TaskStatus,        // Running | Completed | Failed | Cancelled
    parent_tool_use_id: Option<String>,
    num_turns: u32,
    total_cost_usd: f64,
}
```

Emitted on: spawn, completion, failure, cancellation. The existing `parent_tool_use_id` convention (§1.2) already links subagent events to the spawning `Task` call; `TaskUpdate` is the summary-level event the task list UI subscribes to.

### 3.6 Mapping: CLI (exists) vs Desktop engine (to add)

| Needed | CLI already has | Desktop engine adds |
|---|---|---|
| Subagent execution | `SubAgentExecutor::run_subagent` | reuse as-is |
| Foreground `Task` | `TaskTool` (zen-mcp) | reuse; add `run_in_background` + `model` params |
| Background scheduling | `jobs.rs` registry *pattern* (Bash only) | `TaskRegistry`: spawn on tokio task, store handle + metadata |
| Poll / list / cancel | `JobOutput` / `JobKill` (Bash only) | `TaskResult` / `TaskList` / `TaskCancel` tools |
| Lifecycle events | `SubagentStart`/`SubagentStop` hook events | `DesktopEvent::TaskUpdate` |
| Free-model guard | - (CLI has no such policy) | `:free` suffix check on `model` override |
| Concurrency cap | `max_depth` (nesting, not concurrency) | `max_background_tasks` (default 4) |

### 3.7 Open questions (for the team)

1. Should background tasks survive the parent turn ending, or are they scoped to the turn that spawned them?
2. Is 4 the right default cap for concurrent background tasks on free-tier rate limits (20 req/min)?
3. Should `subagent_type` stay a fixed 3-value enum, or reference named `AgentDefinition`s like the CLI (extensible, but heavier prompt)?
4. Where should the `:free` enforcement live - engine-only, or pushed down into the CLI's `TaskTool` too?
5. For `TaskResult` with `wait_ms`: long-poll inside the tool call, or should the Desktop UI poll on its own cadence?

---

*Design by ZD Leader, 2026-10-17. No implementation. Awaiting team review before any code is written.*
