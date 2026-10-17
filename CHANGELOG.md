# Changelog
All notable changes follow [Keep a Changelog](https://keepachangelog.com/) and [SemVer](https://semver.org/).

## [Unreleased]
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
