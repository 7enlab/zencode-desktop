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
