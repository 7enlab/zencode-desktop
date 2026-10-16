# Changelog
All notable changes follow [Keep a Changelog](https://keepachangelog.com/) and [SemVer](https://semver.org/).

## [Unreleased]
### Added
- `-W auto` now falls back to the real terminal width (Unix `ioctl(TIOCGWINSZ)`, Windows `GetConsoleScreenBufferInfo`), std-only FFI.
- Release workflow: tag `v*` → test + build binaries for Linux x86_64, macOS arm64, Windows x86_64 → GitHub Release.

## [0.1.0] - 2026-10-16
### Added
- Std-only Rust rebuild of npm `cowsay@1.6.0` (byte-for-byte default output, 21/21 golden cases).
- Library API `cowsay_bubble::say(text, &Options)`, `Wrap::{None, Hard, Words}`, `auto_width()`.
- CLI: `-W N|auto`, `-n`, `--word-wrap`, 190 embedded cowfiles, `-f`, `-r`, `-l`, modes `-bdgpstwy`, `-e`, `-T`, `--think`/`cowthink`.
- CI matrix Ubuntu/macOS/Windows.
