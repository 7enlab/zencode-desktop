# Upstream proposals for ZenCode CLI (atzenn/zencode)

## Research

The Desktop engine consumes the CLI's crates (`zen-agent`, `zen-tools`, `zen-core`) as pinned git dependencies and, in headless mode, the `zen -p ... --output-format stream-json` contract. While building the engine (PRs #1–#9) and reviewing the stream-json contract, Muse 2 found several issues that can only be fixed in the CLI repo. This file collects them; each is actionable by the CLI team. Desktop-side workarounds are noted where they exist.

## Ideas (proposals)

### U1. Version field in stream-json output

- **Problem:** Event-log session files carry `"v":1` on every record, but `stream-json` stdout lines have no version field. A Desktop client cannot detect a breaking schema change except by failing to parse.
- **Proposal:** Add `"v":1` to every stream-json line (same convention as the event log). Bump on breaking changes and note it in the CLI CHANGELOG.
- **Status:** Open. (CLI PR #4 made `AgentEvent` `#[non_exhaustive]` but did not add the version field.)
- **Desktop workaround:** `engine/src/events.rs` validates the Desktop-side event schema (v1) with insta contract tests, but it cannot detect CLI-side drift.

### U2. ToolSchemaMode in AgentConfig (progressive disclosure)

- **Problem:** The engine can measure and enforce a prompt budget (g4: ≤1,200 tokens, `engine/src/prompt_budget.rs`), but truly lazy tool schemas — names + one-liners first, full JSON schemas on demand — need cooperation inside the `zen-agent` turn loop, which the engine cannot inject.
- **Proposal:** Add `ToolSchemaMode { Full, Minimal }` to `AgentConfig`. In `Minimal` mode the agent sends compact tool definitions and fetches full schemas through a lightweight internal mechanism before each tool's first use.
- **Status:** Proposed (design only; no CLI change drafted).
- **Desktop workaround:** Budget measurement + `ensure_within_budget()` fail-fast guard in `run_task_with_sink`; currently ~782/1200 tokens with the 4 default tools.

### U3. Align on PreToolUse hook usage

- **Problem:** `zen-agent` already fires `HookEvent::PreToolUse` before every tool call, but the Desktop engine hardcodes `NoHooks`, so the event is dead weight on our side.
- **Proposal:** None required from the CLI — this is a note. If the CLI team adds hook events or changes `HookRunner` semantics, please announce it; it affects Desktop's hook chain.
- **Status:** Noted. Desktop now has its own type-safe `PreToolUseHook` (`engine/src/hooks.rs`, PR #8: Allow/Deny/Modify + ordered chain) wired through `RunOptions::pre_tool_use_hooks`.
- **Desktop workaround:** N/A — implemented engine-side.

### U4. RateGate / token-budget placement

- **Problem:** Unclear whether per-provider daily quota + model fallback should live in the CLI or in Desktop.
- **Proposal:** See atzenn/zencode#10 ("[desktop] RateGate + token budget: should they live in CLI or Desktop?").
- **Status:** Open, awaiting CLI-team decision. No duplicate filed here.
- **Desktop workaround:** P2 hook infrastructure (PR #8) is ready to host the policy once its placement is decided.

### U5. Heartbeat during long tool runs

- **Problem:** `stream-json` emits nothing while a tool call (e.g. a long Bash command) runs. A Desktop UI cannot distinguish "working" from "hung" and cannot show progress.
- **Proposal:** Emit a lightweight heartbeat line (e.g. `{"type":"heartbeat","v":1,"tool_use_id":"..."}`) every N seconds while a tool call is in flight.
- **Status:** Open.
- **Desktop workaround:** None — the UI currently shows a spinner with no progress signal.

### U6. Structured spill-file reference

- **Problem:** When tool output exceeds the char limit, the CLI embeds the spill-file path in prose ("Full output saved to \<scratch\>/tool-output/\<id\>.txt"). Desktop must regex prose to offer a "view full output" button.
- **Proposal:** Add a structured field to the tool-result line, e.g. `"spill_file": "<path>"` (null when absent), alongside the prose.
- **Status:** Open.
- **Desktop workaround:** Regex extraction (fragile).

## Results — what Desktop does meanwhile

- All Desktop-side mitigations above are implemented on `muse2/*` branches and proposed as PRs; none touch CLI code.
- When the CLI team accepts a proposal, Desktop adopts it and drops the workaround; until then the workarounds stay.
- New findings go here first; once filed upstream, they should become issues in atzenn/zencode with the `desktop` label.
