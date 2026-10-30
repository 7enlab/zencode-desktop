//! Khung harness contract-test DV-4 — tầng 1 (chạy được tối nay, 30/10/2026).
//!
//! Tầng 1 kiểm tra các yêu cầu hợp đồng CỨNG ở mức fixture (JSON thô) bằng
//! `serde_json` — không phụ thuộc parser, vì parser `engine/src/stream_json.rs`
//! chưa có trên main `aeaf42d` (nó nằm ở tuyến DV-2, xem design doc).
//!
//! Tầng 2 (gắn parser thật) được đặc tả đầy đủ trong
//! `docs/dv4-contract-harness-design.md` và chỉ kích hoạt được sau khi PR #3
//! (DV-2) merge: khi đó `zencode_engine::stream_json::{parse_stream_json,
//! to_desktop_events, interpret_exit_code}` tồn tại và các assert tầng 2 sau
//! đây được thêm nguyên vẹn, không sửa assert tầng 1:
//!   - unknown type -> `DesktopEvent::Unknown`, không drop (fixture 09);
//!   - stream đứt -> `parse_stream_json` trả `Err`, tức 0 event (fixture 17);
//!   - lỗi đánh số dòng vật lý, kể cả dòng trống (fixture 15: dòng 1, 17: dòng 3);
//!   - result -> `SessionEnd` với `success = !is_error && subtype == "success"`,
//!     model, usage, duration (fixture 06/07/12/20);
//!   - exit codes 0/1/130/143 -> Success/Error/Interrupted/Terminated.
//!
//! Chạy: `cargo test --offline --locked --test contract_harness`
//! Báo cáo từng fixture: `cargo test --offline --locked --test contract_harness
//! -- --nocapture` (test `fixture_inventory_report` in một dòng mỗi fixture).

use std::path::PathBuf;

use serde_json::Value;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stream_json")
        .join(name)
}

fn fixture_text(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("đọc fixture {}: {e}", path.display()))
}

/// Các record JSON của một fixture: bỏ qua dòng trống, giữ nguyên thứ tự.
/// Panic kèm số dòng vật lý (bắt đầu từ 1) nếu một dòng không phải JSON object.
fn records(name: &str) -> Vec<Value> {
    let text = fixture_text(name);
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("fixture {name} dòng {}: JSON lỗi: {e}", i + 1));
        assert!(
            v.is_object(),
            "fixture {name} dòng {}: mỗi dòng NDJSON phải là một object",
            i + 1
        );
        out.push(v);
    }
    out
}

/// Dòng vật lý đầu tiên parse JSON lỗi trong một fixture, kèm số dòng (từ 1).
/// Trả `None` nếu mọi dòng (kể cả dòng trống) đều parse được hoặc rỗng.
fn first_bad_line(name: &str) -> Option<(usize, String)> {
    for (i, line) in fixture_text(name).lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if let Err(e) = serde_json::from_str::<Value>(line) {
            return Some((i + 1, e.to_string()));
        }
    }
    None
}

fn record_type(v: &Value) -> &str {
    v.get("type")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("record thiếu trường type: {v}"))
}

/// Các fixture hợp lệ (mọi dòng đều là JSON object, v = 1).
const VALID_FIXTURES: &[&str] = &[
    "01_system_init.jsonl",
    "02_assistant_text.jsonl",
    "03_assistant_tool_use.jsonl",
    "04_user_tool_result.jsonl",
    "05_user_tool_result_error.jsonl",
    "06_result_success.jsonl",
    "07_result_error.jsonl",
    "08_full_session.jsonl",
    "09_unknown_type.jsonl",
    "11_assistant_tool_calls_no_text.jsonl",
    "12_result_with_model.jsonl",
    "18_no_trailing_newline.jsonl",
    "19_multiple_tool_calls.jsonl",
    "20_result_error_with_model.jsonl",
    "21_user_two_tool_results.jsonl",
    "22_unknown_field_on_known_type.jsonl",
];

#[test]
fn all_valid_fixtures_carry_schema_v1() {
    for name in VALID_FIXTURES {
        for (idx, rec) in records(name).iter().enumerate() {
            assert_eq!(
                rec.get("v").and_then(Value::as_u64),
                Some(1),
                "fixture {name} record {idx}: v phải bằng 1, thực tế: {rec}"
            );
        }
    }
}

#[test]
fn every_valid_record_has_a_type() {
    for name in VALID_FIXTURES {
        for rec in records(name) {
            let _ = record_type(&rec);
        }
    }
}

