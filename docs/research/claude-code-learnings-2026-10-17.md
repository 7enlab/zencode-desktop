# Claude Code — Architecture learnings for ZenCode (2026-10-17)

> Sources: the official docs at https://code.claude.com/docs (updated 10/2026) and https://claude.dev.
> Purpose: extract proven patterns to apply to ZenCode (a Rust coding agent, `:free` models only).
> Angle: architecture + working methods, not marketing.

---

## 1. RESEARCH (Research)

### 1.1 Agent loop

- Structure: model call → tool calls → tool results → repeat until stop. Runs headless via `claude -p "<prompt>"` (non-interactive).
- Stop conditions: the model stops calling tools, max turns reached, API error, or `--max-budget-usd` (cost ceiling).
- **Background tasks**: Bash has `run_in_background` — the command runs in the background and Claude gets *notified* when it finishes instead of having to poll. When `-p` ends a turn, the run still waits for background work (default 10 minutes, configurable via `CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS`).
- **SIGTERM**: exits with code 143, records the running command as "killed", runs `SessionEnd` hooks before exiting — nothing is abandoned silently.
- **Bare mode** (`--bare`): skips auto-discovery (hooks, skills, MCP, CLAUDE.md) so CI produces consistent results on every machine.
- *Why it matters for free models:* each turn = one precious request (50 req/day). Background + no polling = no burning requests while waiting.

Sources: https://code.claude.com/docs/en/headless

### 1.2 Built-in tools

- **Bash**: persistent session (cwd is kept between calls, but not variables/aliases/functions); `timeout` defaults to 120,000ms, max 600,000ms (overridable via env `BASH_DEFAULT_TIMEOUT_MS`/`BASH_MAX_TIMEOUT_MS`); `run_in_background`; `description` (5–10 words, shown on the UI); output truncated at 30,000 characters. There is `BashOutput` (polling by `bash_id` + regex filter) and `KillShell`.
- **Edit**: replaces an *exact* string (exact match, not regex). `old_string` **must be unique** in the file (otherwise it fails and asks for more context or `replace_all`); `new_string` must differ from `old_string`. **Read-before-edit is mandatory**: the tool errors if you edit a file that was never Read in the session.
- **Write**: overwrites the whole file; for existing files it also requires a prior Read (anti-clobber); the operation is atomic.
- **Read**: paginated via offset/limit (default 2,000 lines).
- Convention: prefer dedicated tools (Read/Edit/Glob/Grep) over `cat`/`grep` inside Bash.
- *Why it matters:* exact-match + uniqueness is **self-validating** — a mistake fails loudly instead of silently corrupting the wrong place; read-before-write is a cheap guardrail that costs no request.

Sources: https://code.claude.com/docs/en/headless and the Bash/Edit tool references

### 1.3 Skills

- `SKILL.md` + YAML frontmatter (`name`, `description`). **Progressive disclosure**: only the `description` is loaded at startup; the body is loaded only when the skill is invoked — long docs cost almost 0 tokens until needed.
- Locations: enterprise / personal (`~/.claude/skills/`) / project (`.claude/skills/`, committable) / nested (per subdirectory, loaded when Claude touches a file in it) / plugin.
- The model **decides on its own** when to invoke a skill based on its description → descriptions must be short (a warning fires when total descriptions exceed 15,000 tokens).
- Bundled skills: `/doctor` (setup checkup, *checks the context cost of skills/MCP too*), `/code-review`, `/batch`, `/debug`, `/loop`, `/verify`, `/run`.
- `/verify` and `/run` can **write their own recipe back** as a project skill after the first run — next time it just follows the recipe instead of re-discovering.
- *Why it matters:* this is the strongest context-saving pattern — with small-context free models, everything loads "just when needed" instead of "stuffed up front".

Sources: https://code.claude.com/docs/en/skills

### 1.4 Slash commands

