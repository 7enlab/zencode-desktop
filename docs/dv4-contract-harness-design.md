# DV-4: Contract-test harness ghim CLI — Design doc (khung, 30/10/2026)

Trạng thái tối nay: **khung chạy được (tầng 1) đã xong trên nhánh
`dev/dv4-contract-harness` từ main `aeaf42d`**. Phần ghim CLI hai phía
(tầng 2 + tầng 3) **chưa làm** — nó chờ DV-2 merge (PR #3), xem §5.
Tài liệu này là một phần của khung: nó ghi rõ phần nào xong, phần nào chờ,
để mốc 19:30 31/10/2026 nghiệm thu không bị hiểu nhầm là harness đầy đủ.

## 1. Phát hiện quan trọng khi bắt đầu (đọc trước khi dùng khung)

Main `7enlabs/zencode-desktop` tại `aeaf42d1449909b168cf9cc168482161ec679de5`
(PR #2 đã merge) **chưa chứa parser stream-json**: không có
`engine/src/stream_json.rs`, không có `engine/tests/stream_json.rs`, không có
fixture nào. Parser + test + 21 fixture hiện nằm ở tuyến DV-2
(`dev/fixtures-stream-json-b2-v2` = `aeaf42d` + 3 commit DV-2/B-2), còn PR #3
đang được rebase riêng — khung tối nay **không** copy code parser từ đó,
không checkout nhánh đó, không đụng `/tmp/dv2/worktree`, để tránh trùng lặp
với PR #3 và tránh xung đột khi PR #3 merge.

Hệ quả trực tiếp, nói thẳng:

- Tầng 1 tối nay kiểm tra hợp đồng ở **mức fixture (JSON thô)**, không gọi
  parser thật — vì trên main chưa có parser để gọi.
- 21 fixture được đưa vào nhánh DV-4 **nguyên byte** (đã kiểm chứng SHA256
  khớp với tuyến DV-2), nên khi PR #3 merge, các file fixture trùng nội dung
  sẽ hợp nhất sạch, không xung đột nội dung.
- Các assert gắn parser (tầng 2, §4) đã được đặc tả chính xác tới từng tên
  hàm, từng fixture, từng giá trị kỳ vọng — sau merge chỉ việc thêm vào,
  không cần nghiên cứu lại.

## 2. Harness sống ở đâu, chạy như thế nào

| Thành phần | Vị trí trên nhánh DV-4 | Trạng thái tối nay |
|---|---|---|
| Design doc này | `docs/dv4-contract-harness-design.md` | Xong |
| Skeleton test tầng 1 | `engine/tests/contract_harness.rs` (cargo tự phát hiện) | Xong, xanh |
| Fixtures 01–21 | `engine/tests/fixtures/stream_json/*.jsonl` (nguyên byte từ tuyến DV-2) | Xong |
| Fixture mới 22 | `engine/tests/fixtures/stream_json/22_unknown_field_on_known_type.jsonl` | Mới, xem §3 |
| Test tầng 2 (parser thật) | Thêm vào `engine/tests/contract_harness.rs` sau merge | Chờ DV-2 (§4) |
| Ghim CLI hai phía | Harness chạy binary thật + parser (§5) | Chờ DV-2 (§5) |

Chạy khung:

```sh
cd engine
cargo test --offline --locked --test contract_harness
cargo test --offline --locked --test contract_harness -- --nocapture  # kèm báo cáo từng fixture
```

Báo cáo từng fixture: test `fixture_inventory_report` in một dòng mỗi
fixture (`fixture=<tên> records=<n> types=[...]`, riêng fixture bị từ chối
in `rejected_input` kèm lý do: `unsupported_version`, `missing_version`,
hoặc số dòng JSON lỗi). Log chạy (`cargo test` đầy đủ) đính kèm cùng báo
cáo nghiệm thu — không báo miệng số pass/fail.

Quy tắc bất biến của khung (ghi để đời sau không phá):

- **Không sửa parser để ép xanh.** Nếu một assert tầng 2 fail sau merge,
  giữ nguyên assert, báo fixture + assert + hành vi thực tế dưới dạng
  finding, không lặng lẽ bỏ assert.
- **Chỉ thêm file** ở tầng khung: design doc, test, fixture. Không sửa
  `engine/src/stream_json.rs`, không commit `docs/stream-json-v1.md`
  (tài liệu hợp đồng đó chờ quyết riêng của Leader về việc land).
- Fixture mới chỉ thêm khi phát hiện case cứng chưa có fixture; thêm thì
  chỉ thêm file, không đổi parser.

## 3. Tầng 1 tối nay kiểm tra những gì (20 test, mức fixture)

API parser thật (đọc từ tuyến DV-2, chưa có trên main) để tham chiếu:
`parse_stream_json(&str) -> anyhow::Result<Vec<CliLine>>`,
`to_desktop_events(&CliLine) -> Vec<DesktopEvent>`,
`interpret_exit_code(i32) -> CliExit`,
`CliLine { v: u64, line_type: String, event: CliStreamEvent }`.
Tầng 1 **không gọi** các API này; nó kiểm tra điều kiện đầu vào mà các
assert tầng 2 sẽ dựa vào, nên tầng 2 không thể "xanh nhờ fixture sai".

| Yêu cầu hợp đồng CỨNG | Test tầng 1 | Fixture |
|---|---|---|
| Mọi record mang `v = 1` | `all_valid_fixtures_carry_schema_v1` | 01–09, 11, 12, 18–22 |
| NDJSON: mỗi dòng là một object | `records()` trong mọi test (panic kèm dòng vật lý) | tất cả |
| Bỏ dòng trống, dòng trống không đổi chuỗi record | `blank_lines_yield_no_records` | 13 (rỗng), 14 (3 dòng trống → 0 record) |
| Mọi record có `type` | `every_valid_record_has_a_type` | tất cả hợp lệ |
| Không yêu cầu `seq` | `no_valid_fixture_requires_seq` | tất cả hợp lệ (không record nào có `seq`) |
| `system/init` mang `tools`, `model`, `session_id` | `system_init_carries_tools_and_model` | 01 |
| Shape tool-call OpenAI: `message.tool_calls[]` với `function.arguments` là chuỗi JSON-encoded | `assistant_tool_calls_are_openai_shape` | 03 |
| Message chỉ gọi tool thì lược `content` (không gửi null) | `assistant_tool_calls_without_text_omit_content` | 11 |
| Multi tool call giữ thứ tự | `assistant_multiple_tool_calls_keep_order` | 19 (read rồi bash) |
| `user` tool_result: `tool_use_id`, `is_error`, giữ thứ tự | `user_tool_results_shape_and_order` | 04, 05, 21 |
| `result` mang đủ đầu vào chiếu thành `SessionEnd` (`subtype`, `is_error`, `num_turns`, `usage`, `duration_ms`, `model`) | `result_lines_carry_session_end_inputs` | 06, 07, 12, 20 |
| Record type lạ không bị drop khỏi stream | `unknown_type_record_is_preserved_in_stream` | 09 (`heartbeat`, còn nguyên trường `at`) |
| Trường chưa biết trên record đã biết được bỏ qua (additive) | `unknown_fields_on_known_types_are_tolerated` | **22 (mới)** |
| `v` khác 1 bị từ chối | `bad_version_is_rejected_input` | 10 (`v = 2`) |
| Thiếu `v` bị từ chối | `missing_version_is_rejected_input` | 16 |
| JSON lỗi: xác định được dòng vật lý | `malformed_json_fails_on_physical_line_1` | 15 (dòng 1) |
| Stream đứt: dòng trước hợp lệ, dòng đứt xác định được | `truncated_stream_fails_on_physical_line_3` | 17 (dòng 3 đứt, 2 dòng đầu hợp lệ) |
| Không newline cuối vẫn là một record hoàn chỉnh | `final_line_without_trailing_newline_is_a_complete_record` | 18 |
| Phiên đầy đủ giữ thứ tự system → assistant → assistant → user → result | `full_session_order_is_preserved` | 08 |
| Báo cáo từng fixture | `fixture_inventory_report` | tất cả |

Fixture mới duy nhất tối nay — `22_unknown_field_on_known_type.jsonl`:
case cứng "unknown field bỏ qua" trước đây **chưa có fixture nào bao phủ
trên record đã biết** (fixture 09 chỉ có trường lạ trên type lạ). Fixture 22
gồm một `system/init` mang `future_field` + `another_unknown`, và một
`result` mang `future_result_field`. Ở tầng 1, test khẳng định stream vẫn
nguyên vẹn; ở tầng 2, test sẽ khẳng định parser serde bỏ qua các trường
đó mà vẫn parse đúng (struct serde mặc định bỏ qua trường chưa biết).

## 4. Tầng 2 — gắn parser thật ngay sau khi DV-2 merge (đặc tả sẵn)

Khi PR #3 merge, `engine/src/stream_json.rs` + `engine/src/lib.rs`
(`pub mod stream_json;`) xuất hiện trên main. Việc cần làm là rebase nhánh
DV-4 lên main mới rồi thêm các test sau vào `engine/tests/contract_harness.rs`
(không sửa test tầng 1, không sửa parser):

- `parse_stream_json` trên từng fixture hợp lệ trả đúng số `CliLine`, `v = 1`,
  `line_type` đúng thứ tự (đặc biệt 08: 5 dòng).
- Fixture 09: `line.line_type == "heartbeat"` và
  `to_desktop_events(&line) == [DesktopEvent::Unknown { variant:
  "cli_stream:heartbeat" }]` — đúng 1 event, không drop.
- Fixture 17: `parse_stream_json` trả `Err` (fail-fast: 0 event, không trả
  partial), message chứa `line 3` và `invalid JSON`.
- Fixture 15: `Err` chứa `line 1` và `invalid JSON`.
- Fixture 10/16: `Err` chứa `unsupported schema version`.
- Fixture 14/13: `Ok` với vec rỗng. Test blank-line kèm dòng trống xen giữa
  đã có ở tuyến DV-2 (`blank_lines_are_ignored`); tầng 2 giữ lại nguyên test đó.
- Chiếu `result` → `SessionEnd`: 06 cho `success = true`, `num_turns = 3`,
  `input_tokens = 1200`, `output_tokens = 340`, `duration_ms = 45210`,
  `model = ""` (vắng model mặc định rỗng); 07 cho `success = false`,
  result chứa `429`; 12 cho `model = "cohere/north-mini-code:free"`;
  20 cho `success = false` + model đúng.
- Chiếu `system/init` → `SystemMessage { event: "cli_system:init" }` chứa
  `ses_abc123` và model; `assistant` → `AssistantMessage` với text và
  `tool_calls` đúng id/name (03: `call_read_01`/`read`; 19: 2 call theo thứ
  tự; 11: `text = None`, 1 call `call_task_1`/`Task`); `user` → `ToolResult`
  đúng `tool_use_id`/`is_error`/thứ tự (21: `toolu_11` ok rồi `toolu_12` lỗi).
- Exit codes: `interpret_exit_code(0) = Success`, `(1) = Error`,
  `(130) = Interrupted`, `(143) = Terminated`, còn lại `Other(c)`.
- Chịu thay đổi additive ở tầng parser: fixture 22 parse `Ok`, record đầu
  là `System { subtype: "init", .. }`, record cuối là `Result` success —
  các trường tương lai không làm đổi kết quả.
- Projection không mất field ở mức khung kiểm tra được: mọi `CliLine` đều
  chiếu thành ít nhất 1 `DesktopEvent` (không có dòng nào chiếu thành 0
  event — kể cả unknown type). Đây là tính chất "surface, never silently
  change" của parser.

Sau khi tầng 2 xanh, khung mới đủ điều kiện gọi là "harness phía parser"
theo đúng nghĩa, và mới được đưa vào nghiệm thu tầng parser.

## 5. Tầng 3 — ghim CLI hai phía (XONG lần chạy đầu tiên 30/10/2026, xem docs/dv4-tier3-runbook.md)

Đây là phần "ghim" đúng nghĩa của DV-4, và là phần **chưa được làm tối
nay** vì nó cần cả hai đầu đều có mặt trên cùng một main: CLI thật sinh
đúng fixture, và parser Desktop đọc đúng fixture đó.

- **Phía CLI (ghim binary thật sinh đúng fixture):** chạy binary CLI thật
  (`zen -p --output-format stream-json`) với mock LLM, thu stdout, đối chiếu
  từng dòng với fixture tương ứng trong `engine/tests/fixtures/stream_json/`
  (kế thừa cơ chế `crates/zen-cli/tests/stream_json_contract.rs` phía CLI:
  fixture lệch khỏi output thật thì test fail; thay đổi có chủ đích thì
  regenerate fixture + cập nhật `docs/stream-json-v1.md` trong cùng PR).
  Khung Desktop ở tầng này không tự sinh fixture mới từ binary lạ — fixture
  là chuẩn, binary phải khớp fixture, không ngược lại.
- **Phía Desktop (ghim parser):** tầng 2 (§4) chạy trên chính các fixture
  mà phía CLI vừa chứng minh là output thật. Một thay đổi CLI làm lệch
  fixture sẽ fail ở phía CLI trước, không bao giờ lọt xuống parser dưới
  dạng "fixture mới lặng lẽ khớp parser mới".
- **Điểm ghim hai phía:** cùng một bộ fixture + cùng một hợp đồng
  `docs/stream-json-v1.md`. Khi hợp đồng đổi (v bump, record mới, field
  mới bắt buộc), cả hai phía phải đổi cùng PR: cập nhật doc, regenerate
  fixture phía CLI, chạy lại tầng 2 phía Desktop.

Trình tự sau DV-2 merge: (1) rebase nhánh DV-4 lên main mới; (2) thêm test
tầng 2 (§4), chạy xanh; (3) nối harness phía CLI (chạy binary thật, đối
chiếu fixture) — bước này cần quyết của Leader về nơi chạy binary CLI
(trên box hay CI) và về mock LLM dùng chung; (4) báo nghiệm thu kèm log
cả hai phía, chờ ZC Leader chấp nhận bằng văn bản. **Không tự nhận DONE
trước bước (4).**

## 6. 7 khoảng fidelity — follow-up, KHÔNG assert trong khung tối nay

Theo đối chiếu B-3, 7 khoảng sau là mất field/khác biệt giữa parser hiện tại
và hợp đồng đầy đủ; chúng là mục kiểm tra tương lai, không phải assert
của khung tối nay, và không được dùng để ép xanh hay ép đỏ:

1. `parent_tool_use_id` (assistant/user từ sub-agent) — parser hiện chiếu
   thành `None` ở cả hai phía.
2. `parallel` trên `tool_result` — parser không giữ trường này.
3. `system` subtype ngoài `init` (`api_retry`, `permission_denied`,
   `hook_message`, `compact_boundary`, `prune`, `tool_progress`) — parser
   gộp thành `SystemMessage` dạng text, không chiếu sang các variant
   `ApiRetry`/`PermissionDenied`/`CompactBoundary` chuyên biệt của Desktop.
4. `result`: `duration_api_ms`, `permission_denials`, `stop_reason` —
   parser không đọc các trường này.
5. `usage.cache_read_input_tokens` — `StreamUsage` chỉ có input/output.
6. `uuid` / `session_id` trên mọi record — parser chỉ giữ `session_id` ở
   `system`/`result`, không giữ `uuid`.
7. `stream_event` (delta khi `--include-partial-messages`) — parser không có
   variant riêng; hiện rơi vào `Unknown` (không drop, nhưng không phải là
   chiếu delta đúng nghĩa; consumer phải thay delta bằng message đầy đủ).

Khi một khoảng được xử lý, thêm fixture + test tương ứng, ghi rõ vào bảng
§3, và chỉ khi đó nó mới thành assert cứng.

## 7. Còn thiếu cho mốc 19:30 31/10/2026

- [x] Nhánh `dev/dv4-contract-harness` từ `aeaf42d`, commit local (chưa push).
- [x] Design doc này.
- [x] Skeleton tầng 1 xanh trên main (20 test, xem §8 bằng chứng chạy).
- [x] Fixture 22 cho case cứng chưa có fixture.
- [ ] **Tầng 2 (parser thật) — chặn bởi DV-2 chưa merge (PR #3).** Đặc tả sẵn
      ở §4; sau merge chỉ việc thêm test + chạy.
- [x] **Tầng 3 (ghim CLI hai phía) — chạy đầu tiên 30/10/2026 xong, xem
      docs/dv4-tier3-runbook.md (binary ~/muse2/zen @ 290154c, stub localhost,
      parser check 1 passed). Nghiệm thu tầng 3 vẫn chờ văn bản của ZC Leader (§5).**
- [ ] Nghiệm thu bằng văn bản của ZC Leader — chỉ xin sau khi tầng 2 xanh
      (tầng 3 có thể nghiệm thu riêng sau đó nếu Leader tách mốc).
- [ ] Quyết riêng về việc land `docs/stream-json-v1.md` (file untracked,
      cố tình không commit trong khung này).

Tiến độ harness báo tại 19:30 02/11/2026 sẽ cập nhật lại bảng này theo trạng
thái merge của PR #3.

## 8. Bằng chứng chạy tầng 1 (ghi lúc commit, xem log đính kèm khi báo cáo)

Lệnh: `cd engine && cargo test --offline --locked --test contract_harness`
(kèm `-- --nocapture` cho báo cáo từng fixture). Kết quả chi tiết — lệnh,
exit code, số pass/fail — nằm trong báo cáo nghiệm thu của khung, không
ghi số miệng vào tài liệu này để tránh lệch với log thật.
