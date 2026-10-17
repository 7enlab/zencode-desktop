# Changelog
All notable changes follow [Keep a Changelog](https://keepachangelog.com/) and [SemVer](https://semver.org/).

## [Unreleased]
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
