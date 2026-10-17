# ZenCode Desktop — Nghiên cứu / Ý tưởng / Kết quả (log)
## 2026-10-16
### Nghiên cứu / Research
- Đối thủ: Neo-cowsay (Go): word-wrap, go-runewidth, cowfiles nhúng trong binary, --random, màu (--rainbow/--aurora), dùng như thư viện. charasay (Rust): textwrap + unicode-width, mặc định theo độ rộng terminal, --think, nhiều chara màu. ferris-says (Rust): API thư viện say(text,width,writer). gowsay/nmyk cowsay (Go): stdlib-first, full-width.
- Bản gốc npm cowsay@1.6.0: wrap là cắt cứng theo đơn vị UTF-16, không theo từ; -l theo thứ tự readdir của Node (tên file gồm ".cow").
### Ý tưởng / Ideas
- Giữ output mặc định byte-for-byte; tính năng mới chỉ bật bằng cờ.
- Tiếp: thư viện say(text, opts), -W auto theo terminal, màu ANSI tùy chọn, CI ubuntu/macos/windows + release binary.
### Kết quả / Results
- Rust std-only, 0 crate ngoài. Golden parity 21/21, unit test 5/5. --word-wrap xong. 190 cowfile nhúng, -f/-r/-l/modes/-e/-T/--think xong.
- cargo check OK cho Windows (x86_64-pc-windows-gnu) và macOS (x86_64-apple-darwin); binary Linux release 506 KB.
### 18:40 bổ sung
- Kết quả: API thư viện say(text, &Options) (học từ ferris-says/Neo-cowsay), -W auto theo $COLUMNS (học từ charasay), CI GitHub Actions 3 hệ điều hành (chưa push). Test: unit 6/6, doctest 1/1, golden 21/21; Windows/macOS cargo check OK; binary 508 KB.
- Ý tưởng tiếp: đo độ rộng terminal thật bằng ioctl/WinAPI (vẫn std-only qua FFI), màu ANSI tùy chọn (--rainbow như Neo-cowsay), đẩy repo private lên GitHub để CI chạy thật trên macOS/Windows.

## 2026-10-16 — GitHub + CI thật
- **Nghiên cứu**: Windows checkout mặc định `core.autocrlf=true` biến golden `.out` và cowfile thành CRLF → parity fail trên Windows (21 case). Hark GitHub App có quyền contents/workflows/actions write trên mọi repo nhưng không có administration (không tạo repo, không sửa description/topics).
- **Ý tưởng**: `.gitattributes` ép LF cho golden/cowfiles; sau này thêm job release (tag → build binary 3 OS → GitHub Release), commit `Cargo.lock`, job clippy/fmt.
- **Kết quả**: repo private `7enlab/zencode-desktop`, 2 commit trên `main`. CI run 38897073575: ubuntu ✅ macOS ✅ windows ✅ (lần đầu Windows fail vì CRLF, đã sửa). Artifacts binary cho 3 OS. Thêm LICENSE, CHANGELOG, .editorconfig, metadata Cargo, CI chỉ chạy push main/PR/dispatch, permissions read-only, concurrency.

## 2026-10-16 19:04 — Terminal width thật + release
- **Nghiên cứu**: `TIOCGWINSZ` khác nhau theo OS (Linux 0x5413, macOS/BSD 0x40087468); Windows lấy `srWindow.Right-Left+1` từ `GetConsoleScreenBufferInfo`. Làm bằng `extern "C"/"system"` của std, không cần crate `libc`/`windows-sys`.
- **Ý tưởng**: thứ tự `$COLUMNS` → terminal thật → 40 để giữ golden test ổn định; release theo tag `v*` cho Linux x86_64, macOS arm64, Windows x86_64.
- **Kết quả**: commit cc20b2b, CI run 38897812171 xanh trên Ubuntu/macOS/Windows. Workflow release sẵn sàng, chưa gắn tag.


## 2026-10-17 — Lint gate + ANSI colour

