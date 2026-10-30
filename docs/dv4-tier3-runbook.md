# DV-4 tầng 3 — Runbook chạy binary thật với mock LLM (30/10/2026)

Tầng 3 ghim CLI hai phía theo quyết tin 1242/1269: binary CLI thật chạy
dạng subprocess như Desktop thật, LLM giả lập bằng stub HTTP cục bộ,
output stream-json được parser Desktop kiểm chứng. Tài liệu này ghi đủ để
chạy lại từ đầu trên box ZD Developer; số liệu chạy đầu tiên ở §5.

## 1. Vị trí (quyết 1242: binary chạy trên box ZD Developer trong `~/muse2`)

| Thành phần | Đường dẫn trên box | Ghi chú |
|---|---|---|
| Source CLI | `~/muse2/zencode` | Clone `github.com/7enlabs/zencode` @ rev ghim `290154c` |
| Binary | `~/muse2/zen` | Copy từ `~/muse2/zencode/target/debug/zen`, profile debug |
| Mock stub | `~/muse2/mock_llm_stub.py` | Python http.server, chỉ bind `127.0.0.1:18123` |
| Output bắt được | `~/muse2/tier3-output.jsonl` | Stdout của lần chạy §4 |
| Test tầng 3 | `engine/tests/tier3_live_cli.rs` | Trên nhánh DV-4, `#[ignore]`, đọc `DV4_TIER3_CAPTURE` |

Lựa chọn đường dẫn: design doc §5 không nói rõ đường dẫn nên chọn theo
quyết 1242 (`~/muse2`): source tại `~/muse2/zencode`, binary tại `~/muse2/`.

## 2. Build binary (profile debug — design doc im lặng về profile nên chọn debug cho nhanh)

```sh
cd ~/muse2
git clone --depth 100 https://github.com/7enlabs/zencode.git zencode
cd zencode && git checkout -q 290154c && git rev-parse HEAD
# kỳ vọng: 290154c1da24e482cd888f7099a01b036597c15f
cargo build -p zen-cli   # lần đầu cần mạng để lấy crate thiếu trong cache (webpki-roots 1.0.10); build lại sau đó offline được
cp target/debug/zen ~/muse2/zen
~/muse2/zen --version   # kỳ vọng: zen 0.1.0
```

Chú ý: `cargo build --offline` fail lúc đầu vì cache box chỉ có
webpki-roots 1.0.9/1.0.5 trong khi lock cần 1.0.10 — build online lần đầu
để lấy crate đó. Mạng này chỉ phục vụ build (crates.io), không liên quan
đường LLM: stub không bao giờ gọi mạng ngoài.

## 3. Mock LLM stub (đọc code client trước khi viết)

Client LLM của CLI 290154c (`crates/zen-agent/src/client.rs:264`) POST tới
`{base_url}/chat/completions` theo schema OpenAI chat.completion. Stub bắt
chước đúng mock test của chính repo CLI
(`crates/zen-cli/tests/common/mod.rs`, `Reply::text` + `MockServer`):
một canned response duy nhất cho mọi request, không random, không clock,
không nội dung phụ thuộc request — cùng request luôn cho cùng response
byte-identical (đã chứng minh bằng 2 lần curl giống hệt, xem §5).

```sh
nohup python3 ~/muse2/mock_llm_stub.py > /tmp/mock-stub.out 2>&1 &
# stub log request tại /tmp/mock-stub.log (chỉ method + path + số thứ tự)
```

Cấu hình binary trỏ tới stub bằng flag chính thức của CLI (không patch
source CLI): `--model mock-model --base-url http://127.0.0.1:18123/v1`,
kèm `ZEN_API_KEY=dummy` (key giả, stub không kiểm tra).

## 4. Chạy harness tầng 3 (đúng cách Desktop gọi CLI, qua subprocess)

