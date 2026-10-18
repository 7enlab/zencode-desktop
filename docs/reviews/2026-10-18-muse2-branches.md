# Review: all Muse 2 branches (2026-10-18)

Reviewer: Hark 2. Base: `gitlab/main` @ `9548bb4` (after MR !1–!6 and 23 earlier branch merges).
Companion review of the MRs: `docs/reviews/2026-10-18-muse2-mrs.md`.

## Summary

79 `muse2/*` branches on GitLab:

| Verdict | Count | Meaning |
|---|---|---|
| merged | 26 | On `main` (`git merge --no-ff`, original commits kept). 23 earlier today, 3 in this batch. |
| superseded | 13 | Content already on `main` (MR !1–!6 or a newer branch). Nothing to merge. |
| rejected | 3 | Will not be merged (reason below). |
| needs-CLI | 18 | Agent-core behaviour; belongs upstream in `atzenn/zencode`. |
| pending | 19 | Not merged yet: conflicts, size, or a product/policy decision needed. |

### Dependency source fix (this batch)
GitHub is unreachable for this project, so CI could not fetch `zen-core` / `zen-agent` / `zen-tools`.
Commit `build: fetch zencode from GitLab mirror`:
- `engine/Cargo.toml` and `desktop/src-tauri/Cargo.toml` now use `https://gitlab.com/atzenn/zencode.git`, rev `04b742e`.
- `desktop/src-tauri` pinned `zen-core` at `3a09220` while the engine used `04b742e`: two incompatible
  `zen-core` copies (`ForwardingSink` passes a `zen_core::AgentEvent` into the engine's `BridgeSink`).
  Aligned to `04b742e`; the stale desktop `Cargo.lock` was refreshed (also picks up `async-trait` from !4).
- Not built here: the Tauri crate (needs webkit2gtk; not in the CI matrix). `cargo metadata` resolves cleanly.

### Verification (`engine/`, aarch64 Linux, after all merges)
- `cargo fmt --check`: clean
- `cargo clippy --locked --all-targets -- -D warnings`: clean
- `cargo test --locked`: **91 passed, 0 failed** (82 lib + 3 `tests/budget.rs` + 6 `tests/fake_llm.rs`)

## All branches

| Branch | GitHub PR | Verdict | Reason |
|---|---|---|---|
| `muse2/acp-research` | #38 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/claude-learnings` | #4 | merged | Claude Code learnings + snapshot scrub; merged earlier today. |
| `muse2/d1-stream-emitter` | #13 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d10-notifications` | #25 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d11-background-service` | #24 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d2-ipc-design` | #15 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d3-plugins-design` | #18 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d4-session-design` | #14 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d5-checkpoints-design` | #20 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d6-terminal-design` | #19 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d7-model-picker` | #22 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d8-rategate-ui` | #28 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/d9-queue-steer` | #21 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/dep-audit` | #16 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/desktop-proposals` | #2 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/docs-index` | — | merged | docs/ index (batch 3). Status column still says "PR #N" for docs now on main — refresh later. |
| `muse2/e2e-transcripts` | #11 | merged | Sanitized E2E transcripts (docs only), clean merge (batch 3). |
| `muse2/engine-api-update` | — | merged | Engine API guide incl. new modules (docs only), clean merge (batch 3). Unmerged modules are labelled with their PR. |
| `muse2/g2-smoke` | #3 | merged | g2 smoke example; merged earlier today. |
| `muse2/multiplatform` | — | merged | Docs + cross-compile script; merged earlier today. |
| `muse2/p5-task-schema` | #7 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/release-checklist` | #42 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/rev-bump-04b742e` | #1 | merged | CLI rev bump to 04b742e + clippy fix; merged earlier today. |
| `muse2/supply-chain-deny` | — | merged | cargo-deny config + license metadata; merged earlier today. |
| `muse2/troubleshooting-guide` | #27 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/upstream-proposals` | #10 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `muse2/budget-authority` | — | superseded | Landed via MR !1–!6. |
| `muse2/chat-ui` | — | superseded | Code vendored into main by MR !6 (Tauri shell / chat UI / engine wiring). |
| `muse2/convergence-guard` | — | superseded | Landed via MR !1–!6. |
| `muse2/d4-session-index` | — | superseded | Module already on main via MR !6 (session_store.rs). |
| `muse2/d5-checkpoints` | — | superseded | Module already on main via MR !6 (checkpoints.rs, with autocrlf fix). |
| `muse2/engine-api-guide` | #29 | superseded | Older version of docs/engine-api.md; replaced by engine-api-update. |
| `muse2/engine-wireup` | — | superseded | Code vendored into main by MR !6 (Tauri shell / chat UI / engine wiring). |
| `muse2/ipc-bridge` | #43 | superseded | Module already on main via MR !6 (ipc_bridge.rs, with SessionEnd/Unknown arms). |
| `muse2/scope-guard` | — | superseded | Landed via MR !1–!6. |
| `muse2/tauri-shell` | — | superseded | Code vendored into main by MR !6 (Tauri shell / chat UI / engine wiring). |
| `muse2/undo-timeline-ui` | — | superseded | Landed via MR !1–!6. |
| `muse2/verified-claims` | — | superseded | Landed via MR !1–!6. |
| `muse2/zero-cost-firewall` | — | superseded | Landed via MR !1–!6. |
| `muse2/changelog` | — | rejected | Lists unmerged PRs under 0.3.0; CHANGELOG is maintained on main instead. |
| `muse2/merge-helper` | — | rejected | Built on gh/GitHub merge queue; GitHub unavailable and not our workflow. |
| `muse2/ocr-minor-fixes` | — | rejected | Touches frozen core/ (and rebuild/). |
| `muse2/agent-skills` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/approval-flow` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/compaction` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/g3-rategate` | #33 | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/integration-test` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/linux-sandbox` | — | needs-CLI | OS sandbox belongs in zen-tools/CLI; propose upstream. |
| `muse2/macos-sandbox` | — | needs-CLI | OS sandbox belongs in zen-tools/CLI; propose upstream. |
| `muse2/memory-polish` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/p2-pretooluse-hook` | #8 | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/p3-read-tracking` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/parallel-tools` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/plan-mode` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/repo-map` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/smart-router` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/user-learning` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/verify-loop` | — | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `muse2/wasmtime-sandbox` | — | needs-CLI | OS sandbox belongs in zen-tools/CLI; propose upstream. |
| `muse2/windows-sandbox` | — | needs-CLI | OS sandbox belongs in zen-tools/CLI; propose upstream. |
| `muse2/atomic-writes` | — | pending | Conflicts engine/src/lib.rs; atomic file writes likely belong in zen-tools (CLI) — decide, then rebase or upstream. |
| `muse2/cli-v010` | — | pending | Bumps CLI to v0.1.0 (GitLab); conflicts Cargo.toml/lock/events.rs. Rebase onto the new GitLab source as a separate, deliberate rev bump. |
| `muse2/cloudflare-tunnel` | — | pending | Public network exposure (Cloud mode): needs product/security sign-off; conflicts lib.rs. |
| `muse2/config-import` | — | pending | Small, but conflicts engine/src/lib.rs; rebase onto main. |
| `muse2/cost-leaderboard` | — | pending | Depends on zero-cost-benchmark; conflicts lib.rs + Cargo.lock. |
| `muse2/design-to-code` | — | pending | New feature (screenshot → component); conflicts lib.rs; confirm a free vision model exists. |
| `muse2/engine-benchmarks` | #37 | pending | Criterion benches; conflicts lib.rs + Cargo.lock; rebase. |
| `muse2/g4-prompt-budget` | #6 | pending | 12 commits incl. spec-kit scaffold; conflicts lib.rs. Split out the budget; prompt budgeting is agent-core (upstream candidate). |
| `muse2/model-providers` | — | pending | BYOK/paid providers conflict with the zero-cost firewall (cost_firewall.rs conflict); policy decision first. |
| `muse2/notifications` | — | pending | Built on the pre-!6 Tauri shell; conflicts main.rs/index.html/Cargo.*; rebase onto main shell. |
| `muse2/ocr-fixes` | — | pending | Fixes OCR review Major M1–M3; conflicts lib.rs + tests/fake_llm.rs. High priority to rebase. |
| `muse2/rategate-ui-wireup` | — | pending | Pre-!6 shell; conflicts desktop files; rebase. RateGate policy itself is needs-CLI (g3). |
| `muse2/security-sweep` | — | pending | 32 commits / 62 files mixing web app, companion, docs and engine; too large — split into reviewable MRs. |
| `muse2/skills-index` | — | pending | Depends on agent-skills (needs-CLI); conflicts lib.rs + Cargo.lock. |
| `muse2/sovereign-mode` | — | pending | Feature (R5); conflicts lib.rs + Cargo.lock; rebase. |
| `muse2/speckit-init` | — | pending | Clean merge but adds ~5,200 lines of third-party spec-kit scaffold (.specify/, .agent/); owner decision. |
| `muse2/terminal-tab` | — | pending | Pre-!6 shell; conflicts desktop files; rebase. |
| `muse2/zero-cost-benchmark` | — | pending | Conflicts lib.rs + Cargo.lock; rebase (base for cost-leaderboard). |
| `muse2/zero-telemetry` | — | pending | Pre-!6 shell + lib.rs conflicts; rebase. |

## Notes for Muse 2

**Next work, in order**
1. Rebase `ocr-fixes` onto `main` (fixes OCR Major findings M1–M3).
2. Rebase the desktop branches (`notifications`, `terminal-tab`, `rategate-ui-wireup`, `zero-telemetry`)
   onto the Tauri shell that is now on `main` (from MR !6). Do not re-vendor shell files.
3. Rebase `zero-cost-benchmark` → `cost-leaderboard`, `engine-benchmarks`, `config-import`, `sovereign-mode`.
   Every one of them only conflicts in `engine/src/lib.rs` (+ `Cargo.lock`).
4. Split `security-sweep` and `g4-prompt-budget` into small, single-purpose MRs.
5. `cli-v010`: redo as one deliberate CLI rev bump on the GitLab source.
6. Needs a decision from Zen first: `model-providers` (paid providers vs zero-cost firewall),
   `cloudflare-tunnel` (public exposure), `speckit-init` (third-party scaffold), `design-to-code`.
7. Refresh the status column in `docs/README.md` (many docs listed as "PR #N" are now on `main`).
   `docs/engine-api.md` still names the GitHub URL for the CLI dependency.

**Rules**
- At most 5 new MRs and 5 issues per day, at most 20 pushes per day, per account (Muse 2 and Hark share
  the budget). Writes are sequential; on 403/429 stop all writes for at least 1 hour.
- One MR per milestone; rebase onto `main` before opening; run fmt + clippy `-D warnings` + tests locally.
- Everything in the repo is in English (code, docs, commit messages, MR text).
- Agent-core behaviour (loop, tools, compaction, sandbox, hooks, rate gate, memory, routing) is proposed
  upstream to the CLI `atzenn/zencode`, not reimplemented in Desktop. `core/` stays frozen.
- Dependencies come from `https://gitlab.com/atzenn/zencode.git` at one pinned rev for every crate.

**When GitHub comes back**
- Push `main`: PRs whose commits are on `main` close as merged automatically.
- Close rejected / needs-CLI / superseded PRs manually, at most 10 per day, each with a one-line pointer
  to this document. Use half the normal limits during the first week.
