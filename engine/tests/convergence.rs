use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zencode_engine::convergence::ConvergencePolicy;
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

fn tool_reply(id: &str, name: &str, args: Value) -> Value {
    json!({"id":"x","object":"chat.completion","choices":[{"index":0,"finish_reason":"tool_calls","message":{
        "role":"assistant","content":null,
        "tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]}}],
        "usage":{"prompt_tokens":20,"completion_tokens":8,"total_tokens":28}})
}

fn text_reply(t: &str) -> Value {
    json!({"id":"x","object":"chat.completion","choices":[{"index":0,"finish_reason":"stop","message":{
        "role":"assistant","content":t}}],
        "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})
}

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("zen_converge_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn failing_build(id: &str) -> Value {
    tool_reply(
        id,
        "Bash",
        json!({"command": "echo 'error[E0425]: cannot find value `x` in this scope' >&2; exit 1"}),
    )
}

fn count(ndjson: &str, needle: &str) -> usize {
    ndjson.lines().filter(|l| l.contains(needle)).count()
}

#[cfg(unix)]
#[tokio::test]
async fn looping_tool_triggers_replan_then_stop() {
    let dir = tmpdir("loop");

    let replies: Vec<Value> = (1..=7).map(|i| failing_build(&format!("c{i}"))).collect();
    let (url, seen) = serve(replies).await;
    let sink = Arc::new(RecordingSink::new());
    let mut opts = RunOptions::new(url, "fake/model:free", &dir);
    opts.dangerous_allow_all = true;
    opts.max_turns = 20;
    opts.convergence = Some(ConvergencePolicy::default());
    let res = run_task_with_sink(&opts, "make the build pass", sink.clone())
        .await
        .unwrap();

    assert!(res.is_error, "guard must end the task as an error");
    assert!(
        res.result.contains("convergence guard"),
        "result names the guard: {}",
        res.result
    );
    let log = sink.to_ndjson().unwrap();
    assert_eq!(count(&log, "loop_detected"), 2, "{log}");
    assert_eq!(count(&log, "convergence_stop"), 1, "{log}");

    let reqs = seen.lock().unwrap();

    assert_eq!(reqs.len(), 7, "model calls that reached the server");

    let fifth = serde_json::to_string(&reqs[4]["messages"]).unwrap();
    assert!(
        fifth.contains("CONVERGENCE GUARD"),
        "re-plan prompt reached the model"
    );
    let fourth = serde_json::to_string(&reqs[3]["messages"]).unwrap();
    assert!(
        !fourth.contains("CONVERGENCE GUARD"),
        "not before the loop fired"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn guard_is_opt_in_and_quiet_on_progress() {
    assert!(RunOptions::new("http://x", "m", ".").convergence.is_none());

    let dir = tmpdir("progress");
    let (url, _seen) = serve(vec![
        failing_build("c1"),
        failing_build("c2"),
        tool_reply("c3", "Bash", json!({"command": "echo ok"})),
        failing_build("c4"),
        failing_build("c5"),
        text_reply("Build fixed."),
    ])
    .await;
    let sink = Arc::new(RecordingSink::new());
    let mut opts = RunOptions::new(url, "fake/model:free", &dir);
    opts.dangerous_allow_all = true;
    opts.convergence = Some(ConvergencePolicy::default());
    let res = run_task_with_sink(&opts, "make the build pass", sink.clone())
        .await
        .unwrap();
    assert!(!res.is_error, "{}", res.result);
    assert_eq!(res.result, "Build fixed.");
    let log = sink.to_ndjson().unwrap();
    assert_eq!(count(&log, "loop_detected"), 0, "{log}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn repeating_the_same_probe_is_still_stagnant() {
    let dir = tmpdir("probe");
    let fp = dir.join("notes.txt");
    std::fs::write(&fp, "same content\n").unwrap();
    let fp = fp.to_string_lossy().to_string();
    let mut replies: Vec<Value> = (1..=7)
        .map(|i| tool_reply(&format!("r{i}"), "Read", json!({"file_path": fp})))
        .collect();
    replies.push(text_reply("Re-planned."));
    let (url, seen) = serve(replies).await;
    let sink = Arc::new(RecordingSink::new());
    let mut opts = RunOptions::new(url, "fake/model:free", &dir);
    opts.dangerous_allow_all = true;
    opts.max_turns = 20;
    opts.convergence = Some(ConvergencePolicy::default());
    let res = run_task_with_sink(&opts, "look around", sink.clone())
        .await
        .unwrap();
    assert!(!res.is_error, "{}", res.result);
    let log = sink.to_ndjson().unwrap();
    assert_eq!(count(&log, "loop_detected"), 1, "{log}");
    assert!(log.contains("stagnant_rounds"), "{log}");
    let reqs = seen.lock().unwrap();
    let last = serde_json::to_string(&reqs[reqs.len() - 1]["messages"]).unwrap();
    assert!(
        last.contains("CONVERGENCE GUARD"),
        "re-plan prompt reached the model"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
