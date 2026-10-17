# PLAN — ZenCode Desktop

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
ZenCode CLI = https://github.com/atzenn/zencode (public, Rust workspace ~27k dòng: zen-core, zen-agent, zen-tools, zen-cli, zen-ext, zen-mcp). Đã có: vòng agent, client OpenAI-compatible + retry 429/5xx theo Retry-After + `fallback_models`, `edit_match.rs` sửa file linh hoạt, compaction, session event log replay byte-identical, hooks, MCP, sub-agent, background Bash, permissions, headless JSON, CI + cargo-audit. Desktop KHÔNG tự viết lại các phần này nữa; `core/` (zencode_core) đóng băng, giữ để tham khảo, không xóa.
Desktop dùng crate CLI qua git dependency ghim rev (hiện `3a09220`), cập nhật rev có chủ đích. Chỉ có quyền đọc repo atzenn: cải tiến cho lõi thì làm lớp bọc trong Desktop + ghi đề xuất vào docs/upstream-proposals.md (khi có quyền Write thì gửi PR).

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
