use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::events::{DesktopEvent, Envelope};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {

    pub step: u64,

    pub seq: u64,

    pub kind: String,

    pub title: String,

    pub detail: String,

    pub files: Vec<String>,

    pub checkpoint_id: Option<String>,

    pub undoable: bool,

    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineCheckpoint {
    pub id: String,
    pub message: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineResponse {
    pub session_id: String,
    pub entries: Vec<TimelineEntry>,
    pub checkpoints: Vec<TimelineCheckpoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestoreOutcome {
    pub restored_files: Vec<String>,

    pub safety_checkpoint_id: String,
}

pub fn build_timeline(envelopes: &[Envelope]) -> Vec<TimelineEntry> {

    let mut checkpoint_for_seq: HashMap<u64, String> = HashMap::new();
    for env in envelopes {
        if let DesktopEvent::SystemMessage { event, text } = &env.event {
            if event == "checkpoint" {
                if let Some((id, seq)) = parse_checkpoint_note(text) {
                    checkpoint_for_seq.insert(seq, id);
                }
            }
        }
    }

    let mut entries: Vec<TimelineEntry> = Vec::new();
    let mut by_tool_use: HashMap<String, usize> = HashMap::new();
    let mut step: u64 = 0;

    for env in envelopes {
        match &env.event {
            DesktopEvent::AssistantMessage { tool_calls, .. } => {
                for tc in tool_calls {
                    step += 1;
                    let kind = kind_for_tool(&tc.name);
                    let files = files_for_tool(&tc.name, &tc.arguments);
                    let title = title_for_tool(&tc.name, &files, &tc.arguments);
                    let entry = TimelineEntry {
                        step,
                        seq: env.seq,
                        kind,
                        title,
                        detail: "running…".to_string(),
                        files,
                        checkpoint_id: checkpoint_for_seq.get(&env.seq).cloned(),
                        undoable: false,
                        status: "pending".to_string(),
                    };
                    by_tool_use.insert(tc.id.clone(), entries.len());
                    entries.push(entry);
                }
            }
            DesktopEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
                ..
            } => {
                if let Some(&idx) = by_tool_use.get(tool_use_id) {
                    let e = &mut entries[idx];
                    e.status = if *is_error { "error" } else { "ok" }.to_string();
                    e.detail = first_line(content, 140);
                }
            }
            DesktopEvent::PermissionDenied { tool_name, .. } => {
                step += 1;
                entries.push(TimelineEntry {
                    step,
                    seq: env.seq,
                    kind: "blocked".to_string(),
                    title: format!("Blocked: {tool_name} needed your approval"),
                    detail: "the agent did not run this - waiting on you".to_string(),
                    files: vec![],
                    checkpoint_id: None,
                    undoable: false,
                    status: "blocked".to_string(),
                });
            }
            DesktopEvent::SystemMessage { event, text } => {
                if event == "checkpoint" {

                    continue;
                }
                if event == "session_start" || event == "session_end" {
                    step += 1;
                    entries.push(TimelineEntry {
                        step,
                        seq: env.seq,
                        kind: "system".to_string(),
                        title: if event == "session_start" {
                            "Session started".to_string()
                        } else {
                            "Session ended".to_string()
                        },
                        detail: first_line(text, 140),
                        files: vec![],
                        checkpoint_id: None,
                        undoable: false,
                        status: "info".to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    for e in entries.iter_mut() {
        e.undoable = e.checkpoint_id.is_some();
        if e.status == "pending" && e.kind != "blocked" {
            e.detail = "no result recorded".to_string();
        }
    }
    entries
}

fn parse_checkpoint_note(text: &str) -> Option<(String, u64)> {
    let text = text.trim();
    let rest = text.strip_prefix("checkpoint ")?;
    let (id, rest) = rest.split_once(" before seq ")?;
    let seq: u64 = rest.trim().parse().ok()?;
    let id = id.trim();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((id.to_string(), seq))
}

fn kind_for_tool(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        n if n.contains("write") => "write".to_string(),
        n if n.contains("edit") => "edit".to_string(),
        n if n.contains("bash") || n.contains("shell") || n.contains("exec") => "bash".to_string(),
        n if n.contains("read") => "read".to_string(),
        _ => "tool".to_string(),
    }
}

fn files_for_tool(name: &str, arguments: &str) -> Vec<String> {
    let args: serde_json::Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    if kind_for_tool(name) == "bash" {
        return vec![];
    }
    let mut files = Vec::new();
    for key in ["path", "file", "file_path", "filePath", "filename"] {
        if let Some(p) = args.get(key).and_then(|v| v.as_str()) {
            let p = p.trim();
            if !p.is_empty() && !files.contains(&p.to_string()) {
                files.push(p.to_string());
            }
        }
    }
    files
}

fn title_for_tool(name: &str, files: &[String], arguments: &str) -> String {
    let file = files.first().map(|f| format!("`{f}`")).unwrap_or_default();
    match kind_for_tool(name).as_str() {
        "write" => format!("Wrote {file}").trim_end().to_string(),
        "edit" => format!("Edited {file}").trim_end().to_string(),
        "read" => format!("Read {file}").trim_end().to_string(),
        "bash" => {
            let cmd = serde_json::from_str::<serde_json::Value>(arguments)
                .ok()
                .and_then(|v| {
                    v.get("command")
                        .or_else(|| v.get("cmd"))
                        .and_then(|c| c.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let short = first_line(&cmd, 60);
            if short.is_empty() {
                "Ran a shell command".to_string()
            } else {
                format!("Ran `{short}`")
            }
        }
        _ => format!("Used {name}"),
    }
}

fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() > max {
        let cut: String = line.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Envelope, ToolCallInfo, SCHEMA_VERSION};

    fn env(seq: u64, event: DesktopEvent) -> Envelope {
        Envelope {
            v: SCHEMA_VERSION,
            seq,
            event,
        }
    }

    fn assistant_with_tool(seq: u64, id: &str, name: &str, args: &str) -> Envelope {
        env(
            seq,
            DesktopEvent::AssistantMessage {
                text: None,
                tool_calls: vec![ToolCallInfo {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments: args.to_string(),
                }],
                parent_tool_use_id: None,
            },
        )
    }

    #[test]
    fn edit_entry_with_result() {
        let log = vec![
            assistant_with_tool(0, "t1", "Edit", r#"{"path":"src/main.rs"}"#),
            env(
                1,
                DesktopEvent::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: "replaced 1 occurrence".to_string(),
                    is_error: false,
                    parent_tool_use_id: None,
                },
            ),
        ];
        let tl = build_timeline(&log);
        assert_eq!(tl.len(), 1);
        assert_eq!(tl[0].kind, "edit");
        assert_eq!(tl[0].title, "Edited `src/main.rs`");
        assert_eq!(tl[0].status, "ok");
        assert_eq!(tl[0].detail, "replaced 1 occurrence");
        assert_eq!(tl[0].step, 1);
        assert!(!tl[0].undoable);
    }

    #[test]
    fn checkpoint_link_makes_entry_undoable() {
        let log = vec![
            env(
                0,
                DesktopEvent::SystemMessage {
                    event: "checkpoint".to_string(),
                    text: "checkpoint abc1234 before seq 1".to_string(),
                },
            ),
            assistant_with_tool(1, "t1", "Write", r#"{"path":"a.txt"}"#),
        ];
        let tl = build_timeline(&log);
        assert_eq!(tl.len(), 1);
        assert_eq!(tl[0].checkpoint_id.as_deref(), Some("abc1234"));
        assert!(tl[0].undoable);
    }

    #[test]
    fn malformed_checkpoint_note_ignored() {
        assert_eq!(parse_checkpoint_note("checkpoint before seq"), None);
        assert_eq!(parse_checkpoint_note("checkpoint zzz before seq 3"), None);
        assert_eq!(parse_checkpoint_note("checkpoint abc before seq x"), None);
        assert_eq!(
            parse_checkpoint_note("checkpoint deadbee before seq 7"),
            Some(("deadbee".to_string(), 7))
        );
    }

    #[test]
    fn blocked_and_error_statuses() {
        let log = vec![
            env(
                0,
                DesktopEvent::PermissionDenied {
                    tool_name: "Bash".to_string(),
                    tool_use_id: "t9".to_string(),
                },
            ),
            assistant_with_tool(1, "t2", "Bash", r#"{"command":"rm -rf /"}"#),
            env(
                2,
                DesktopEvent::ToolResult {
                    tool_use_id: "t2".to_string(),
                    content: "boom".to_string(),
                    is_error: true,
                    parent_tool_use_id: None,
                },
            ),
        ];
        let tl = build_timeline(&log);
        assert_eq!(tl.len(), 2);
        assert_eq!(tl[0].kind, "blocked");
        assert_eq!(tl[0].status, "blocked");
        assert_eq!(tl[1].kind, "bash");
        assert_eq!(tl[1].title, "Ran `rm -rf /`");
        assert_eq!(tl[1].status, "error");
    }

    #[test]
    fn files_extracted_from_common_arg_keys() {
        assert_eq!(
            files_for_tool("Edit", r#"{"file_path":"x.rs","other":1}"#),
            vec!["x.rs"]
        );
        assert_eq!(
            files_for_tool("Bash", r#"{"command":"ls"}"#),
            Vec::<String>::new()
        );
        assert_eq!(files_for_tool("Edit", "not json"), Vec::<String>::new());
    }

    #[test]
    fn timeline_response_serializes() {
        let r = TimelineResponse {
            session_id: "s1".to_string(),
            entries: vec![],
            checkpoints: vec![TimelineCheckpoint {
                id: "abc".to_string(),
                message: "m".to_string(),
                timestamp: 0,
            }],
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("checkpoints"));
    }
}
