# PLAN — ZenCode Desktop

> **SUPERSEDED.** The authoritative roadmap is **ZenCode Desktop — Roadmap FINAL**
> (Notion, ZenCode-Hub, written by Hark 2, 2026-10-18). This file is kept for history.
> Milestones: v0.3.0 "Foundation + Trust" → v0.4.0 "Stream, Freedom & Hardening" →
> v0.5.0 "Control Room & Proof" → v1.0.0 "Stable & Open".

Quy tắc: mỗi phiên làm MỘT bước (≤ ~1 giờ), ghi cách chứng minh; lỗi 2 lần ở cùng bước → ghi vào mục "Thất bại" và dừng bước đó.
Không được làm hỏng `rebuild/` (golden parity 21/21). Chỉ dùng model `:free`, không tiêu tiền.

## Roadmap lõi agent
| # | Bước | Trạng thái | Chứng minh |
|---|------|-----------|------------|
| a | Crate `core/` (`zencode_core`, std-only): trait `Llm`, `Tool`, `Sandbox`, `SessionLog` + vòng `run_turn` | DONE 2026-10-17 | `cd core && cargo test` (6 test vòng lặp: tool call, "model thấy = đã log", sandbox deny trả về model, unknown tool + step limit, lỗi LLM được log, resume + fork); CI lint/test matrix có `core` |
| b | `JsonlLog`: SessionLog append-only JSONL trên đĩa (encode/decode JSON std-only), replay/fork từ file; dedupe AGENTS.md/CLAUDE.md theo hash nội dung | DONE 2026-10-17 (core/src/jsonl.rs, core/tests/jsonl.rs: 4 test) | test round-trip ghi→đọc→`replay_context` bằng nhau; ghi lỗi giữa chừng không làm hỏng dòng trước; 2 file cùng nội dung chỉ inject 1 lần |
| c | Tool tối thiểu `shell` + `str_replace` (mode Minimal), hỏi `Sandbox` trước mọi side effect | SUPERSEDED 2026-10-17 → dùng zen-tools của zencode CLI | test trong tempdir: str_replace đúng 1 lần khớp, báo lỗi khi 0/≥2 khớp; NoSandbox từ chối |
| d | Client OpenAI-compatible tới kênh free (`/workspace/tools/free-llm.sh` / relay), chỉ model `:free` | SUPERSEDED 2026-10-17 → dùng zen-agent client.rs (retry 429 + fallback_models) | từ chối model không có hậu tố `:free`; test với server giả localhost |
| e | Sandbox fail-closed (Landlock / sandbox-exec / Job Object), báo `Isolation::Partial` trung thực | DEFERRED (sau khi tích hợp CLI; CLI đã có permissions) | test policy; trên kernel không hỗ trợ trả Partial chứ không Full |
| f | Ý tưởng từ docs/research/daily-feature-ideas.md (chưa có file) | — | — |

Khi (b)+(c)+(d) xong → lõi agent chạy được một vòng thật → tag `v0.3.0`.

## HƯỚNG MỚI 2026-10-17: dựa trên ZenCode CLI (Zen quyết định)
ZenCode CLI = https://gitlab.com/atzenn/zencode (public, Rust workspace ~27k dòng: zen-core, zen-agent, zen-tools, zen-cli, zen-ext, zen-mcp). Đã có: vòng agent, client OpenAI-compatible + retry 429/5xx theo Retry-After + `fallback_models`, `edit_match.rs` sửa file linh hoạt, compaction, session event log replay byte-identical, hooks, MCP, sub-agent, background Bash, permissions, headless JSON, CI + cargo-audit. Desktop KHÔNG tự viết lại các phần này nữa; `core/` (zencode_core) đóng băng, giữ để tham khảo, không xóa.
The Desktop consumes the CLI crates through a pinned-rev git dependency (currently `3a09220`); rev bumps are deliberate. Only read access to the atzenn repo: core improvements go in a Desktop-side wrapper layer, and proposals are recorded in docs/upstream-proposals.md (send PRs once Write access is granted).

| # | Bước | Trạng thái | Chứng minh |
|---|------|-----------|------------|
| g1 | Crate `engine/` phụ thuộc `zen-agent`, `zen-tools`, `zen-core` (git, rev ghim); chạy 1 vòng agent với LLM giả (server localhost) gọi tool Read + Edit trong tempdir | DONE (4 test: Read→Edit qua HTTP thật tới server giả, Bash, lỗi 500, input sai) | `cd engine && cargo test` xanh; CI build engine trên 3 OS |
| s1 | Schema sự kiện có version (engine/src/events.rs, SCHEMA_VERSION=1) + test hợp đồng insta | DONE 2026-10-17 (CI xanh 3 OS, 4512c0f) | `cd engine && cargo test`: snapshot event_stream_v1 + từ chối version mới/thiếu (6 test xanh) |
| g2 | `zencode run "<task>"` gọi engine với kênh free thật (chỉ `:free`), lưu session | TODO | chạy thật 1 task nhỏ, transcript không key vào docs/e2e/ |
| g3 | RateGate: hạn mức ngày theo provider (OpenRouter :free 50 req/ngày, lỗi cũng tính) bọc quanh client CLI, chuyển model khi cạn | TODO | test đếm quota + chuyển model với server giả |
| g4 | Ngân sách prompt: đo system prompt + tool defs của CLI, test CI ≤1.200 token (hoặc profile tool tối thiểu) | TODO | test in số token, fail nếu vượt |
| g5 | `zencode eval` 10 task, bảng requests/tokens/429/edit-fail/pass | TODO | chạy với ≥1 model :free; tag v0.3.0 khi ≥5/10 + CI xanh |
| g6 | Desktop UI (sau v0.3.0) | TODO | — |

