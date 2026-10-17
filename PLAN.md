# PLAN — ZenCode Desktop

Quy tắc: mỗi phiên làm MỘT bước (≤ ~1 giờ), ghi cách chứng minh; lỗi 2 lần ở cùng bước → ghi vào mục "Thất bại" và dừng bước đó.
Không được làm hỏng `rebuild/` (golden parity 21/21). Chỉ dùng model `:free`, không tiêu tiền.

## Roadmap lõi agent
| # | Bước | Trạng thái | Chứng minh |
|---|------|-----------|------------|
| a | Crate `core/` (`zencode_core`, std-only): trait `Llm`, `Tool`, `Sandbox`, `SessionLog` + vòng `run_turn` | DONE 2026-10-17 | `cd core && cargo test` (6 test vòng lặp: tool call, "model thấy = đã log", sandbox deny trả về model, unknown tool + step limit, lỗi LLM được log, resume + fork); CI lint/test matrix có `core` |
| b | `JsonlLog`: SessionLog append-only JSONL trên đĩa (encode/decode JSON std-only), replay/fork từ file; dedupe AGENTS.md/CLAUDE.md theo hash nội dung | TODO (phiên sau) | test round-trip ghi→đọc→`replay_context` bằng nhau; ghi lỗi giữa chừng không làm hỏng dòng trước; 2 file cùng nội dung chỉ inject 1 lần |
| c | Tool tối thiểu `shell` + `str_replace` (mode Minimal), hỏi `Sandbox` trước mọi side effect | TODO | test trong tempdir: str_replace đúng 1 lần khớp, báo lỗi khi 0/≥2 khớp; NoSandbox từ chối |
| d | Client OpenAI-compatible tới kênh free (`/workspace/tools/free-llm.sh` / relay), chỉ model `:free` | TODO | từ chối model không có hậu tố `:free`; test với server giả localhost |
| e | Sandbox fail-closed (Landlock / sandbox-exec / Job Object), báo `Isolation::Partial` trung thực | TODO | test policy; trên kernel không hỗ trợ trả Partial chứ không Full |
| f | Ý tưởng từ docs/research/daily-feature-ideas.md (chưa có file) | — | — |

Khi (b)+(c)+(d) xong → lõi agent chạy được một vòng thật → tag `v0.3.0`.

## Ghi chú thiết kế
- `core/` là crate độc lập (Cargo.lock riêng), chưa gộp workspace để không đổi đường dẫn `rebuild/target` mà release.yml đang dùng.
- Tên package `zencode_core` (tránh trùng crate `core` của Rust).
- Bất biến log: mọi message vào context đi qua `log::see()` (log trước, push sau); `run_turn` khởi tạo context bằng `log.replay_context()` → resume/fork tự nhiên.
- Lỗi tool / sandbox deny được trả lại model dưới dạng message `tool` "ERROR: ..."; lỗi LLM / log / step limit kết thúc turn và được log là `Event::Error`.

## Hạ tầng
- Sandbox Hark có thể mất `gcc` sau khi khởi động lại → `cargo test` báo "linker `cc` not found". Sửa: `sudo -n apt-get update -qq && sudo -n apt-get install -y -qq gcc libc6-dev`.

## Thất bại
(chưa có)