### Nghiên cứu
- Neo-cowsay (Go) có `--rainbow`/`--aurora`/`--bold`; charasay (Rust) tô màu bằng crate ngoài. Chuẩn cộng đồng: NO_COLOR (no-color.org: biến khác rỗng = tắt màu) và CLICOLOR_FORCE (bixense.com/clicolors: khác `0` = ép màu).
- Windows 10+ chỉ hiểu ANSI khi bật `ENABLE_VIRTUAL_TERMINAL_PROCESSING` (0x0004) qua `SetConsoleMode`; console cũ từ chối → bỏ màu.
- `std::io::IsTerminal` (std, Rust 1.70) thay crate `atty`/`is-terminal`.
- clippy có thể chạy cross-target (`--target x86_64-pc-windows-msvc`) mà không cần linker → bắt lỗi code `cfg(windows)` ngay trên Ubuntu.

### Ý tưởng
- Mặc định tắt màu để giữ parity byte-for-byte; màu là extension có cờ riêng.
- Reset `ESC[0m` cuối mỗi dòng để màu không tràn; `strip()` để test vòng lặp paint→strip = bản gốc.
- Sau này: 256-màu/truecolor (`COLORTERM=truecolor`), gradient aurora, tô bubble và cow khác màu.

### Kết quả
- CI thêm job lint: `cargo fmt --check` + `cargo clippy -D warnings` cho Linux, Windows, macOS; build/test dùng `--locked`; commit `Cargo.lock`.
- Module `color.rs` (std-only, có FFI Windows), cờ `--cow-color`, `--color=WHEN`; 2 unit test + 1 integration test mới; golden 21/21 không đổi (mặc định không màu khi pipe).

## 2026-10-17 — Đối thủ: DeepSeek Harness (dsh)

### Nghiên cứu
- dsh: agent harness mã nguồn mở MIT của DeepSeek (ra 13/08/2026, developer preview, bản nguồn 0.1.5-alpha.1), TypeScript monorepo trên Cordis (khung plugin). Chạy `npx @deepseek-ai/dsh web` (Web UI 127.0.0.1:3080), có Desktop app, profile `headless` cho tác vụ one-shot, SDK Python qua JSON-RPC trên stdio.
- "Everything is a plugin": model adapter (`ctx.llm`), tool registry, sandbox, session log, UI và cả agent loop đều là plugin; loop phát sự kiện công khai (`turn/start`, `system-prompt/assemble`, `tools/pre-execute`, `tools/post-execute`...).
- Cordis: hiệu ứng có thể đảo ngược (LIFO, gỡ plugin trả hệ về đúng trạng thái cũ) + coeffect phản ứng (dependency khai báo, tự kích hoạt lại).
- Session log append-only, bất biến "model-visible means logged" → resume/fork/replay/Trajectory view.
- 4 mode: Standard, Code (`run_code`: model viết TS gọi nhiều tool một lần), Minimal (shell + str_replace_editor, dùng cho benchmark), Creator.
- Sandbox: Linux bwrap + Landlock (launcher C fail-closed), macOS Seatbelt, Windows restricted ACL token; tự báo "partial" khi kernel cũ.
- Subagent có thể là Claude Code / Codex; MCP, skills, goals/jobs, schedule, webhook, LSP.
- Điểm yếu (ArceApps/Justin3Go): tốn token gấp ~10 lần Pi trên cùng model (4.5K vs 47.6K), bug inject CLAUDE.md + AGENTS.md hai lần, plugin chạy trong process (không sandbox), hệ plugin cộng đồng chưa ổn (41 tương thích / 219 cần xem lại), API đổi liên tục.

### Ý tưởng cho ZenCode Desktop
- Kiến trúc "seam": tách trait `Llm`, `Tool`, `Sandbox`, `SessionLog` trong Rust; plugin = crate/feature hoặc tiến trình con qua JSON-RPC stdio (an toàn hơn dsh vì plugin ngoài process).
- Session log append-only JSONL + replay/fork; bất biến "cái gì model thấy đều được log".
- Sandbox đa nền tảng std-first: Landlock syscalls (Linux), sandbox-exec profile (macOS), Job Object/restricted token (Windows) — fail-closed, báo partial trung thực.
- Mode Minimal (shell + str_replace) làm baseline đánh giá model free; tránh nhân đôi CLAUDE.md/AGENTS.md (hash nội dung, dedupe).
- Tiết kiệm token là lợi thế cạnh tranh vì ta chỉ dùng model free.

### Kết quả
- Chưa cài dsh (cần Node + API key DeepSeek; DeepSeek API trả phí → không dùng). Có thể thử sau với endpoint OpenAI-compatible free qua relay.