#[test]
fn no_valid_fixture_requires_seq() {
    // Hợp đồng v1: không có trường seq; consumer tự đánh số dòng nếu cần.
    for name in VALID_FIXTURES {
        for rec in records(name) {
            assert!(
                rec.get("seq").is_none(),
                "fixture {name}: record mang trường seq, trái hợp đồng v1: {rec}"
            );
        }
    }
}

#[test]
fn blank_lines_yield_no_records() {
    assert!(records("13_empty_input.jsonl").is_empty());
    let blank = fixture_text("14_blank_lines_only.jsonl");
    assert_eq!(blank.lines().count(), 3, "fixture 14 phải có 3 dòng vật lý");
    assert!(records("14_blank_lines_only.jsonl").is_empty());
}

#[test]
fn system_init_carries_tools_and_model() {
    let recs = records("01_system_init.jsonl");
    assert_eq!(recs.len(), 1);
    let rec = &recs[0];
    assert_eq!(record_type(rec), "system");
    assert_eq!(rec.get("subtype").and_then(Value::as_str), Some("init"));
    assert_eq!(
        rec.get("session_id").and_then(Value::as_str),
        Some("ses_abc123")
    );
    assert_eq!(
        rec.get("model").and_then(Value::as_str),
        Some("cohere/north-mini-code:free")
    );
    let tools = rec
        .get("tools")
        .and_then(Value::as_array)
        .expect("system/init phải có mảng tools");
    let names: Vec<&str> = tools.iter().map(|t| t.as_str().unwrap()).collect();
    assert_eq!(names, ["read", "write", "bash"]);
}

#[test]
fn assistant_text_shape() {
    let recs = records("02_assistant_text.jsonl");
    assert_eq!(recs.len(), 1);
    let blocks = recs[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .expect("02: message.content phải là mảng block");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].get("type").and_then(Value::as_str), Some("text"));
    assert_eq!(
        blocks[0].get("text").and_then(Value::as_str),
        Some("Tôi sẽ đọc file cấu hình trước.")
    );
}

#[test]
fn assistant_tool_calls_are_openai_shape() {
    // Shape CỨNG: message.tool_calls, mỗi call {id, type: "function",
    // function: {name, arguments}} với arguments là chuỗi JSON-encoded.
    let recs = records("03_assistant_tool_use.jsonl");
    let msg = recs[0].get("message").expect("03: thiếu message");
    assert_eq!(
        msg.get("content").and_then(Value::as_str),
        Some("Đọc file:")
    );
    let calls = msg
        .get("tool_calls")
        .and_then(Value::as_array)
        .expect("03: thiếu message.tool_calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("id").and_then(Value::as_str),
        Some("call_read_01")
    );
    assert_eq!(
        calls[0].get("type").and_then(Value::as_str),
        Some("function")
    );
    let fun = calls[0].get("function").expect("03: thiếu function");
    assert_eq!(fun.get("name").and_then(Value::as_str), Some("read"));
    let args = fun
        .get("arguments")
        .and_then(Value::as_str)
        .expect("03: arguments phải là chuỗi JSON-encoded");
    let parsed: Value = serde_json::from_str(args).expect("03: arguments phải parse được");
    assert_eq!(
        parsed.get("path").and_then(Value::as_str),
        Some("tauri.conf.json")
    );
}

#[test]
fn assistant_tool_calls_without_text_omit_content() {
    // Khi message chỉ gọi tool thì content bị lược (không gửi null).
    let recs = records("11_assistant_tool_calls_no_text.jsonl");
    let msg = recs[0].get("message").expect("11: thiếu message");
    assert!(
        msg.get("content").is_none(),
        "11: content phải bị lược, không phải null"
    );
    let calls = msg
        .get("tool_calls")
        .and_then(Value::as_array)
        .expect("11: thiếu message.tool_calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("id").and_then(Value::as_str),
        Some("call_task_1")
    );
    assert_eq!(
        calls[0]
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str),
        Some("Task")
    );
}

#[test]
fn assistant_multiple_tool_calls_keep_order() {
    let recs = records("19_multiple_tool_calls.jsonl");
    let calls = recs[0]
        .get("message")
        .and_then(|m| m.get("tool_calls"))
        .and_then(Value::as_array)
        .expect("19: thiếu message.tool_calls");
    assert_eq!(calls.len(), 2);
    let ids: Vec<&str> = calls
        .iter()
        .map(|c| c.get("id").and_then(Value::as_str).unwrap())
        .collect();
    let names: Vec<&str> = calls
        .iter()
        .map(|c| {
            c.get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap()
        })
        .collect();
    assert_eq!(ids, ["call_read_02", "call_bash_01"]);
    assert_eq!(names, ["read", "bash"]);
}