```sh
rm -rf /tmp/dv4-tier3-home /tmp/dv4-tier3-cwd && mkdir -p /tmp/dv4-tier3-home /tmp/dv4-tier3-cwd
cd /tmp/dv4-tier3-cwd
env -u ZEN_MODEL -u ZEN_BASE_URL -u OPENAI_API_KEY -u ZEN_SETTINGS \
  HOME=/tmp/dv4-tier3-home ZEN_API_KEY=dummy ZEN_TRUST_FOLDER=1 \
  ~/muse2/zen -p --output-format stream-json \
  --model mock-model --base-url http://127.0.0.1:18123/v1 hello \
  > ~/muse2/tier3-output.jsonl 2> /tmp/dv4-tier3-stderr.txt
echo "exit=$?"
```

Isolated HOME + `ZEN_TRUST_FOLDER=1` bắt chước cách test của chính repo CLI
(`ZenRun`) để lần chạy không dính setting người dùng; các flag `--model` /
`--base-url` là đường trỏ provider chính thức của CLI, không phải patch.

Kiểm chứng bằng parser thật (test `#[ignore]`, không chạy trong `cargo test`
thường để CI không cần file capture):

```sh
cd ~/zencode-desktop/engine
DV4_TIER3_CAPTURE=~/muse2/tier3-output.jsonl \
  cargo test --offline --locked --test tier3_live_cli -- --ignored
```

## 5. Bằng chứng chạy đầu tiên (30/10/2026, box ZD Developer)

- Build: `cargo build -p zen-cli` — xong, binary `~/muse2/zen`, `zen 0.1.0`.
  Rev đã build: `290154c1da24e482cd888f7099a01b036597c15f`.
- Stub: `127.0.0.1:18123`, 2 lần curl request giống hệt cho response
  byte-identical (`DETERMINISTIC-IDENTICAL`); log stub ghi 3 request
  (2 curl kiểm chứng + 1 lần binary gọi) — binary chỉ gọi đúng 1 lần LLM,
  toàn bộ qua localhost, không có gọi mạng ngoài.
- Lần chạy subprocess: exit code 0, stderr rỗng, stdout 3 dòng, đều `v = 1`:
  `system/init` (tools, model `mock-model`) → `assistant`
  (content `Hello from the mock LLM.`) → `result` (subtype `success`,
  `is_error false`, `num_turns 1`, usage 7/3 khớp đúng usage của stub).
- Parser check (`tier3_live_cli`, 1 passed, 0 failed): 3 dòng parse `Ok`,
  thứ tự `system → assistant → result`, mỗi dòng chiếu ≥ 1 event,
  system → `SystemMessage cli_system:init`, assistant → `AssistantMessage`
  đúng text mock, result → `SessionEnd` success, `num_turns 1`,
  model `mock-model`, input 7 / output 3, `interpret_exit_code(0) = Success`.

Phần thay đổi theo từng lần chạy (bình thường, không phải finding):
`session_id`, `uuid`, `duration_ms` khác nhau mỗi lần — chính repo CLI cũng
normalise các trường này thành placeholder trong contract test của nó.
Nội dung (text mock, usage, thứ tự, subtype) là deterministic.

## 6. Lệch giữa design doc §5 và quyết 1242/1269 (ưu tiên quyết)

- §5 để ngỏ "cần quyết của Leader về nơi chạy binary CLI và mock LLM dùng
  chung" — nay đã chốt: box `~/muse2` + stub HTTP cục bộ (quyết 1242/1269).
- §5 nhắc cơ chế phía CLI "regenerate fixture + cập nhật
  `docs/stream-json-v1.md` trong cùng PR" khi thay đổi có chủ đích — tầng 3
  này KHÔNG regenerate fixture, KHÔNG commit `docs/stream-json-v1.md`:
  fixture là chuẩn, binary phải khớp fixture; việc land doc vẫn chờ quyết
  riêng của Leader (quy tắc bất biến của khung).
- §5 yêu cầu "cả hai đầu đều có mặt trên cùng một main" — nay đúng:
  main `4b71b23` đã có parser + fixtures sau khi PR #3 merge.

## 7. Finding: không có

Mọi check tầng 3 đều pass trong lần chạy đầu tiên; không có check nào fail
để giữ lại làm finding, không có điểm chặn nào (đường LLM stub được sạch
bằng flag chính thức, không cần patch source CLI).
