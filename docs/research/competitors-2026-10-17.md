# ZenCode Desktop — Nghiên cứu đối thủ / Competitor research (2026-10-17)

> Phạm vi / Scope: coding agent CLI/desktop, góc nhìn "chạy tốt trên model MIỄN PHÍ" (OpenRouter `:free`, router free; model yếu/chậm, hay 429, context nhỏ).
> Phương pháp / Method: web_search (ưu tiên sau 2026-08-01) + web_fetch trang gốc khi có thể. Mỗi ý có link.
> Quy ước / Convention: **[gốc]** = đã mở trang gốc (changelog/release/docs/blog tác giả); **[thứ cấp]** = bài tổng hợp/tin tức, chưa mở trang gốc; **chưa xác nhận** = không tìm được nguồn đáng tin.
> Không sửa repo nào. Không có số liệu tự bịa.

---

## Bối cảnh ràng buộc cho người dùng model free / Constraints that shape everything

| Ràng buộc / Constraint | Số liệu / Figure | Nguồn |
|---|---|---|
| OpenRouter `:free` rate limit | 20 req/phút; **50 req/ngày** nếu mua < 10 credits, 1.000 req/ngày nếu ≥ 10 credits | [gốc] https://openrouter.ai/docs/api/reference/limits |
| Request lỗi vẫn bị tính | "Failed attempts count toward that daily cap" | [gốc] https://openrouter.ai/pricing (FAQ "Do you enforce rate limits?") |
| 429 có header | `X-RateLimit-Limit/Remaining/Reset`, đôi khi `Retry-After`; khuyến nghị exponential backoff; hỗ trợ "model fallbacks" | [gốc] https://openrouter.ai/docs/api/reference/limits |
| Kiểm tra quota | `GET /api/v1/key` → `free_model_daily_requests {used, limit, remaining}` (reset 00:00 UTC) | [gốc] cùng trang trên |
| 429 giữa stream | Đến dưới dạng SSE event `finish_reason: "error"` chứ không phải HTTP 429 | [gốc] cùng trang trên |
| Prompt caching gói Free | Bảng giá ghi Prompt Caching = **No** ở gói Free | [gốc] https://openrouter.ai/pricing — *áp dụng cụ thể cho từng `:free` endpoint: chưa xác nhận* |

**Hệ quả / Implication (VI):** với 50 req/ngày, *mỗi request* là tài nguyên khan hiếm → thắng bằng "nhiều việc/1 request" (prompt nhỏ, gộp tool call, ít retry mù), không phải bằng prompt caching.
**(EN):** At 50 requests/day, request economy beats token economy; blind retries literally burn the daily budget.

---

## 1. Nghiên cứu / Research

### 1.1 Bảng đối thủ / Competitor table

