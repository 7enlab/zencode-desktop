//! Integration tests for the CLI stream-json v1 parser (DV-2).
//!
//! Fixtures live in `engine/tests/fixtures/stream_json/` as `.jsonl` files,
//! one scenario each. They encode the CLI/Desktop contract v1 per the
//! authoritative `docs/stream-json-v1.md` (CLI team, zen 0.1.0), reconciled
//! 2026-10-23: assistant `content` is a string (omitted when only calling
//! tools), tool calls are OpenAI-style `message.tool_calls` with JSON-encoded
//! `arguments`, and the `result` line carries `model`.

use std::path::PathBuf;

use zencode_engine::events::DesktopEvent;
use zencode_engine::stream_json::{
    interpret_exit_code, parse_stream_json, to_desktop_events, CliExit, CliLine,
};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stream_json")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

fn parse_fixture(name: &str) -> Vec<CliLine> {
    parse_stream_json(&fixture(name)).unwrap_or_else(|e| panic!("parse fixture {name}: {e}"))
}

fn parse_single(name: &str) -> CliLine {
    let lines = parse_fixture(name);
    assert_eq!(lines.len(), 1, "fixture {name} should hold exactly one line");
    lines.into_iter().next().unwrap()
}

fn project(name: &str) -> Vec<DesktopEvent> {
    parse_fixture(name)
        .iter()
        .flat_map(to_desktop_events)
        .collect()
}

#[test]
fn parses_system_init() {
    let line = parse_single("01_system_init.jsonl");
    assert_eq!(line.v, 1);
    assert_eq!(line.line_type, "system");
    let events = to_desktop_events(&line);
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SystemMessage { event, text } => {
            assert_eq!(event, "cli_system:init");
            assert!(text.contains("ses_abc123"), "{text}");
            assert!(text.contains("cohere/north-mini-code:free"), "{text}");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_assistant_text() {
    let events = project("02_assistant_text.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::AssistantMessage {
            text,
            tool_calls,
            parent_tool_use_id,
        } => {
            assert_eq!(text.as_deref(), Some("Tôi sẽ đọc file cấu hình trước."));
            assert!(tool_calls.is_empty());
            assert_eq!(parent_tool_use_id, &None);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_assistant_tool_use() {
    let events = project("03_assistant_tool_use.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::AssistantMessage { text, tool_calls, .. } => {
            assert_eq!(text.as_deref(), Some("Đọc file:"));
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "call_read_01");
            assert_eq!(tool_calls[0].name, "read");
            assert!(tool_calls[0].arguments.contains("tauri.conf.json"), "{}", tool_calls[0].arguments);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_user_tool_result() {
    let events = project("04_user_tool_result.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "toolu_01");
            assert!(content.contains("zencode-desktop"), "{content}");
            assert!(!is_error);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_user_tool_result_error() {
    let events = project("05_user_tool_result_error.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::ToolResult {
            tool_use_id, is_error, ..
        } => {
            assert_eq!(tool_use_id, "toolu_02");
            assert!(is_error);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_result_success() {
    let events = project("06_result_success.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SessionEnd {
            success,
            result,
            num_turns,
            total_cost_usd,
            input_tokens,
            output_tokens,
            duration_ms,
            ..
        } => {
            assert!(success);
            assert_eq!(result, "Đã cập nhật cấu hình xong.");
            assert_eq!(*num_turns, 3);
            assert_eq!(*total_cost_usd, 0.0);
            assert_eq!(*input_tokens, 1200);
            assert_eq!(*output_tokens, 340);
            assert_eq!(*duration_ms, 45210);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_result_error() {
    let events = project("07_result_error.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SessionEnd { success, result, .. } => {
            assert!(!success);
            assert!(result.contains("429"), "{result}");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_full_session_in_order() {
    let events = project("08_full_session.jsonl");
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            DesktopEvent::SystemMessage { .. } => "system",
            DesktopEvent::AssistantMessage { .. } => "assistant",
            DesktopEvent::ToolResult { .. } => "tool_result",
            DesktopEvent::SessionEnd { .. } => "session_end",
            other => panic!("unexpected event in full session: {other:?}"),
        })
        .collect();
    assert_eq!(kinds, ["system", "assistant", "assistant", "tool_result", "session_end"]);
    match events.last().unwrap() {
        DesktopEvent::SessionEnd { success, num_turns, .. } => {
            assert!(success);
            assert_eq!(*num_turns, 2);
        }
        other => panic!("unexpected final event: {other:?}"),
    }
}

#[test]
fn unknown_line_type_is_surfaced_not_dropped() {
    let line = parse_single("09_unknown_type.jsonl");
    assert_eq!(line.line_type, "heartbeat");
    let events = to_desktop_events(&line);
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::Unknown { variant } => assert!(variant.contains("heartbeat"), "{variant}"),
        other => panic!("unknown line type must surface as Unknown, got: {other:?}"),
    }
}

#[test]
fn rejects_unsupported_schema_version() {
    let err = parse_stream_json(&fixture("10_bad_version.jsonl")).unwrap_err();
    assert!(err.to_string().contains("unsupported schema version"), "{err}");
}

#[test]
fn blank_lines_are_ignored() {
    let lines = parse_stream_json("\n\n  \n{\"v\":1,\"type\":\"system\",\"subtype\":\"init\"}\n\n").unwrap();
    assert_eq!(lines.len(), 1);
}

#[test]
fn exit_codes_follow_contract_v1() {
    assert_eq!(interpret_exit_code(0), CliExit::Success);
    assert_eq!(interpret_exit_code(1), CliExit::Error);
    assert_eq!(interpret_exit_code(130), CliExit::Interrupted);
    assert_eq!(interpret_exit_code(143), CliExit::Terminated);
    assert_eq!(interpret_exit_code(2), CliExit::Other(2));
    assert_eq!(interpret_exit_code(-1), CliExit::Other(-1));
}

#[test]
fn parses_assistant_tool_calls_without_text() {
    // content is omitted when the message only calls tools (docs v1).
    let events = project("11_assistant_tool_calls_no_text.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::AssistantMessage { text, tool_calls, .. } => {
            assert_eq!(text, &None);
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "call_task_1");
            assert_eq!(tool_calls[0].name, "Task");
            assert!(
                tool_calls[0].arguments.contains("Explore"),
                "{}",
                tool_calls[0].arguments
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_result_model() {
    let events = project("12_result_with_model.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SessionEnd { success, model, .. } => {
            assert!(success);
            assert_eq!(model, "cohere/north-mini-code:free");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn result_without_model_defaults_to_empty() {
    let events = project("06_result_success.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SessionEnd { model, .. } => assert_eq!(model, ""),
        other => panic!("unexpected event: {other:?}"),
    }
}