#[test]
fn user_tool_results_shape_and_order() {
    let one = records("04_user_tool_result.jsonl");
    let blocks = one[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .expect("04: thiếu content blocks");
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].get("type").and_then(Value::as_str),
        Some("tool_result")
    );
    assert_eq!(
        blocks[0].get("tool_use_id").and_then(Value::as_str),
        Some("toolu_01")
    );
    // is_error vắng mặt nghĩa là không lỗi (mặc định false ở tầng parser).
    assert!(blocks[0].get("is_error").is_none());

    let err = records("05_user_tool_result_error.jsonl");
    let eblocks = err[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .expect("05: thiếu content blocks");
    assert_eq!(
        eblocks[0].get("tool_use_id").and_then(Value::as_str),
        Some("toolu_02")
    );
    assert_eq!(
        eblocks[0].get("is_error").and_then(Value::as_bool),
        Some(true)
    );

    let two = records("21_user_two_tool_results.jsonl");
    let tblocks = two[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .expect("21: thiếu content blocks");
    assert_eq!(tblocks.len(), 2);
    assert_eq!(
        tblocks[0].get("tool_use_id").and_then(Value::as_str),
        Some("toolu_11")
    );
    assert_eq!(
        tblocks[1].get("tool_use_id").and_then(Value::as_str),
        Some("toolu_12")
    );
    assert_eq!(
        tblocks[1].get("is_error").and_then(Value::as_bool),
        Some(true)
    );
}

/// Đầu vào mà tầng parser (tầng 2) phải chiếu thành SessionEnd:
/// success suy ra từ `!is_error && subtype == "success"`.
fn session_end_inputs(rec: &Value) -> (bool, &str, &str) {
    let subtype = rec.get("subtype").and_then(Value::as_str).unwrap();
    let is_error = rec.get("is_error").and_then(Value::as_bool).unwrap();
    let model = rec.get("model").and_then(Value::as_str).unwrap_or("");
    (!is_error && subtype == "success", subtype, model)
}

#[test]
fn result_lines_carry_session_end_inputs() {
    let ok = records("06_result_success.jsonl");
    assert_eq!(ok.len(), 1);
    let (success, subtype, model) = session_end_inputs(&ok[0]);
    assert!(success);
    assert_eq!(subtype, "success");
    assert_eq!(ok[0].get("num_turns").and_then(Value::as_u64), Some(3));
    assert_eq!(
        ok[0]
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(Value::as_u64),
        Some(1200)
    );
    assert_eq!(
        ok[0]
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(Value::as_u64),
        Some(340)
    );
    assert_eq!(
        ok[0].get("duration_ms").and_then(Value::as_u64),
        Some(45210)
    );
    // Fixture 06 không mang model: tầng parser mặc định model rỗng (Level B).
    assert_eq!(model, "");

    let err = records("07_result_error.jsonl");
    let (success, _, _) = session_end_inputs(&err[0]);
    assert!(!success);
    assert!(err[0]
        .get("result")
        .and_then(Value::as_str)
        .unwrap()
        .contains("429"));

    let with_model = records("12_result_with_model.jsonl");
    let (success, _, model) = session_end_inputs(&with_model[0]);
    assert!(success);
    assert_eq!(model, "cohere/north-mini-code:free");

    let err_model = records("20_result_error_with_model.jsonl");
    let (success, _, model) = session_end_inputs(&err_model[0]);
    assert!(!success);
    assert_eq!(model, "cohere/north-mini-code:free");
    assert!(err_model[0]
        .get("result")
        .and_then(Value::as_str)
        .unwrap()
        .contains("429"));
}

#[test]
fn unknown_type_record_is_preserved_in_stream() {
    // Yêu cầu CỨNG ở mức stream: record lạ không bị drop khỏi chuỗi.
    // (Chiếu thành DesktopEvent::Unknown là assert tầng 2, sau merge DV-2.)
    let recs = records("09_unknown_type.jsonl");
    assert_eq!(recs.len(), 1);
    assert_eq!(record_type(&recs[0]), "heartbeat");
    assert!(recs[0].get("at").is_some(), "09: trường lạ phải còn nguyên");
}

#[test]
fn unknown_fields_on_known_types_are_tolerated() {
    // Thay đổi additive: trường chưa biết trên record đã biết không được
    // làm hỏng stream. Fixture 22 là fixture mới duy nhất của khung tối nay.
    let recs = records("22_unknown_field_on_known_type.jsonl");
    assert_eq!(recs.len(), 2);
    assert_eq!(record_type(&recs[0]), "system");
    assert!(recs[0].get("future_field").is_some());
    assert_eq!(
        recs[0].get("another_unknown").and_then(Value::as_u64),
        Some(123)
    );
    assert_eq!(record_type(&recs[1]), "result");
    assert_eq!(
        recs[1].get("future_result_field").and_then(Value::as_str),
        Some("ignore-me")
    );
}

#[test]
fn bad_version_is_rejected_input() {
    let recs_text = fixture_text("10_bad_version.jsonl");
    let v: Value = serde_json::from_str(recs_text.trim()).expect("10: phải là JSON");
    assert_eq!(v.get("v").and_then(Value::as_u64), Some(2));
    // v != 1 nên consumer v1 phải từ chối toàn bộ stream (assert tầng 2).
}

#[test]
fn missing_version_is_rejected_input() {
    let v: Value = serde_json::from_str(fixture_text("16_missing_version.jsonl").trim())
        .expect("16: phải là JSON");
    assert!(v.get("v").is_none(), "16: record thiếu trường v");
}

#[test]
fn malformed_json_fails_on_physical_line_1() {
    let (line, _) = first_bad_line("15_malformed_json.jsonl").expect("15: phải có dòng JSON lỗi");
    assert_eq!(line, 1);
}

#[test]
fn truncated_stream_fails_on_physical_line_3() {
    // Điều kiện đầu vào của fail-fast: 2 dòng đầu hợp lệ, dòng 3 đứt.
    // Hành vi fail-fast (trả Err, 0 event) là assert tầng 2, sau merge DV-2.
    let text = fixture_text("17_truncated_stream.jsonl");
    let mut good = 0;
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(_) => good += 1,
            Err(_) => {
                assert_eq!(i + 1, 3, "17: dòng đứt phải là dòng vật lý 3");
                assert_eq!(good, 2, "17: 2 dòng trước dòng đứt phải hợp lệ");
                return;
            }
        }
    }
    panic!("17: không tìm thấy dòng đứt");
}

