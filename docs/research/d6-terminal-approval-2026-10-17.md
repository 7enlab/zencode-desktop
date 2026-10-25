# D6 — Embedded Terminal + Approval UI (design)

*Status: design only, for Z App's g6 (Tauri UI). No implementation.*

## Research

**Terminal stack (matches `docs/desktop-architecture.md` §3):**
- Backend: [`portable-pty`](https://crates.io/crates/portable-pty) (wez) — cross-platform PTY (Unix `fork`+`exec`, Windows ConPTY), the same crate behind WezTerm. Alternatives (`pty-process`, raw `libc`) lack Windows parity.
- Frontend: [xterm.js](https://xtermjs.org) with the **WebGL renderer** (`xterm-addon-webgl`) — required for smooth scroll on fast output; canvas fallback when WebGL unavailable. Addons: `xterm-addon-fit` (resize), `xterm-addon-search`, `xterm-addon-unicode11`.
- Transport: PTY bytes flow backend→frontend over a Tauri `ipc::Channel` (see D2 design); keystrokes flow frontend→backend via a Tauri command. One channel per terminal tab.

**Approval flows in the wild:**
- *Claude Code:* per-tool allow/deny prompt; "always allow" remembers per session; dangerous commands (e.g. `rm -rf /`) are never auto-allowed; permission modes (`default` / `acceptEdits` / `plan`).
- *Cursor:* auto-run allowlist for "safe" commands, ask-mode for the rest; background agents inherit stricter defaults.
- Common lesson: **read-only tools are free, writes ask, destructive never auto-runs.** The allowlist is session-scoped, not global — a malicious prompt can't permanently bless `curl | sh`.

## Ideas

**Terminal component:**
- `TerminalManager` (Rust): spawns one `portable-pty` pair per tab, runs the user's login shell (`$SHELL`, fallback `/bin/bash` / `powershell.exe`), handles resize (`master.resize()`), kills the child on tab close.
- Frontend `TerminalPane` (xterm.js): attaches to the channel, `fit` on container resize, scrollback 10k lines, WebGL renderer with canvas fallback.
- Terminals are **user-driven only** — the agent never types into them. The agent's `Bash` tool runs headless (existing engine behavior); the terminal is for the human to inspect/run things manually.

**Approval UI:**
- Every agent tool call renders an **inline approval card** in the chat stream (tool name, args preview, diff preview for Edit/Write).
- Three tiers:
  1. **Auto-allow** — read-only tools (`Read`, `Glob`, `Grep`, `Bash` with read-only commands like `ls`, `git status`). No card, just a collapsed log line.
  2. **Ask** — `Write`, `Edit`, `Bash` (general). Card with Allow once / Allow always this session / Deny (+ optional edited command).
  3. **Never auto-allow** — destructive patterns (`rm -rf`, `sudo`, `mkfs`, `curl … | sh`, writes outside project root). Card is mandatory; "allow always" is disabled for this tier.
- Session allowlist: `{tool, arg-pattern}` pairs approved via "always this session"; cleared on session end; persisted per-project (not global) only if the user opts in.

**Hook integration (P2 / PR #8):**
- The approval UI *is* the `PreToolUseHook` implementation for interactive sessions: pending tool call → card → user decision → `Allow` / `Deny(reason)` / `Modify(new_args)` fed back into the `HookChain`.
- Headless/`--yes` runs use a non-interactive hook (policy file only) — same trait, different backend.

## Results

**Approval policy schema** (`~/.zencode/approval.json`, v1):
```json
{
  "version": 1,
  "default_tier": "ask",
  "tiers": {
    "auto_allow": ["Read", "Glob", "Grep"],
    "ask": ["Write", "Edit", "Bash"],
    "never_auto": ["Bash:sudo", "Bash:rm -rf", "Bash:curl|sh"]
  },
  "session_allowlist": [
    {"tool": "Bash", "pattern": "cargo test *"}
  ]
}
```
- `never_auto` entries use `tool:arg-pattern` (glob). First match wins: `never_auto` > `session_allowlist` > `ask` > `auto_allow`.

**Wiring sketch:**
```rust
// Backend: one PTY per tab, bytes over a Channel (see D2 Envelope)
struct TerminalTab { pty: Box<dyn MasterPty>, child: Box<dyn Child>, tx: Channel<Vec<u8>> }
// Frontend: xterm.js attaches; onData -> invoke("term_write", {tab_id, data})
```
Approval card → `PreToolUseHook::on_tool_use(tool, args)` → returns `Allow | Deny(reason) | Modify(new_args)`; the agent loop awaits the decision (with a 5-min timeout → Deny).

**Open questions for Z App:**
1. Should the terminal share the approval policy (e.g. warn before pasting a command containing `rm -rf`), or stay fully manual?
2. Session allowlist persistence: per-project opt-in file vs memory-only — which default?
3. Destructive-tier pattern list: ship a built-in denylist, or let the user define everything from empty?