## Lộ trình theo nghiên cứu đối thủ (2026-10-17, docs/research/competitors-2026-10-17.md §3)
- v0.3.0: (1) `zencode run` + shell/read/str_replace + JSONL; (2) RateGate + ModelChain (429, cap 50 req/ngày :free, fallback); (3) tool-call parser nghiêm + text-mode fallback (1 khối bash) + str_replace dễ dãi (whitespace/CRLF/indent, vẫn đúng 1 match); (4) system prompt + tool defs ≤ 1.200 token (CI test) + cắt output tool; (5) `zencode eval` 10 task, bảng requests/tokens/429/edit-fail/pass. Tag khi ≥1 model :free thật xong ≥5/10.
- v0.4.0: nhiều tool call/lượt + read-only song song; compaction; checkpoint/undo + ghi atomic; verify loop `--until` + plan mode.
- v0.5.0: repo map ≤1k token; hooks; sandbox Linux → macOS; ACP cho Zed.

## Ghi chú thiết kế
- `core/` là crate độc lập (Cargo.lock riêng), chưa gộp workspace để không đổi đường dẫn `rebuild/target` mà release.yml đang dùng.
- Tên package `zencode_core` (tránh trùng crate `core` của Rust).
- Bất biến log: mọi message vào context đi qua `log::see()` (log trước, push sau); `run_turn` khởi tạo context bằng `log.replay_context()` → resume/fork tự nhiên.
- Lỗi tool / sandbox deny được trả lại model dưới dạng message `tool` "ERROR: ..."; lỗi LLM / log / step limit kết thúc turn và được log là `Event::Error`.

## Hạ tầng
- Sandbox Hark có thể mất `gcc` sau khi khởi động lại → `cargo test` báo "linker `cc` not found". Sửa: `sudo -n apt-get update -qq && sudo -n apt-get install -y -qq gcc libc6-dev`.

## Thất bại
(chưa có)

- g1 ghi chú: Bash tool trên Windows CI lỗi (chưa rõ nguyên nhân, cần log chi tiết) → test Bash chỉ chạy unix; cần điều tra ở g2. Clippy cross-target bị bỏ cho engine (ring không cross-build); test native 3 OS vẫn chạy.

## Status notes (English, 2026-10-19)

### 0.3-7 — R6 convergence guard wired into `run_task` — DONE
- `RunOptions::convergence: Option<ConvergencePolicy>` (default `None` = off).
  `ConvergenceMonitor` observes the agent from outside (sink / backend /
  permission wrappers): first loop → the next tool call is refused with the
  forced re-plan prompt; a second loop → `convergence_stop` event, tools denied,
  the next model call fails with the reason.
- Stagnation fix: a tool round is stagnant only if it made no edit, no passing
  verify (Bash), and produced no tool output not already seen in this run.
  Progress rounds reset the streak (`LoopDetector::record_progress_round`).
  Repeating the same failing command or the same probe is still stagnant.
- Proof: `cd engine && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --locked`
  → 176 passed, 0 failed, 1 ignored (InstaBox, 2026-10-19). Integration tests in
  `engine/tests/convergence.rs`: loop → re-plan → stop; progress stays quiet;
  repeated identical probe still fires `stagnant_rounds`.

### 0.3-5 part 1 — unsigned `cargo tauri build` on Linux (InstaBox) — PASSED
- Built 2026-10-18 in `desktop/src-tauri` (log `~/tauri-build.log` on the box):
  release binary in 2m40s (total 3m15s), `zencode-desktop` binary 33.3 MB.
- Bundles: `ZenCode Desktop_0.1.0_amd64.deb` 9.36 MiB,
  `ZenCode Desktop-0.1.0-1.x86_64.rpm` 9.36 MiB,
  `ZenCode Desktop_0.1.0_amd64.AppImage` 109.29 MiB. No compile errors or
  warnings; only linuxdeploy "Not calling strip" notices for bundled libs.
- No `.sig` files were produced (unsigned build). Note: since 0.3-4 set
  `bundle.createUpdaterArtifacts: true`, a local build without
  `TAURI_SIGNING_PRIVATE_KEY` is expected to stop at the updater signing step
  (Tauri behaviour, not yet re-verified here); local smoke builds can pass
  `--config '{"bundle":{"createUpdaterArtifacts":false}}'`.
- Next (part 2): run the AppImage headless (xvfb) smoke test; macOS/Windows
  bundles need CI runners.