| Đối thủ | Ngôn ngữ | Điểm mạnh nổi bật / Standout | Mới nhất 2026 (ngày + link) | Điểm yếu (với user model free) |
|---|---|---|---|---|
| **Claude Code** (Anthropic) | TypeScript (npm) | Hệ sinh thái lớn nhất: subagent (nền, lồng 3 cấp), hooks (33 event), plan mode, skills, mods, `/goal`, Rewind "Summarize up to here" | 2.1.301–302 (15–16/10): model riêng cho auto-mode classifier, fork subagent mặc định, hooks tối đa 12/event [thứ cấp] https://claude-for.com/guides/claude-code-2-1-301-302-2026-10-17 · 2.1.295 (08/10): `onFailure: "block"` cho hook [thứ cấp] https://dev.classmethod.jp/en/articles/20261009-cc-updates-v2-1-295/ · W20 (11–15/5): `/goal`, `continueOnBlock`, "Summarize up to here" [gốc] https://code.claude.com/docs/en/whats-new/2026-w20 | Closed source, gắn model Anthropic; system prompt rất lớn (ước tính 5k–55k token tùy nguồn — **chưa xác nhận** con số) |
| **OpenAI Codex CLI** | **Rust** | Sandbox Linux bubblewrap+seccomp, macOS Seatbelt; apply_patch; Code Mode; parallel read-only tools | rust-v0.163.0 (15/10): steer giữa câu trả lời, chạy song song tool read-only, "Treat incomplete responses as terminal errors" (#53451), sandbox integrity checks [gốc] https://github.com/openai/codex/releases/tag/rust-v0.163.0 · v0.162.0 (08/10): managed git worktrees [thứ cấp] https://bisque.today/p/codex-cli/codex-cli-0-162-0 | Tối ưu cho model OpenAI; custom provider phải tương thích Responses API; codebase rất lớn (hàng trăm PR/bản) |
| **OpenCode** (Anomaly, trước là sst) | TypeScript | 2.0: một background service cho TUI/web/desktop; **Code Mode** (MCP/OpenAPI → hàm JS, nhiều tool/1 round-trip); background subagent; 80+ plugin; agent `plan` read-only | 2.0 (~12/10, Cloudflare changelog ngày 12/10) [thứ cấp, có trích launch note] https://automater.ai/news/opencode-2-0-one-background-service-code-mode-subagents-sdk/ · v2.0.30 (15/10) diff panel mặc định [thứ cấp] https://www.turboai.dev/blog/opencode-2-v2-0-30 · v1.18.17 giới hạn số retry + jitter; v1.18.27 timeout 5 phút cho model khởi động chậm [thứ cấp] https://www.turboai.dev/blog/opencode-v1-18-32 | Breaking changes 2.0 (plugin 1.x hỏng, API server mới); prompt ~15k token theo một nguồn — **chưa xác nhận** |
| **Crush** (Charm) | Go | TUI đẹp, LSP context, MCP, đa nền tảng (cả BSD/Android), Agent Skills | v0.98.0 (08/10): OSC 7501 Program Status, OAuth model ngoài catalog, timeout MCP [thứ cấp] https://seismonews.com/event/charm-ships-crush-v0-98-0-with-grok-oauth-login · v0.98.1 (09/10) [gốc] https://pkg.go.dev/github.com/charmbracelet/crush@v0.98.1/internal/agent | v0.x, không cam kết API; giấy phép FSL-1.1 [thứ cấp] https://kunavo.com/guides/crush-pricing |
| **Aider** | Python | **Repo map** (mặc định `--map-tokens` 1k), nhiều edit format (whole/diff/udiff/architect), benchmark edit công khai | Không có bản lớn năm 2026: tag cuối v0.86.0 (08/2025), PyPI 0.86.2 (12/02/2026) [gốc] https://github.com/Aider-AI/aider/tags · repo map docs [gốc] https://aider.chat/docs/repomap.html | Phát triển chậm lại; không có agent loop tự chạy shell đầy đủ (fork cộng đồng cecli bổ sung [thứ cấp] http://shortlisted.tools/products/aider) |
| **Gemini CLI** (Google) | TypeScript | Free tier Google; plan mode; skills; subagent routing | v0.64.0 (13/10): ghi file **atomic** (temp + rename), serialize thao tác cùng path, state.json có backup/khôi phục [gốc ngày] https://geminicli.com/docs/changelogs/latest/ + [thứ cấp chi tiết] https://aftergradient.com/news/2026/10/gemini-cli-0-64-0 | Các bản 10/2026 chủ yếu sửa lỗi; tối ưu cho Gemini |
| **Pi** (Earendil, Mario Zechner) | TypeScript (Node ≥22.19) | **Tối giản**: 4 tool (read/write/edit/bash), system prompt + tool defs < 1.000 token; không MCP/subagent/plan mode by design; extensions/skills/packages | Pi 1.0 (01/10) + Pi Durable [gốc] https://earendil.com/posts/pi-1-0/ · 1.1.0 (08/10) OSC 7501 [thứ cấp] https://thegitreporter.com/articles/2026-10-09-daily-pi-and-crush-tell-the-terminal-when-they-need-you/ · 1.2.0 (12/10) [thứ cấp] https://en.wikipedia.org/wiki/Pi_(AI_agent) | YOLO mặc định, không sandbox; cần Node; tự build tính năng nâng cao |
| **Goose** (AAIF, gốc Block) | **Rust** + TS | Agent loop dạng **state machine** tách thành crate `goose-agent`; 70+ extension MCP; ACP; desktop+CLI+API | v1.54.0 (08/10): foreground subagent trong state-machine loop, extension lease, detect vision [thứ cấp] https://patchwiredaily.com/r/goose/1.54.0.html · v1.55.0 assets 16/10 [gốc] https://github.com/aaif-goose/goose/releases · v1.47.0 (21/08) tách agent loop thành state machine [thứ cấp] https://mindpattern.ai/f/21707 | Tổng quát (không chuyên coding); gói desktop nặng (~155 MB rpm) |
| **Cline** | TypeScript | IDE + CLI + SDK + Desktop (cho open-weight model), checkpoint, plugin marketplace | CLI 3.0.54 (13/08): **từ chối JSON tool-call bị cắt** thay vì "sửa" âm thầm; token telemetry tách cache [gốc] https://github.com/cline/cline/releases/tag/cli-v3.0.54 · Cline Desktop (3.0.62, 15/09) [thứ cấp] https://freedom.tech/posts/2026-09-15-cline-cli-3-0-62/ | Nặng IDE; nhiều daemon/hub |
| **Kilo Code** | TypeScript (lõi đồng bộ từ OpenCode) | 500+ model giá gốc, modes Ask/Plan/Code/Debug, Agent Manager, worktree | v7.8.10 (14/10, pre): Search Ignored Paths, sửa grep encoding [thứ cấp] https://terminalblog.com/blog/kilocode-v7-8-10-search-ignored-paths-grep-fix-mcp-windows/ · v7.8.0 sync OpenCode v1.18.19–32 [thứ cấp] https://terminalblog.com/blog/kilo-code-v7-8-1-sbom-supply-chain-security/ | Phát hành rất nhanh → lỗi vặt; thuộc Anaconda từ 15/07/2026 [thứ cấp] https://andrew.ooo/answers/kilo-code-vs-cline-vs-continue-open-source-coding-agents-july-2026/ |
| **Zed agent** | **Rust** | Agent native trong editor + **ACP** chạy agent ngoài (Claude Code, Codex, OpenCode, Cursor, Pi…); tự compact thread | 1.23.2 (07/10): Google AI auto-retry khi overload, ACP plan refresh [gốc] https://zed.dev/releases/stable · v1.25.0-pre: compaction cho ACP agent ngoài [thứ cấp] https://www.devagentradar.com/insights/radar-brief-2026-10-15 | Gắn editor Zed |
| **Amp** | chưa xác nhận | Orchestration nhiều agent, orbs (máy remote), thread sharing | Free khi BYOK (13/09) [gốc] https://ampcode.com/news/free-agent · dùng Claude Pro/Max (10/10) [gốc] https://ampcode.com/news/use-your-claude-plan | "Amp Free" (ad-funded) đã dừng [thứ cấp] https://kunavo.com/guides/amp-pricing; hướng tới model frontier |
| **Cursor CLI** | chưa xác nhận | Headless/CI, ACP, sandbox, Auto-review | 2026.10.08: Auto-review hỏi trước lệnh xoá rủi ro [thứ cấp] https://www.vibehacker.com/news/cursor-cli-2026-10-08-makes-auto-review-ask-before-risky-deletes-and-flags-out-o · Projects (10/09) [gốc] https://cursor.com/changelog/projects | Closed, cần tài khoản Cursor; lỗi ACP báo sai model [gốc] https://forum.cursor.com/t/cursor-cli-acp-reports-grok-4-6-fast-despite-explicitly-selecting-the-listed-non-fast-model/174835 |

**Đối chiếu DeepSeek Harness (dsh)** (từ nghiên cứu trước của dự án, không kiểm lại hôm nay): plugin Cordis, session log append-only, sandbox bwrap/Landlock/Seatbelt, tốn token ~10x Pi.

### 1.2 Benchmark

- **Terminal-Bench**: harness ảnh hưởng điểm. Cùng model, khác harness lệch 2,8–8,1 điểm (vd. Fable 5: 83,8% Claude Code vs 80,4% Terminus 2; GPT-5.5: 83,1% Codex vs 78,0% Terminus 2; khoảng tin cậy ±1,2–1,7% → chênh < 3 điểm không đáng tin) [thứ cấp] https://note.com/hacklog_stealth/n/n2b39609cef45?hl=en , https://www.codewithseb.com/blog/terminal-bench-2-1-coding-agent-leaderboard-reality . Bảng chính thức: https://www.tbench.ai/leaderboard/terminal-bench/2.0 (chưa mở được bảng số đầy đủ — số liệu trên **chưa xác nhận từ gốc**). Vals.ai đã "archive" TB 2.0 vì bão hoà [thứ cấp] https://www.vals.ai/benchmarks/terminal-bench-2.
- **SWE-bench Verified (bash-only)**: chấm *mọi* model bằng mini-SWE-agent — ~100 dòng Python, chỉ có bash, **không dùng tool-calling API** → chạy được với bất kỳ model nào; README ghi ">74%" [gốc] https://mini-swe-agent.com/latest/ , https://www.swebench.com/verified.html . Prompt yêu cầu "đúng MỘT khối bash" mỗi lượt [gốc] https://github.com/SWE-agent/mini-swe-agent/blob/60f566bd/src/minisweagent/config/benchmarks/swebench_backticks.yaml.
- **Bài học cho ZenCode**: (a) harness tối giản cạnh tranh được; (b) cần bộ eval riêng chạy trên model free vì leaderboard không đo model yếu + 429.

### 1.3 Kỹ thuật đáng học nhất / Top techniques (7)

1. **Harness tối giản / Lean harness** — Pi: system prompt + 4 tool < 1.000 token [gốc] https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ ; đo độc lập: 550 token prompt + 588 token schema = 1.138 token/request [thứ cấp] https://harrisonsec.com/blog/system-prompt-token-floor/ . mini-SWE-agent >74% SWE-bench Verified chỉ với bash. *Vì sao hiệu quả:* model đã được RL trên vai trò coding agent; prompt nhỏ = ít chiếm context nhỏ, ít "nhiễu" cho model yếu. Pi còn ghi: MCP Playwright 21 tool = 13,7k token, Chrome DevTools MCP = 18k token chiếm 7–9% context trước khi làm việc [gốc, cùng bài].
2. **Áp dụng edit "dễ dãi có kiểm soát" / Flexible edit application** — Aider: tắt flexible patching → lỗi edit tăng **9 lần**; không dùng "high-level diff" → lỗi tăng 30–50%; udiff nâng GPT-4 Turbo từ 20% lên 61% trên bộ refactor 89 task; JSON/function-call kém hơn text format vì escape [gốc] https://aider.chat/2023/12/21/unified-diffs.html . Model yếu nên dùng `whole` [gốc] https://aider.chat/docs/leaderboards/notes.html.
3. **Retry/fallback theo quota / Quota-aware retry & fallback** — OpenRouter: backoff, tôn trọng `Retry-After`, dùng model fallbacks; request lỗi vẫn tính quota [gốc] (xem bảng ràng buộc). OpenCode giới hạn số retry + jitter để tránh "retry storm", timeout 5 phút cho model chậm [thứ cấp] https://www.turboai.dev/blog/opencode-v1-18-32 . Zed auto-retry khi Google AI overload [gốc] https://zed.dev/releases/stable.
4. **Kiểm tra tool-call nghiêm, không "sửa bừa" / Strict tool-call validation** — Cline 3.0.54 từ chối JSON bị cắt thay vì bịa dấu đóng [gốc] https://github.com/cline/cline/releases/tag/cli-v3.0.54 ; Codex coi response incomplete là lỗi terminal (#53451) [gốc] https://github.com/openai/codex/releases/tag/rust-v0.163.0 ; Pi validate tham số bằng schema và trả lỗi chi tiết cho model [gốc] bài Pi. Kết hợp với **text-mode fallback** kiểu mini-SWE-agent (một khối ```bash```) cho model free không có tool-calling.
5. **Quản lý context / Context management** — Claude Code Rewind "Summarize up to here" giữ nguyên các lượt gần [gốc] https://code.claude.com/docs/en/whats-new/2026-w20 ; Pi compaction (docs) + gợi ý "giữ ~20k token gần nhất, tóm tắt phần cũ" [thứ cấp] https://stillbrook.dev/articles/conservative-to-the-token ; Pi `read` mặc định 2.000 dòng + offset/limit [gốc] bài Pi; progressive disclosure (README/skill chỉ đọc khi cần thay vì nạp toàn bộ MCP schema) [gốc] bài Pi.
6. **Kinh tế request / Request economy** — Codex chạy song song tool read-only (0.163) [gốc]; OpenCode Code Mode: một script gọi nhiều tool trong 1 round-trip, MCP mặc định qua Code Mode [thứ cấp, trích launch note]. Với 50 req/ngày đây là đòn bẩy lớn nhất (giảm số lượt LLM/task).
7. **An toàn & hoàn tác / Safety & undo** — ghi file atomic (temp+rename) và serialize cùng path (Gemini 0.64) [thứ cấp chi tiết]; hook `onFailure: "block"` (hook lỗi → chặn thay vì cho qua) [thứ cấp]; `continueOnBlock` trả lý do từ chối về model và tiếp tục lượt [gốc W20]; sandbox bwrap + seccomp (Codex docs) [gốc 0.163]; checkpoint snapshot (Cline Desktop) [thứ cấp].

> Ghi chú prompt caching: OpenRouter docs có sticky routing + cache 0,1×–0,5× giá input cho provider hỗ trợ [gốc] https://openrouter.ai/docs/guides/best-practices/prompt-caching , nhưng gói Free ghi "Prompt Caching: No" [gốc pricing]. → Với user free, **giữ prefix ổn định vẫn nên làm (rẻ, tương thích tương lai)** nhưng không kỳ vọng lợi ích đo được; đặt ưu tiên thấp.

---

## 2. Ý tưởng / Ideas — 12 tính năng cho ZenCode (xếp theo giá trị cho user model free)

Ký hiệu công sức / Effort: S ≤ 1 ngày · M 2–4 ngày · L ≥ 1 tuần. Mốc: **[v0.3.0] [v0.4.0] [sau/later]**.
Giả định kiến trúc: `core/` có trait `Llm`, `Tool`, `Sandbox`, `SessionLog` (JSONL). Lựa chọn HTTP/TLS std-first (crate tối thiểu như `ureq`+`rustls` hay gọi `curl`) — **chưa xác nhận** quyết định hiện tại của dự án.

| # | Tính năng / Feature | Học từ | Vì sao (free model) | Phác thảo Rust trong `core/` | Công sức | Đo trước/sau / Metric | Mốc |
|---|---|---|---|---|---|---|---|
| 1 | **RateGate + ModelChain** (429-aware fallback chain) | OpenRouter limits/fallbacks; OpenCode retry cap+jitter | 429 và quota 50/ngày là lỗi phổ biến nhất; retry mù đốt quota | `core/llm/rate.rs`: `struct RateGate { rpm: TokenBucket, daily: DailyBudget }` đọc `X-RateLimit-*`, `Retry-After`, `GET /api/v1/key`. `core/llm/chain.rs`: `struct ModelChain(Vec<Box<dyn Llm>>)` impl `Llm`; `enum LlmError { RateLimited{retry_after, scope: Minute\|Day}, ContextOverflow, Transient, Fatal }`; khi `Day` → nhảy model/router kế, không retry; khi `Minute` → chờ, tối đa N lần + jitter; xử lý cả lỗi giữa stream (`finish_reason:"error"`) | M | % task hoàn thành trên 10 task eval khi dùng 1 key free; số request lãng phí (lỗi) / task; số lần dừng vì 429 | **v0.3.0** |
| 2 | **Prompt budget ≤ 1.200 token** (system + tool schemas) | Pi | Context nhỏ + mỗi request quý | `core/prompt/mod.rs`: prompt tĩnh, 3 tool (shell/read/str_replace) mô tả ngắn; `fn prompt_floor_tokens()` ước lượng (chars/4) in ra trong `--stats`; test CI fail nếu vượt ngưỡng | S | token cố định/request (mục tiêu ≤ 1.200; Pi đo 1.138) | **v0.3.0** |
| 3 | **str_replace dễ dãi có kiểm soát** | Aider flexible patching; Pi edit | Model yếu hay sai whitespace/indent → edit fail → tốn thêm request | `core/tools/str_replace.rs` + `core/edit/matcher.rs`: thang khớp exact → chuẩn hoá CRLF/trailing space → relative-indent → khớp theo dòng bỏ dòng trống; bắt buộc **duy nhất 1 match**, nếu 0/≥2 trả lỗi có cấu trúc (top-3 vùng gần giống + số dòng) | M | tỉ lệ edit thất bại / tổng edit; số request "sửa lại edit" / task (Aider: tắt flexible → lỗi ×9) | **v0.3.0** |
| 4 | **Tool-call parser nghiêm + text-mode fallback** | Cline 3.0.54; Codex #53451; mini-SWE-agent | Nhiều model free không có/không ổn định tool-calling; JSON bị cắt | `core/parse/toolcall.rs`: validate bằng serde theo schema; JSON cắt/thiếu → `ToolError::Malformed` gửi lại cho model (không tự bịa). `core/parse/text_fallback.rs`: nếu provider không hỗ trợ tools, yêu cầu đúng 1 khối ```bash``` hoặc khối `<<<str_replace>>>`; capability lưu trong `ModelProfile` | M | % lượt có tool-call hợp lệ; số model free chạy được `zencode run` (mục tiêu: cả model không có tools) | **v0.3.0** |
| 5 | **Cắt output tool + spill ra file** | Pi read 2.000 dòng; Gemini maxChars | Một `cargo build` dài có thể nuốt hết context nhỏ | `core/context/truncate.rs`: head+tail theo token budget, phần bị cắt ghi `.zencode/spill/<id>.txt` và báo đường dẫn để model `read` với offset | S | token trung bình/tool result; số lần ContextOverflow | **v0.3.0** |
| 6 | **Bộ eval cục bộ + metrics từ JSONL** | Terminal-Bench (harness gap), mini-SWE-agent | Không đo thì không biết tính năng nào giúp model free | `core/eval/`: 10–20 task nhỏ (repo mẫu + lệnh verify); runner đọc SessionLog JSONL → requests/task, tokens/task, 429, edit-fail, pass | S–M | baseline cho mọi tính năng khác | **v0.3.0** |
| 7 | **Nhiều tool call / 1 lượt + song song read-only** | Codex 0.163; OpenCode Code Mode | Giảm số lượt LLM/task — đòn bẩy lớn nhất khi 50 req/ngày | `core/agent/loop.rs`: chấp nhận `Vec<ToolCall>` mỗi response; `Tool::is_read_only()`; chạy read-only song song bằng `std::thread::scope`, tool ghi chạy tuần tự; prompt khuyến khích gộp | M | requests/task trên bộ eval (mục tiêu −25%, **chưa xác nhận** khả thi) | v0.4.0 |
| 8 | **Compaction "giữ gần, tóm xa"** | Claude Code Rewind summarize; Pi compaction | Model free context nhỏ (thường vài chục k token — tùy model) | `core/context/compact.rs`: `trait Compactor`; khi vượt 70% context: giữ system + N lượt cuối, tóm tắt phần cũ bằng *chính model đang dùng* (1 request) hoặc tóm tắt cơ học không tốn request (danh sách file đã sửa, lệnh đã chạy, lỗi cuối); ghi event `compaction` vào JSONL | M | số task chết vì overflow; pass-rate task dài | v0.4.0 |
| 9 | **Checkpoint/undo + ghi atomic** | Gemini 0.64 atomic; Cline checkpoint; Claude Code rewind | Model yếu phá file thường xuyên → cần quay lui rẻ | `core/fs/atomic.rs` (write temp + `std::fs::rename`); `core/checkpoint.rs`: snapshot file trước mỗi edit vào `.zencode/ckpt/<turn>/`; `zencode undo [turn]` | S–M | thời gian/khả năng khôi phục sau edit hỏng; 0 file hỏng khi kill giữa chừng | v0.4.0 |
| 10 | **Verify loop `--until "<cmd>"`** | Claude Code `/goal` | Model yếu hay tuyên bố "xong" sai; kiểm bằng lệnh thật không tốn request LLM | `core/agent/goal.rs`: sau mỗi lượt kết thúc, chạy lệnh verify trong Sandbox; fail → feed stderr (đã truncate) làm lượt mới; trần số lượt theo DailyBudget | S | % "xong giả" trên eval; pass-rate | v0.4.0 |
| 11 | **Plan mode = tập tool read-only** | OpenCode agent `plan`; Pi `--tools read,...` | Rẻ: lập kế hoạch bằng model free nhanh, sửa bằng model tốt hơn hoặc ngược lại | `core/agent/profile.rs`: `AgentProfile { tools: ToolSet, model: ModelChainId }`; `zencode run --plan` ghi `PLAN.md` | S | — (tính năng UX); đo requests ở pha plan | v0.4.0 |
| 12 | **Repo map nhẹ (std-first)** | Aider repo map (1k token mặc định) | Giảm lượt `ls/grep` dò codebase | `core/repomap.rs`: quét file theo `.gitignore`, trích symbol bằng regex/heuristic theo ngôn ngữ (fn/struct/class), xếp hạng theo file được nhắc + gần đây; budget 1k token; tree-sitter để sau (crate nặng) | M–L | requests/task cho task "tìm & sửa" | sau / later |
| + | Hooks (pre/post tool, `onFailure: block`, `continueOnBlock`) | Claude Code | An toàn, tự động hoá | `core/hooks.rs`, exec dạng argv không qua shell | M | — | sau |
| + | Sandbox bwrap/Landlock/Seatbelt | Codex, dsh | Chạy YOLO an toàn hơn | impl `Sandbox` theo OS | L | — | sau |
| + | ACP server (chạy trong Zed) | Zed ACP | Phân phối: Zed liệt kê Pi/OpenCode/Codex làm external agent | `zencode acp` qua stdio JSON-RPC | M–L | — | sau |
| + | Subagent kiểu Pi (tự gọi `zencode run --print`) | Pi | Không cần cơ chế subagent riêng | dùng shell tool | S | — | sau |

---

## 3. Kết quả / Results — Lộ trình đề xuất v0.3.0 → v0.5.0

### v0.3.0 — "Một vòng agent thật, sống sót trên key free" / *A real loop that survives a free key*
1. `zencode run "<task>"` với 3 tool shell/read/str_replace + SessionLog JSONL (mốc đã định). **Xong khi:** chạy hết 10 task eval ở chế độ offline (mock `Llm`) và ≥ 1 model `:free` thật hoàn thành ≥ 5/10.
2. RateGate + ModelChain (#1). **Xong khi:** test giả lập 429 (phút/ngày, có/không `Retry-After`, lỗi giữa stream) đều đi đúng nhánh; số request lỗi/task ≤ 1 trung bình trên eval.
3. Tool-call parser nghiêm + text-mode fallback (#4) và str_replace dễ dãi (#3). **Xong khi:** bộ test fixture (≥ 30 trường hợp whitespace/indent/CRLF/JSON cắt) pass; edit-fail rate được log; ≥ 1 model không có tool-calling chạy được.
4. Prompt budget ≤ 1.200 token (#2) + cắt output (#5). **Xong khi:** CI test khẳng định floor ≤ 1.200 token (ước lượng); không tool result nào > budget cấu hình.
5. Eval runner + báo cáo metrics (#6). **Xong khi:** `zencode eval` in bảng requests/task, tokens/task, 429, edit-fail, pass — làm baseline cho v0.4.0.

### v0.4.0 — "Ít request hơn, khó hỏng hơn" / *Fewer requests, harder to break*
1. Nhiều tool call/lượt + song song read-only (#7). **Xong khi:** requests/task trên eval giảm so với baseline v0.3.0 (báo số thực, mục tiêu −25%).
2. Compaction (#8). **Xong khi:** task dài (≥ 30 lượt) trên model context nhỏ không chết vì overflow; event compaction có trong JSONL.
3. Checkpoint/undo + ghi atomic (#9). **Xong khi:** test kill-giữa-chừng không để file nửa vời; `zencode undo` khôi phục đúng byte.
4. Verify loop `--until` (#10) + plan mode (#11). **Xong khi:** tỉ lệ "xong giả" trên eval giảm so với v0.3.0; `--plan` không thể gọi tool ghi (test).

### v0.5.0 — "Mở rộng & an toàn" / *Extensibility & safety*
1. Repo map nhẹ (#12). **Xong khi:** requests/task cho nhóm task "tìm & sửa" giảm so với v0.4.0; map ≤ 1k token.
2. Hooks (pre/post tool, onFailure block, continueOnBlock). **Xong khi:** hook lỗi/timeout chặn hành động (test); lý do chặn được trả cho model.
3. Sandbox Linux (bwrap hoặc Landlock) → macOS Seatbelt. **Xong khi:** lệnh ghi ngoài workspace bị chặn trong test tích hợp trên Linux CI.
4. ACP server cho Zed (hoặc skills/progressive disclosure nếu ACP để sau). **Xong khi:** Zed nhận ZenCode làm external agent và chạy 1 task (kiểm tay + ghi lại).

---

## Hạn chế của báo cáo / Limitations
- Nhiều chi tiết release lấy từ trang tin thứ cấp (ghi rõ [thứ cấp]); nên mở changelog gốc trước khi trích dẫn công khai.
- Số liệu Terminal-Bench "harness gap" chưa đối chiếu được trên bảng gốc tbench.ai.
- Kích thước system prompt của Claude Code/OpenCode: các nguồn mâu thuẫn → chưa xác nhận.
- Ngôn ngữ cài đặt của Amp và Cursor CLI: chưa xác nhận.
- Mục tiêu "−25% requests" là giả thuyết cần eval kiểm chứng, không phải số liệu đo.
