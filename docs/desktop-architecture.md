# ZenCode Desktop — kiến trúc & thứ tự làm (Zen chốt 2026-10-17)

## 1. Nền app
- Tauri 2: vỏ desktop, gọi thẳng crate `zen-agent`/`zen-core`. Plugin chính thức: updater, shell, fs, dialog, store, log, window-state, single-instance, deep-link (`zencode://`).
- Phương án thuần Rust nếu bỏ webview: GPUI (Zed), Iced hoặc egui.

## 2. Nối với ZenCode CLI (github.com/atzenn/zencode)
- Crate qua git dependency (ghim rev), hoặc chạy `zen` headless qua `stream-json`.
- Một file schema có version cho stream-json và event log, kèm test hợp đồng ở cả hai repo (repo CLI chỉ đọc: test hợp đồng phía Desktop + đề xuất trong docs/upstream-proposals.md).
- ACP (Agent Client Protocol của Zed): Desktop chạy được ZenCode và agent khác; ZenCode cắm được vào Zed.
- MCP: tái dùng `zen-mcp`, thêm màn quản lý server MCP.

## 3. Thành phần giao diện
- xterm.js + `portable-pty`: terminal thật.
- CodeMirror 6 (nhẹ) hoặc Monaco: xem/sửa code.
- `similar`: diff từng hunk, chấp nhận/từ chối từng phần.
- `gix`: trạng thái git, checkpoint/undo.
- `notify`: theo dõi file thay đổi.
- `tree-sitter`: tô màu cú pháp, repo map.

## 4. Test và chất lượng
- cargo-nextest; insta (snapshot stream-json/UI state).
- tauri-driver (WebDriver) hoặc Playwright / e2e (tester-army, cài ở /workspace/tools/e2e-army, Firefox vì sandbox arm64 không có Chromium): test giao diện kể cả "thử phá" (input rỗng, bấm liên tục, reload).
- cargo-deny + cargo-audit; cargo-llvm-cov.

## 5. Build và phát hành
- tauri-action: build Windows, macOS, Linux.
- sccache + Swatinem/rust-cache.
- release-please hoặc git-cliff: tự sinh CHANGELOG.
- Ký số macOS/Windows tốn phí → để sau, phát hành bản chưa ký.

## 6. Plugin/skill cho agent
- obra/superpowers, context7 (đã có cho Pi): TDD/plan, docs Tauri mới nhất.
- coding-agents-rules → AGENTS.md của repo Desktop.
- Prompt 22 (team code review), 16 (REA + oracle): review PR, parity test với CLI.
- REVIEW.md + PR template của zencode: chép sang Desktop.

## Thứ tự (sau g1 engine)
1. Schema stream-json có version + test hợp đồng.
2. Vỏ Tauri 2 chạy một phiên chat qua `zen`.
3. Diff view + terminal.
4. CI đa nền tảng với tauri-action, e2e test.
5. ACP, MCP manager, auto-update.
(RateGate, ngân sách prompt, `zencode eval` 10 task vẫn là điều kiện tag v0.3.0; làm xen khi cần.)