- Built-ins: `/init` (creates CLAUDE.md), `/plan` (plan mode), `/compact [focus]`, `/context` (visualizes context as a grid + optimization hints), `/clear`, `/resume`, `/branch` (forks the conversation at the current point), `/fork` (copies to a background session), `/rewind`, `/review`/`/code-review`, `/diff`, `/doctor`, `/debug`, `/model`, `/effort`, `/permissions`, `/mcp`, `/usage`, `/background`, `/batch`, `/btw` (side question, not recorded in history), `/add-dir`, `/cd`.
- Typing a command while Claude is replying gets **queued** to run after the current turn (except a few that run immediately: `/status`, `/tasks`, `/usage`).
- Custom command = a skill (new format) or a markdown file in `.claude/commands/` (legacy).
- *Why it matters:* focused `/compact`, `/context`, and `/rewind` give the user control over context — the scarcest resource.

Sources: https://code.claude.com/docs/en/commands

### 1.5 Hooks

33 events, 3 cadences: per-session (`SessionStart`, `SessionEnd`, `Setup`), per-turn (`UserPromptSubmit`, `Stop`, `StopFailure`), per-tool-call (`PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`, `PermissionRequest`, `PermissionDenied`). Plus: `PreCompact`/`PostCompact`, `PreModelSwitch`/`PostModelSwitch`, `SubagentStart`/`SubagentStop`, `FileChanged`, `Notification`, `Elicitation` (MCP asking for input mid-call).

- Handlers: shell command (receives JSON on stdin), HTTP endpoint, MCP tool, LLM prompt, subagent, or plugin JS function.
- **Matcher**: filters by tool name (`Bash`, `Edit|Write`, regex `mcp__.*`), by command (`rm *`), etc.
- **Decision**: returns `permissionDecision` = allow/deny/ask; `PreToolUse` can **block** a tool call before it runs; `PermissionDenied` supports `hookSpecificOutput.retry` to make the model retry.
- Scopes: user / project / local / managed / plugin / skill / subagent — merged cumulatively across layers.
- *Why it matters:* a blocking PreToolUse = a guardrail that **costs zero model requests**; PostToolUseFailure = structured error handling; PreCompact = a place to inject summarization instructions.

Sources: https://code.claude.com/docs/en/hooks

### 1.6 Subagents

- Each subagent runs in its own **context window**, with its own system prompt, limited toolset, and permissions. Only a summary is returned.
- Built-ins: **Explore** (read-only, searches the codebase), **Plan** (research for plan mode), **General-purpose**, and helpers.
- The model delegates on its own based on the subagent's `description`. **Cost control**: route cheap work to a cheap model (Haiku).
- Linkage: a subagent's messages carry `parent_tool_use_id` = the ID of the tool call that spawned it (builds a nested tree); `--forward-subagent-text` reproduces the full transcript.
- `isolation: worktree` gives a subagent its own copy of the repo.
- *Why it matters:* exploration (dumping files, logs) is the biggest context hog — confining it to a separate context is the largest saving for small models.

Sources: https://code.claude.com/docs/en/sub-agents

### 1.7 Permissions

