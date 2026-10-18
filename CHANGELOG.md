# Changelog
All notable changes follow [Keep a Changelog](https://keepachangelog.com/) and [SemVer](https://semver.org/).

## [Unreleased]

### Added (Muse 2 MRs !1–!6, merged 2026-10-18; review: docs/reviews/2026-10-18-muse2-mrs.md)
- engine: `cost_firewall` (R1) — rejects non-`:free`, non-allowlisted models before any network call; `SpendMeter`.
- engine: `claims` (R4) — Verified Claims registry, `ClaimingSink`, `run_task_with_claims`; `DesktopEvent::SessionEnd` with claim counts.
- engine: `convergence` (R6) — loop detection and forced re-plan guard (not yet wired into runs).
- engine: `budget` (R12) — opt-in per-task hard budget (`RunOptions.budget`); adds `async-trait`.
- engine: `scope` (R3) — opt-in declared-scope guard (`RunOptions.scope`) + `desktop/src/scope.js` approval card.
- desktop: R2 time-travel timeline — `checkpoints`, `session_store`, `ipc_bridge`, `timeline` modules and the Tauri 2 shell (`desktop/src-tauri`, `desktop/src`).

### Fixed
- engine: budget checker now wraps the scope guard instead of being shadowed by it; `SessionEnd` handled in IPC; CI-safe `Instant` in a budget test; checkpoint shadow repo keeps CRLF bytes (`core.autocrlf=false`).

### Added
- engine: `events` — versioned (v1) UI event schema, `RecordingSink`, `run_task_with_sink`, NDJSON parser rejecting unknown versions; insta contract snapshot.

### Added
- `engine/` (zencode_engine): `run_task()` chạy vòng agent của ZenCode CLI (zen-agent @3a09220) với Read/Edit/Write/Bash; 4 integration test với server OpenAI-compatible giả trên localhost (g1).

### Added
- core: `JsonlLog` (append-only JSONL, chịu dòng cuối bị cắt, cắt đuôi hỏng) và `dedupe_instructions` (AGENTS.md/CLAUDE.md trùng nội dung chỉ inject 1 lần).
- New std-only crate `core/` (`zencode_core` 0.1.0): agent seams `Llm`, `Tool`, `Sandbox` (fail-closed `NoSandbox`, honest `Isolation`), `SessionLog` (append-only `MemoryLog` with `replay_context` and `fork`) and a minimal loop `run_turn`; invariant "everything the model sees is logged". 6 integration tests.
- CI lint and test jobs now run a crate matrix (`rebuild`, `core`).
- `PLAN.md` roadmap for the agent core.
- `--cow-color aurora`: diagonal hue gradient, 24-bit when `COLORTERM=truecolor|24bit`, else xterm 256-colour.
- `--cow-color <red|green|yellow|blue|magenta|cyan|white|gray|bold|rainbow>` and `--color=auto|always|never` (ZenCode extension). Default `auto`: colour only on a terminal, honours `NO_COLOR` and `CLICOLOR_FORCE`; piped output stays byte-identical to cowsay@1.6.0. Windows enables VT processing via `SetConsoleMode` (std-only FFI).
- Commit `Cargo.lock`; CI builds/tests with `--locked`.
- New CI lint job: `cargo fmt --check` + `cargo clippy -D warnings` for Linux, Windows and macOS targets.
- Code formatted with rustfmt; clippy fixes (`is_some_and` / `is_none_or`).

## [0.2.0] - 2026-10-16
### Added
- `-W auto` now falls back to the real terminal width (Unix `ioctl(TIOCGWINSZ)`, Windows `GetConsoleScreenBufferInfo`), std-only FFI.
- Release workflow: tag `v*` → test + build binaries for Linux x86_64, macOS arm64, Windows x86_64 → GitHub Release.

## [0.1.0] - 2026-10-16
### Added
- Std-only Rust rebuild of npm `cowsay@1.6.0` (byte-for-byte default output, 21/21 golden cases).
- Library API `cowsay_bubble::say(text, &Options)`, `Wrap::{None, Hard, Words}`, `auto_width()`.
- CLI: `-W N|auto`, `-n`, `--word-wrap`, 190 embedded cowfiles, `-f`, `-r`, `-l`, modes `-bdgpstwy`, `-e`, `-T`, `--think`/`cowthink`.
- CI matrix Ubuntu/macOS/Windows.