#[test]
fn final_line_without_trailing_newline_is_a_complete_record() {
    let text = fixture_text("18_no_trailing_newline.jsonl");
    assert!(
        !text.ends_with(char::from(10)),
        "18: file khong duoc ket thuc bang newline"
    );
    let recs = records("18_no_trailing_newline.jsonl");
    assert_eq!(recs.len(), 1);
    assert_eq!(record_type(&recs[0]), "result");
    assert_eq!(recs[0].get("v").and_then(Value::as_u64), Some(1));
}

#[test]
fn full_session_order_is_preserved() {
    let recs = records("08_full_session.jsonl");
    let types: Vec<&str> = recs.iter().map(record_type).collect();
    assert_eq!(
        types,
        ["system", "assistant", "assistant", "user", "result"]
    );
    let last = recs.last().unwrap();
    assert_eq!(last.get("subtype").and_then(Value::as_str), Some("success"));
    assert_eq!(last.get("is_error").and_then(Value::as_bool), Some(false));
}

#[test]
fn fixture_inventory_report() {
    // Báo cáo từng fixture: chạy kèm `-- --nocapture` để thấy bảng này.
    // Mỗi dòng: tên fixture, số record (bỏ dòng trống), các type theo thứ tự.
    // Fixture lỗi (10/15/16/17) được báo riêng, không tính vào bảng record.
    let mut names: Vec<&str> = VALID_FIXTURES.to_vec();
    names.push("13_empty_input.jsonl");
    names.push("14_blank_lines_only.jsonl");
    for name in names {
        let recs = records(name);
        let types: Vec<&str> = recs.iter().map(record_type).collect();
        println!("fixture={name} records={} types={:?}", recs.len(), types);
    }
    // 10 và 16 là JSON hợp lệ nhưng bị từ chối ở tầng version (v=2 / thiếu v),
    // không phải ở tầng parse JSON — báo rõ lý do thay vì số dòng lỗi.
    println!("fixture=10_bad_version.jsonl rejected_input reason=unsupported_version v=2");
    let (line15, _) = first_bad_line("15_malformed_json.jsonl").expect("15: phải có dòng JSON lỗi");
    println!("fixture=15_malformed_json.jsonl rejected_input bad_line={line15}");
    println!("fixture=16_missing_version.jsonl rejected_input reason=missing_version");
    let (line, _) = first_bad_line("17_truncated_stream.jsonl").expect("17: phải có dòng đứt");
    println!("fixture=17_truncated_stream.jsonl rejected_input bad_line={line}");
}