- Tiered by tool type: read-only (no prompt), Bash (prompts, except built-in read-only commands), file edits (prompts), WebFetch/WebSearch (prompts).
- Rules: `Tool` or `Tool(specifier)` — e.g. `Bash(npm run *)`, `Read(./.env)`. Evaluation order: **deny → ask → allow** (deny always wins).
- **Denying a whole tool** (bare `Bash`) *removes it from the model's context entirely* → both safer and token-cheaper.
- Permission modes: `default`/`manual`, `acceptEdits` (auto-approve file edits), `plan` (read-only, no edits), `auto` (a classifier approves on the user's behalf), `dontAsk`, `bypassPermissions`.
- "Yes, and don't ask again" saves the rule into `.claude/settings.local.json` (repo scope). Replies to the prompt can **carry an attached comment** (Tab) → sent to the model as the refusal reason.
- *Why it matters:* plan mode = free safe exploration; deny-removes-from-context directly shrinks the prompt budget.

Sources: https://code.claude.com/docs/en/permissions

### 1.8 Sessions & checkpoints

- `/resume` (return to an old session), `/branch` (fork the conversation at the current point), `/fork` (copy to a background session), `/clear` (new session, keeps memory).
- `/rewind`: rolls back **both code and conversation** to a checkpoint, or **summarizes part** of the conversation ("Summarize from here" / "Summarize up to here").
- Transcripts are stored as JSONL: each line has `type`, `uuid`, `parentUuid` (conversation tree), `timestamp`, `sessionId`, `message.content` (blocks), `message.usage` (token accounting).
- *Why it matters:* partial-summarize rewind = precise context "surgery"; branch = cheap experimentation.

Sources: https://code.claude.com/docs/en/commands, https://claude-dev.tools/docs/jsonl-format

### 1.9 Output formats (stream-json)

- `text` (default), `json` (result + session metadata + `total_cost_usd`), `stream-json` (real-time NDJSON events).
- Event types: `system`/`init` (first: `session_id`, `model`, `tools`, `mcp_servers` with each server's `status`, `permissionMode`, `cwd`, `claude_code_version`), `assistant` (**one message per content block**, sharing `message.id`), `user` (carries tool results, `tool_use_id` matches `tool_use`), `stream_event` (only with `--include-partial-messages`: wraps raw API events `message_start`/`content_block_start`/`content_block_delta`/`content_block_stop`/`message_delta`/`message_stop`), `result` (last: text, cost, usage, session metadata).
- `parent_tool_use_id`: `null` in the main loop, set when the message comes from a subagent (builds the nested tree); forked skills use the `forked-command-` prefix.
- Documented ordering: partial messages arrive *before* that block's `content_block_stop`; a cut-off stream still gets `message_stop`, but a half-finished tool call = incomplete input.
- *Why it matters:* confirms the earlier review's F2/F3/F8 findings; the "init reports tool/MCP health up front" and "result carries cost accounting" patterns are worth copying for the RateGate UI.

Sources: https://code.claude.com/docs/en/headless, https://code.claude.com/docs/en/agent-sdk/streaming-output

### 1.10 Context management

- **Auto-compact** when nearly full; `/compact [focus]` (summarize with guidance); `/autocompact <tokens>` sets the threshold.
- `/context`: live breakdown by group + optimization hints.
- **What survives compaction**: the system prompt; the root CLAUDE.md **re-read from disk** (not re-summarized); auto memory re-read from disk; the plan-mode plan; **re-read of at most 5 recently edited files** (files over 5,000 tokens keep only a path reference); skill bodies (max 5k per skill, 25k total); background tasks keep running.
- Rules: `/clear` when switching to unrelated work; **push heavy reading to subagents**.
- *Why it matters:* "re-read stable content from disk instead of re-summarizing" = no fidelity loss; cap re-injected skill content.

Sources: https://code.claude.com/docs/en/context-window

### 1.11 MCP

- Transports: `streamable-http` (recommended), SSE, stdio, WebSocket, `sdk` (in-process).
- Tool naming: `mcp__<server>__<tool>`.
- **Discovery cache**: the tool list is cached, **lazy connection** — only connects when the first tool is actually called.
- **Tool search**: MCP tool schemas are *deferred* by default (not stuffed into context).
- Servers in the project's `.mcp.json` need **workspace trust** before being approved.
- **Elicitation**: an MCP server can ask the user for input *mid* tool call.
- *Why it matters:* lazy connect + deferred schema = MCP costs ~0 until used — progressive disclosure again.

Sources: https://code.claude.com/docs/en/mcp

### 1.12 Cost & efficiency

- `/usage`: session cost, **prompt cache statistics** (hit/miss, warm/cold, *miss causes* e.g. "tool definitions changed"), attribution by skill/subagent/MCP server.
- **Prompt caching**: prefix-match; order requests by stability (system prompt → project context → conversation). Things that **break the cache**: changing model, changing effort, connecting/disconnecting an MCP server, denying a whole tool, compacting.
- Token count live next to the spinner; `/insights` exports an HTML session analysis.
- *Why it matters:* designing a "stable prefix" (unchanging system prompt + tool defs across turns) pays off even when the free provider doesn't support caching yet — and it's the right habit for when a caching provider is used later.

Sources: https://code.claude.com/docs/en/costs, https://code.claude.com/docs/en/prompt-caching

---

## 2. IDEAS (Top 10 patterns ZenCode should copy, ranked by value for free-model users)

1. **Progressive disclosure everywhere** — skills load their body only when invoked, MCP schemas are deferred, nested configs load when a file is touched. The strongest context-saving pattern for small models. *(Skills §1.3, MCP §1.11)*
2. **Read-only subagents for exploration** (Explore-style: separate context, limited tools, summary only). File/log dumps never flood the main context. *(§1.6)*
3. **Read-before-write** — Edit/Write refuse files that were never Read in the session. A cheap guardrail against clobbering. *(§1.2)*
4. **Exact-match + uniqueness for edits** — failures are explicit with hints, never silent corruption in the wrong place. *(§1.2)*
5. **Blocking PreToolUse hooks** — guardrails that cost no model requests; PostToolUseFailure for structured error handling. *(§1.5)*
6. **Bash background + notify** (`run_in_background`, env-configurable timeout, BashOutput/KillShell). Long builds burn no turns. *(§1.2)*
7. **Partial-summarize rewind + focused compact** — precise context "surgery" instead of blindly summarizing the whole session. *(§1.8, §1.10)*
8. **Compaction re-reads stable content from disk** (CLAUDE.md, memory, the 5 most recent files) instead of re-summarizing — no fidelity loss. *(§1.10)*
9. **Plan mode + deny-removes-tool-from-context** — safe exploration; a denied tool is dropped from the prompt entirely (shrinks the prompt budget). *(§1.7)*
10. **Init reports tool/MCP health + result carries cost accounting** (`total_cost_usd`, per-model usage) — copy straight into the Desktop event schema; feeds the RateGate UI (D8). *(§1.9)*

---

## 3. RESULTS (Action proposals, mapped to ZenCode's PLAN)

**P1 — Prompt budget via progressive disclosure (g4).**
Defer tool schemas (load on first call), per-directory nested configs. Measure the token floor in CI as g4 already plans, but add a test that "adding 1 tool must not grow the prompt by more than N tokens".

**P2 — PreToolUse hook point in the engine (g3).**
Add a hook point before tools run: allow block/allow without calling the model. This is the foundation for attaching RateGate (check quota before burning a request) and the permission UI later.

**P3 — Claude-style read-tracking + Edit semantics (g2/g5).**
For the existing `str_replace`: add (a) refusal to edit files never Read in the session, (b) keep the uniqueness rule (already there), (c) atomic writes. Add edit-fail rate as a metric in the g5 eval.

**P4 — Cost accounting in the result event (D8).**
The Desktop `result` event gains `total_cost_usd` + `usage { input_tokens, output_tokens }` per model — exactly like Claude's `result`. This is the data that feeds the RateGate UI quota meter.

**P5 — Background Task tool with parent_tool_use_id (D4, post-v0.3.0).**
A lean subagent: run exploration in a separate context, link messages back via `parent_tool_use_id`. Not needed for v0.3.0, but design the event schema now so it's compatible later.

---

## Limitations of this report

- Some tool details (Bash/Edit schemas) come from secondary sources (a mintlify mirror, deepwiki, a leaked system-prompt gist) because the official docs don't publish the full schemas; they were cross-checked across multiple sources.
- Concrete figures (30,000-character output truncation, 120s/600s timeouts) are Claude Code's and may not transfer verbatim to ZenCode — treat them as starting points, tuned by eval.
- Claude Code's docs change continuously (v2.1.x); architectural patterns are more stable than per-version details.
