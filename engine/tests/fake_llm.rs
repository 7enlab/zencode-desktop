//! g1: full agent loop (HTTP client -> agent -> Read/Edit tools) against a scripted
//! OpenAI-compatible server on localhost. No network, no key.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zencode_engine::{run_task, RunOptions};

/// Serves `replies` in order as chat completions; records request bodies.
/// Redacts the nondeterministic `"duration_ms"` value in the event stream so the
/// contract snapshot is stable across machines and runs (CI was red on all three
/// OSes because the stored snapshot pinned one machine's wall-clock duration).
fn scrub_duration_ms(s: &str) -> String {
    let key = "\"duration_ms\":";
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(key) {
        out.push_str(&rest[..i + key.len()]);
        rest = &rest[i + key.len()..];
        let n = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        out.push('0');
        rest = &rest[n..];
    }
    out.push_str(rest);
    out
}
async fn serve(replies: Vec<Value>) -> (String, Arc<Mutex<Vec<Value>>>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(vec![]));
    let seen2 = seen.clone();
    let replies = Arc::new(Mutex::new(replies.into_iter()));
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let seen = seen2.clone();
            let replies = replies.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 8192];
                let body_start;
                let len;
                loop {
                    let n = s.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_lowercase();
                        len = head
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        body_start = p + 4;
                        break;
                    }
                }
                while buf.len() < body_start + len {
                    let n = s.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let req: Value =
                    serde_json::from_slice(&buf[body_start..body_start + len]).unwrap();
                seen.lock().unwrap().push(req);
                let next = replies.lock().unwrap().next();
                let (status, body) = match next {
                    Some(b) => ("200 OK", b.to_string()),
                    None => (
                        "500 Internal Server Error",
                        "{\"error\":\"script exhausted\"}".into(),
                    ),
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    (url, seen)
}

fn tool_reply(id: &str, name: &str, args: Value) -> Value {
    json!({"id":"x","object":"chat.completion","choices":[{"index":0,"finish_reason":"tool_calls","message":{
        "role":"assistant","content":null,
        "tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]}}],
        "usage":{"prompt_tokens":20,"completion_tokens":8,"total_tokens":28}})
}

fn text_reply(t: &str) -> Value {
    json!({"id":"x","object":"chat.completion","choices":[{"index":0,"finish_reason":"stop","message":{
        "role":"assistant","content":t}}],"usage":{"prompt_tokens":30,"completion_tokens":5,"total_tokens":35}})
}

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("zen_engine_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn agent_reads_then_edits_file_through_real_tools() {
    let dir = tmpdir("edit");
    let f = dir.join("hello.txt");
    std::fs::write(&f, "hello world\n").unwrap();
    let fp = f.to_string_lossy().to_string();
    let (url, seen) = serve(vec![
        tool_reply("c1", "Read", json!({"file_path": fp})),
        tool_reply(
            "c2",
            "Edit",
            json!({"file_path": fp, "old_string": "world", "new_string": "zen"}),
        ),
        text_reply("Done: replaced world with zen."),
    ])
    .await;
    let opts = RunOptions::new(url, "fake/model:free", &dir);
    let res = run_task(&opts, "change world to zen in hello.txt")
        .await
        .unwrap();
    assert!(!res.is_error, "{}", res.result);
    assert_eq!(res.result, "Done: replaced world with zen.");
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "hello zen\n");
    let reqs = seen.lock().unwrap();
    assert_eq!(reqs.len(), 3, "exactly 3 model requests");
    assert_eq!(reqs[0]["model"], "fake/model:free");
    // the tool result of Read was fed back to the model
    let last = serde_json::to_string(&reqs[2]["messages"]).unwrap();
    assert!(
        last.contains("hello world"),
        "Read output reached the model"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// Windows: the Bash tool needs a POSIX shell; CI's Windows runner failed here (g1 note in PLAN.md).
#[cfg(unix)]
#[tokio::test]
async fn bash_tool_runs_in_cwd() {
    let dir = tmpdir("bash");
    let cmd = if cfg!(windows) {
        "echo hi> out.txt"
    } else {
        "echo hi > out.txt"
    };
    let (url, _) = serve(vec![
        tool_reply(
            "c1",
            "Bash",
            json!({"command": cmd, "description": "write"}),
        ),
        text_reply("ok"),
    ])
    .await;
    let res = run_task(&RunOptions::new(url, "m:free", &dir), "make out.txt")
        .await
        .unwrap();
    assert!(!res.is_error, "{}", res.result);
    assert!(std::fs::read_to_string(dir.join("out.txt"))
        .unwrap()
        .starts_with("hi"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn empty_task_and_bad_cwd_are_rejected() {
    let opts = RunOptions::new("http://127.0.0.1:1/v1", "m", std::env::temp_dir());
    assert!(run_task(&opts, "   ").await.is_err());
    let bad = RunOptions::new("http://127.0.0.1:1/v1", "m", "/definitely/not/here");
    assert!(run_task(&bad, "x").await.is_err());
}

#[tokio::test]
async fn server_error_surfaces_as_error_result() {
    let dir = tmpdir("err");
    let (url, _) = serve(vec![]).await; // every request -> 500
    let mut opts = RunOptions::new(url, "m:free", &dir);
    opts.max_turns = 2;
    let res = run_task(&opts, "anything").await.unwrap();
    assert!(res.is_error, "500s must not look like success");
    let _ = std::fs::remove_dir_all(&dir);
}

// Step 1 contract: the versioned event stream of a real Read→Edit run. The snapshot pins the
// schema; any CLI bump that changes what the UI would see fails here first.
#[tokio::test]
async fn event_stream_contract_v1() {
    use zencode_engine::events::{parse_ndjson, RecordingSink, SCHEMA_VERSION};
    let dir = tmpdir("events");
    let f = dir.join("a.txt");
    std::fs::write(&f, "one\n").unwrap();
    let fp = "a.txt".to_string();
    let (url, _) = serve(vec![
        tool_reply("c1", "Read", json!({"file_path": f.to_string_lossy()})),
        text_reply("ok"),
    ])
    .await;
    let _ = fp;
    let sink = std::sync::Arc::new(RecordingSink::new());
    let opts = RunOptions::new(url, "fake/model:free", &dir);
    let res = zencode_engine::run_task_recorded(&opts, "read a.txt", sink.clone())
        .await
        .unwrap();
    assert!(!res.is_error, "{}", res.result);
    let nd = sink.to_ndjson();
    let evs = parse_ndjson(&nd).unwrap();
    assert!(evs.iter().all(|e| e.v == SCHEMA_VERSION));
    assert!(evs.iter().enumerate().all(|(i, e)| e.seq == i as u64));
    // normalise the tempdir path so the snapshot is machine independent
    // The path appears JSON-escaped twice (arguments is a JSON string inside JSON), so on
    // Windows each separator is `\\\\` in the stream. Try both escape levels, then unify separators.
    let raw = dir.to_string_lossy().to_string();
    let esc1 = raw.replace('\\', "\\\\");
    let esc2 = esc1.replace('\\', "\\\\");
    let norm = scrub_duration_ms(&nd);
    let norm = norm
        .replace(&esc2, "<DIR>")
        .replace(&esc1, "<DIR>")
        .replace(&raw, "<DIR>")
        .replace("<DIR>\\\\\\\\", "<DIR>/")
        .replace("<DIR>\\\\", "<DIR>/");
    insta::assert_snapshot!("event_stream_v1", norm);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn newer_or_missing_schema_version_is_rejected() {
    use zencode_engine::events::parse_ndjson;
    assert!(parse_ndjson(r#"{"v":99,"seq":0,"type":"text_delta","text":"x"}"#).is_err());
    assert!(parse_ndjson(r#"{"seq":0,"type":"text_delta","text":"x"}"#).is_err());
    assert!(parse_ndjson("not json").is_err());
    assert_eq!(
        parse_ndjson(r#"{"v":1,"seq":0,"type":"text_delta","text":"x"}"#)
            .unwrap()
            .len(),
        1
    );
}
