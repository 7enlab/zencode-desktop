# docs/ index

Every document in `docs/` (and the security audit in `audit/`), one line each.
**Status:** `main` = merged · `PR #N` = open branch, not yet merged.

## Getting started

| File | What it covers | Status |
|---|---|---|
| `docs/desktop-architecture.md` | Architecture decisions: Tauri 2 shell, CLI integration, UI components, test & release plan (Zen-approved 2026-10-17, Vietnamese) | main |
| `docs/engine-api.md` | Engine API guide: quick start, core types, DesktopEvent v1 schema, testing; verified against main + PRs #4/#6/#8/#9/#23 | PR #29 |
| `docs/troubleshooting.md` | Build/test/model/API/platform/CI problems, each as symptom → cause → fix | PR #27 |

## Architecture & design

| File | What it covers | Status |
|---|---|---|
| `docs/ipc-bridge.md` | Engine→Tauri wire format, BridgeSink, reconnect replay (engine half of the g6 UI) | PR #43 |
| `docs/upstream-proposals.md` | 6 proposals needing CLI-team action: stream-json version, ToolSchemaMode, hooks, RateGate placement, heartbeat, spill-file refs | PR #10 |
| `docs/research/desktop-features-2026-10-17.md` | Desktop GUI proposals D1–D11 (differentiator: RateGate UI) | PR #2 |
| `docs/research/d1-stream-emitter-2026-10-17.md` | D1: StreamEmitter trait for engine→UI event streaming | PR #13 |
| `docs/research/d2-ipc-design-2026-10-17.md` | D2: IPC event flow (Tauri Channel, rAF batching, reconnect) | PR #15 |
| `docs/research/d4-session-design-2026-10-17.md` | D4: session list/search/rename/fork | PR #14 |
| `docs/research/d5-checkpoints-2026-10-17.md` | D5: checkpoint timeline + per-hunk diff (git shadow repo) | PR #20 |
| `docs/research/d3-plugins-updater-2026-10-17.md` | D3: day-one Tauri plugins; updater keypair must exist before first release | PR #18 |
| `docs/research/d6-terminal-approval-2026-10-17.md` | D6: embedded terminal + 3-tier approval UI | PR #19 |
| `docs/research/d7-model-picker-2026-10-17.md` | D7: free-only model picker (registry + engine hard block) | PR #22 |
| `docs/research/d8-rategate-ui-2026-10-17.md` | D8: RateGate UI, policy-agnostic (policy blocked on atzenn/zencode#10) | PR #28 |
| `docs/research/d9-queue-steer-2026-10-17.md` | D9: message queue + steer (FIFO, interrupt, steer at tool-call boundary) | PR #21 |
| `docs/research/d10-notifications-2026-10-17.md` | D10: completion notifications (triggers, payload schema) | PR #25 |
| `docs/research/d11-background-service-2026-10-17.md` | D11: background service (tray icon, headless tasks) | PR #24 |
| `docs/research/task-tool-schema-2026-10-17.md` | P5: background Task tool contract design (plus `task-tool.schema.json`) | PR #7 |

## Research

| File | What it covers | Status |
|---|---|---|
| `docs/research/competitors-2026-10-17.md` | CLI/desktop coding-agent competitors under free-model constraints (bilingual VI/EN) | main |
| `docs/research/claude-code-learnings-2026-10-17.md` | 12 architecture learnings from claude.dev; proposals P1–P5 | PR #4 |
| `docs/research/acp-zed-2026-10-17.md` | Zed Agent Client Protocol research for v0.5.0 | PR #38 |

## Operations

| File | What it covers | Status |
|---|---|---|
| `docs/release-checklist-v0.3.0.md` | v0.3.0 gate (eval ≥5/10 + CI green), PR merge order, blockers, tag steps | PR #42 |
| `docs/benchmarks-2026-10-17.md` | Criterion baseline for `zencode_engine` (measured 2026-10-17) | PR #37 |
| `docs/e2e/README.md` | How to read the sanitized E2E transcripts | PR #11 |
| `docs/e2e/2026-10-17-g2-smoke.md` | g2 smoke run vs a real `:free` model (reconstruction) | PR #11 |
| `docs/e2e/2026-10-17-eval-10-tasks.md` | 10-task eval transcript, 10/10 pass | PR #11 |
| `audit/2026-10-17.md` | Dependency security audit (cargo-audit, 0 advisories) | PR #16 |

## Roadmap to docs

- **New contributors:** read `docs/desktop-architecture.md` → `docs/engine-api.md` → `docs/research/competitors-2026-10-17.md`; when stuck, `docs/troubleshooting.md`.
- **Z App (reviewing PRs):** `docs/release-checklist-v0.3.0.md` for merge order; `docs/engine-api.md` for the API surface; `docs/e2e/` transcripts for run proof.
- **CLI team (atzenn/zencode):** `docs/upstream-proposals.md` (U1–U6), plus issues #10, #19, #20, #21 on atzenn/zencode.
