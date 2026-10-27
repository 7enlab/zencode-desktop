use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zencode_engine::budget::TaskBudget;
use zencode_engine::events::RecordingSink;
use zencode_engine::{run_task_with_sink, RunOptions};

async fn serve(replies: Vec<Value>) -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    let seen = Arc::new(std::sync::Mutex::new(vec![]));
    let seen2 = seen.clone();
    let replies = Arc::new(std::sync::Mutex::new(replies.into_iter()));
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

fn text_reply_with_usage(t: &str, prompt_tokens: u64, completion_tokens: u64) -> Value {
    json!({"id":"x","object":"chat.completion","choices":[{"index":0,"finish_reason":"stop","message":{
        "role":"assistant","content":t}}],
        "usage":{"prompt_tokens":prompt_tokens,"completion_tokens":completion_tokens,
                 "total_tokens":prompt_tokens+completion_tokens}})
}

fn tool_reply(id: &str, name: &str, args: Value) -> Value {
    json!({"id":"x","object":"chat.completion","choices":[{"index":0,"finish_reason":"tool_calls","message":{
        "role":"assistant","content":null,
        "tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]}}],
        "usage":{"prompt_tokens":20,"completion_tokens":8,"total_tokens":28}})
}

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("zen_budget_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn has_budget_event(sink: &RecordingSink) -> bool {
    sink.to_ndjson()
        .unwrap()
        .lines()
        .any(|l| l.contains("\"event\":\"budget_exceeded\"") || l.contains("budget_exceeded"))
}

#[tokio::test]
async fn output_token_budget_stops_task_and_logs_event() {
    let dir = tmpdir("tokens");

    let (url, seen) = serve(vec![text_reply_with_usage("done", 30, 60_000)]).await;
    let sink = Arc::new(RecordingSink::new());
    let mut opts = RunOptions::new(url, "fake/model:free", &dir);
    opts.dangerous_allow_all = true;
    opts.budget = Some(TaskBudget {
        max_output_tokens: 50_000,
        ..TaskBudget::unlimited()
    });
    let res = run_task_with_sink(&opts, "say hi", sink.clone())
        .await
        .unwrap();
    assert!(res.is_error, "budget trip must end the task as an error");
    assert!(
        res.result
            .contains("Task stopped: output token budget 50,000 exceeded (used 60,000)"),
        "unexpected result: {}",
        res.result
    );
    assert!(
        has_budget_event(&sink),
        "budget_exceeded missing from session log"
    );

    assert_eq!(seen.lock().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn tool_call_budget_denies_and_stops_task() {
    let dir = tmpdir("tools");
    let f = dir.join("a.txt");
    std::fs::write(&f, "one\n").unwrap();
    let fp = f.to_string_lossy().to_string();
    let (url, _) = serve(vec![
        tool_reply("c1", "Read", json!({"file_path": fp})),
        tool_reply("c2", "Read", json!({"file_path": fp})),
        text_reply_with_usage("done", 30, 5),
    ])
    .await;
    let sink = Arc::new(RecordingSink::new());
    let mut opts = RunOptions::new(url, "fake/model:free", &dir);
    opts.dangerous_allow_all = true;
    opts.budget = Some(TaskBudget {
        max_tool_calls: 1,
        ..TaskBudget::unlimited()
    });
    let res = run_task_with_sink(&opts, "read a.txt twice", sink.clone())
        .await
        .unwrap();
    assert!(res.is_error, "budget trip must end the task as an error");
    assert!(
        res.result
            .contains("Task stopped: tool call budget 1 exceeded (used 2)"),
        "unexpected result: {}",
        res.result
    );
    assert!(
        has_budget_event(&sink),
        "budget_exceeded missing from session log"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn generous_budget_lets_small_task_finish_cleanly() {
    let dir = tmpdir("ok");
    let (url, _) = serve(vec![text_reply_with_usage("hello!", 30, 5)]).await;
    let sink = Arc::new(RecordingSink::new());
    let mut opts = RunOptions::new(url, "fake/model:free", &dir);
    opts.dangerous_allow_all = true;
    opts.budget = Some(TaskBudget::default());
    let res = run_task_with_sink(&opts, "say hi", sink.clone())
        .await
        .unwrap();
    assert!(!res.is_error, "{}", res.result);
    assert_eq!(res.result, "hello!");
    assert!(
        !has_budget_event(&sink),
        "no budget event should be logged within budget"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
