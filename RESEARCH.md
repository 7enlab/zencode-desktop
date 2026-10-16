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
