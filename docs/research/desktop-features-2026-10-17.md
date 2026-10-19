# ZenCode Desktop — Desktop-layer feature proposals (2026-10-17)

> Follows up on `docs/research/competitors-2026-10-17.md` (CLI/engine focus, 12 features).
> This doc covers the **desktop GUI layer only**: what a Tauri 2 shell around the engine should do,
> based on two new studies done today. Method: web research, primary sources preferred, marked below.
> Author: Muse 2 (side branch, awaiting Hark 2's review).

---

## 1. Research

### 1.1 Desktop GUI competitors (6 apps surveyed)

| App | Shell architecture | Proven pattern for us |
|---|---|---|
| **Cline Desktop** (beta, ~09/2026) | Tauri + Bun + Next.js; shares one agent harness with its VS Code extension and CLI — desktop is just another client; sessions carry over CLI↔desktop | Thin shell over shared core (our `engine/` direction is correct) |
| **Goose desktop** (Block) | Electron + React; each window spawns a Rust `goosed` server, REST + SSE; moving to **ACP** as the single protocol for all clients in 2.0 | ACP as the long-term client/server split (like LSP) |
| **Kilo Desktop** (beta, 06/10/2026) | Tab-based; spawns a local Kilo CLI and drives it through chat — literally a shell over the same engine as VS Code/CLI | Shell-over-engine, again |
| **Zed agent panel** | Native Rust; **ACP** (JSON-RPC over stdio) separates editor from agent; parallel threads, worktree isolation, message queue + steer | ACP precedent; queue/steer UX |
| **Cursor** | VS Code fork; agents window with tabs, checkpoints, per-hunk diff review, 3-mode terminal approval, queued follow-ups | Strongest usage/quota UI (`/usage` meters, billing reset date) |
| **OpenCode 2.0** (~12/10/2026) | **Tauri v2** desktop (Rust backend + SolidJS) + Electron variant; one background server process all clients connect to; tasks survive closing the UI | Background service: work outlives the window |

**Converged standard** (every serious app has these): session list + search + fork, checkpoints/restore, per-hunk diff accept/reject, embedded terminal with approval UI, model picker, token/context meter, message queue while busy, completion notifications.

**Market gap (no app does this well): none of the six has offline request queueing or automatic model fallback on rate-limit in the UI.** They all show cost *after* spending (dashboards/dialogs); none budgets *before* spending. This is exactly where ZenCode's RateGate + ModelChain (g3) can differentiate — if the UI is done right (quota meter, queue, auto-switch).

### 1.2 Tauri 2 patterns for agent apps (2026)

- **Plugins (official, current)**: updater, dialog, fs (scoped), shell (scoped), store, log, window-state, single-instance, deep-link (`zencode://`), process, notification, opener, stronghold (encrypted API keys). Do NOT call LLM APIs from the webview (plugin `http`) — keys stay in the Rust backend.
- **Updater from the first release.** Real-world lesson (dev.to/0xMassi, 2026): shipping the updater late stranded 100+ early users on the old version. Generate the minisign keypair on day one; private key → CI secret.
- **Token streaming via `tauri::ipc::Channel`**, not one event per chunk; frontend batches DOM updates with `requestAnimationFrame`; one `request_id`/`session_id` per stream (PolyUI pattern, MIT).
- **Terminal = `portable-pty` (Rust) + xterm.js with the WebGL addon**; batch PTY output (~16ms ticks); cap scrollback (~5000 lines). Reference: NovaTerm (Apache 2.0, <10MB, same stack).
- **Engine stays Tauri-agnostic**: define a `StreamEmitter` trait in `engine/`; the Tauri adapter implements it, tests use a spy. Agent logic never knows about Tauri.
- **Least-privilege capabilities**: `fs` scoped to the chosen project dir, `shell` scoped to allowed binaries.
- **Biggest 2026 risk is WebKitGTK on Linux** (Ollama ships no Linux GUI at all because of it). Develop primarily on Windows (WebView2) + macOS; budget explicit test time on Ubuntu 22.04 *and* 24.04; avoid bleeding-edge web APIs.
- **CI**: tauri-action matrix from the start (macOS 2 archs, ubuntu-22.04, windows-latest), draft releases, auto-generated `latest.json`. OS code signing later (Apple $99/yr, Azure Trusted Signing) — ship unsigned first, but never ship without the updater.

---

## 2. Ideas — desktop-layer proposals (D1–D11)

| # | Proposal | Proven by | PLAN mapping | Effort |
|---|---|---|---|---|
| D1 | **Thin-shell architecture**: `engine/` stays pure Rust; Tauri commands are thin adapters over a `StreamEmitter` trait | Cline, Kilo, OpenCode 2.0 | step 2 (Tauri shell) | M |
| D2 | **Token streaming via IPC Channel + rAF batching**; typed events (`agent-delta`, `tool-call`, `tool-result`, `session-end`) each carrying `session_id` | PolyUI, Zed | step 2 | S–M |
| D3 | **Day-one plugin set + updater keypair** (updater, dialog, fs, shell, store, log, window-state, single-instance, deep-link, process, notification, opener, stronghold); least-privilege `capabilities/` | Tauri docs; 0xMassi lesson | step 2 / CI | S |
| D4 | **Session list + search + rename + fork-from-message** (fork = cheap checkpoint without file snapshots; resume/fork from the v1 event log) | Goose, Zed, Cline | step 3 | M |
| D5 | **Checkpoints timeline + Restore with preview; per-hunk diff accept/reject** (`similar` crate for hunks, CodeMirror 6 for view) | Cursor, Zed (`shift-ctrl-r`), Cline | steps 3–4 | M–L |
| D6 | **Embedded terminal** (portable-pty + xterm.js WebGL) **with approval UI** per tool: Allow / Ask / Deny (maps to the CLI's existing permissions) | Cursor (3 run modes), Goose (4 modes) | step 5 | M–L |
| D7 | **Free-only model picker**: lists only `:free` models, tags provider, shows context window | Cline (FREE tag), Kilo | step 2 | S |
| D8 | **RateGate UI — the differentiator**: daily quota meter per provider, offline queue when exhausted ("run when quota returns"), automatic ModelChain fallback with visible switch reason | **No competitor** | g3 | M |
| D9 | **Message queue + steer while busy** (queue follow-ups, reorder, steer mid-generation) | Cursor, Zed | step 2 polish | S–M |
| D10 | **Completion notifications** (OS badge + sound) for background/long tasks | Cursor, Cline | step 2 polish | S |
| D11 | **Background service** (post-v0.3.0): one local server owns sessions; UI is a detachable client; tasks survive closing the window | OpenCode 2.0 | after v0.3.0 | L |

Out of scope for v0.3.0 (noted for later): share-session links, slash commands/@-mentions, scheduled tasks/automations, quick launcher, multi-session worktree isolation, ACP server for third-party editors.

---

## 3. Results — what changes in the PLAN

1. **Architecture doc**: adopt D1–D3 as the Tauri shell's founding decisions (thin adapters, Channel streaming, day-one updater). The v0.3 step-2 ("Tauri 2 shell, one chat session via engine") is the vehicle.
2. **g3 scope grows a UI half**: RateGate is not just engine middleware — D8 (meter + queue + auto-fallback UI) is what makes it a differentiator. Engine and UI should be designed together.
3. **Ordering**: D4 (sessions) → D5 (checkpoints/diff) → D6 (terminal) matches existing steps 3–5; D7–D10 are step-2 polish items to schedule right after the first chat shell works.
4. **Work split** (see Hub page "Muse 2 — ZenCode Desktop work split 2026-10-17"):
   - *Muse 2 (side branches, Hark 2 reviews)*: this proposal; Tauri shell scaffold (D1–D3); engine g2 (real `:free` run).
   - *Hark 2 (when quota returns)*: review; `rebuild/` → zencode-cowsay move; continue steps 3–5.
   - *CLI team*: stream-json version (F1); RateGate/token-budget placement (issue #10).

## Limitations
- Several release details come from secondary sources (marked in the survey notes); verify against primary changelogs before quoting publicly.
- Cline Desktop's Tauri usage is reported by secondary sources only.
- Effort estimates are relative (S ≤ 1 day, M 2–4 days, L ≥ 1 week) and unvalidated.
