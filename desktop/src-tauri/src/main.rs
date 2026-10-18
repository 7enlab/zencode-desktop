//! ZenCode Desktop — Tauri 2 shell with engine wiring (g6).
//!
//! Tauri commands (see `docs/ipc-bridge.md` for the frontend pipeline):
//! - `run_task` — starts an engine run in the background, streaming
//!   [`IpcEnvelope`]s to the UI over a `tauri::ipc::Channel` (one per run).
//! - `list_sessions` — sessions persisted by `SessionStore`.
//! - `get_log_since` — reconnect replay: envelopes after `seq`, live or from disk.
//! - `get_timeline` — R2 undo timeline: plain-language action entries +
//!   checkpoints for a session.
//! - `restore_checkpoint` — R2: restore a checkpoint into the session's
//!   workspace (auto-checkpoints first, so the restore is itself undoable).
//!
//! The engine runs as a library (`zencode_engine`, path dependency). Only
//! `:free` models are accepted — the desktop never spends money.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::ipc::Channel;
use tauri::Manager;
use zencode_engine::{
    checkpoints::CheckpointManager,
    events::parse_ndjson,
    ipc_bridge::{BridgeSink, IpcEnvelope},
    session_store::{SessionMeta, SessionStore},
    timeline::{self, RestoreOutcome, TimelineCheckpoint, TimelineResponse},
    RunOptions,
};

/// Live run sinks, keyed by session id (for `get_log_since` during a run).
struct AppState {
    sinks: Mutex<HashMap<String, Arc<BridgeSink>>>,
}

/// Only `:free` models are allowed — ZenCode Desktop never uses paid models.
fn ensure_free_model(model: &str) -> Result<(), String> {
    let m = model.trim();
    if m.is_empty() {
        return Err("model must not be empty".to_owned());
    }
    if !m.ends_with(":free") {
        return Err(format!("only :free models are allowed (got {m:?})"));
    }
    Ok(())
}

/// Sink that records into a [`BridgeSink`] and forwards each new envelope to
/// the UI over a Tauri [`Channel`].
struct ForwardingSink {
    bridge: Arc<BridgeSink>,
    channel: Channel<IpcEnvelope>,
}

impl zen_core::EventSink for ForwardingSink {
    fn emit(&self, ev: zen_core::AgentEvent) {
        let before = self.bridge.envelopes().len();
        self.bridge.emit(ev);
        let envs = self.bridge.envelopes();
        // Forward only when a new envelope was actually recorded, so `seq`
        // stays gapless and nothing is ever re-sent.
        if envs.len() > before {
            let _ = self.channel.send(envs[envs.len() - 1].clone());
        }
    }
}

/// Start one agent run. Returns the new session id immediately; envelopes
/// stream to `on_event` as the run progresses.
#[tauri::command]
async fn run_task(
    app: tauri::AppHandle,
    prompt: String,
    model: String,
    base_url: String,
    cwd: String,
    on_event: Channel<IpcEnvelope>,
) -> Result<String, String> {
    ensure_free_model(&model)?;
    if prompt.trim().is_empty() {
        return Err("prompt must not be empty".to_owned());
    }
    if base_url.trim().is_empty() {
        return Err("no LLM backend configured: set base_url to a free OpenAI-compatible endpoint (e.g. https://ai-gateway.kilo.ai/api/v1)".to_owned());
    }
    let cwd_path = PathBuf::from(&cwd);
    if !cwd_path.is_dir() {
        return Err(format!("cwd is not a directory: {cwd}"));
    }

    let session_id = SessionStore::new_session_id();
    let bridge = Arc::new(BridgeSink::new());
    app.state::<AppState>()
        .sinks
        .lock()
        .map_err(|e| format!("state lock poisoned: {e}"))?
        .insert(session_id.clone(), bridge.clone());

    let sink: Arc<dyn zen_core::EventSink> = Arc::new(ForwardingSink {
        bridge: bridge.clone(),
        channel: on_event,
    });
    let mut opts = RunOptions::new(base_url, model.clone(), cwd_path.clone());
    // The user picked this folder and started the run, so tools are explicitly
    // pre-approved (OCR M1 opt-in). TODO: replace with the R3 scope/approval flow.
    opts.dangerous_allow_all = true;
    let sid = session_id.clone();

    // Run in the background; the UI streams envelopes over `on_event`.
    // Nothing is faked: if the backend is unreachable the run fails and the
    // error is recorded in the session log.
    tauri::async_runtime::spawn(async move {
        let result = zencode_engine::run_task_with_sink(&opts, &prompt, sink).await;
        if let Err(e) = &result {
            // Record the failure as the run's last word (best-effort).
            eprintln!("run_task {sid} failed: {e:#}");
        }
        // Persist the run for resume/list, best-effort.
        if let Ok(store) = SessionStore::with_default_root() {
            let envelopes: Vec<_> = bridge
                .envelopes()
                .iter()
                .filter_map(|e| e.to_envelope().ok())
                .collect();
            let _ = store.save_session(&sid, &envelopes, None, &model, &cwd_path);
        }
    });

    Ok(session_id)
}

/// Sessions persisted on disk, newest first.
#[tauri::command]
fn list_sessions() -> Result<Vec<SessionMeta>, String> {
    let store = SessionStore::with_default_root().map_err(|e| e.to_string())?;
    store.list_sessions().map_err(|e| e.to_string())
}

/// Reconnect replay: envelopes with `seq > since`, from the live run if it is
/// still in memory, otherwise from the persisted NDJSON log.
#[tauri::command]
fn get_log_since(
    app: tauri::AppHandle,
    session_id: String,
    since: u64,
) -> Result<Vec<IpcEnvelope>, String> {
    // Block path traversal before touching the filesystem.
    if session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
        || session_id.trim().is_empty()
    {
        return Err("invalid session id".to_owned());
    }
    // Live run first.
    if let Ok(sinks) = app.state::<AppState>().sinks.lock() {
        if let Some(bridge) = sinks.get(&session_id) {
            return Ok(bridge.get_log_since(since));
        }
    }
    // Otherwise replay the persisted NDJSON.
    let store = SessionStore::with_default_root().map_err(|e| e.to_string())?;
    let path = store.root().join(format!("{session_id}.ndjson"));
    let ndjson = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let bridge = BridgeSink::from_ndjson(&ndjson).map_err(|e| e.to_string())?;
    Ok(bridge.get_log_since(since))
}

/// Reject session ids that could escape the sessions directory.
fn validate_session_id_arg(session_id: &str) -> Result<(), String> {
    if session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
        || session_id.trim().is_empty()
    {
        return Err("invalid session id".to_owned());
    }
    Ok(())
}

/// Checkpoint ids are git short/full SHAs: hex only, sane length.
fn validate_checkpoint_id_arg(checkpoint_id: &str) -> Result<(), String> {
    let id = checkpoint_id.trim();
    if id.len() < 4 || id.len() > 40 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid checkpoint id".to_owned());
    }
    Ok(())
}

/// The session's workspace dir, from its persisted metadata.
fn session_cwd(session_id: &str) -> Result<PathBuf, String> {
    let store = SessionStore::with_default_root().map_err(|e| e.to_string())?;
    let meta = store
        .list_sessions()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| m.id == session_id)
        .ok_or_else(|| "unknown session id".to_owned())?;
    if meta.cwd.trim().is_empty() {
        return Err("session has no workspace recorded".to_owned());
    }
    Ok(PathBuf::from(meta.cwd))
}

/// R2 undo timeline: plain-language action entries plus the session's
/// checkpoints, newest checkpoint first.
#[tauri::command]
fn get_timeline(session_id: String) -> Result<TimelineResponse, String> {
    validate_session_id_arg(&session_id)?;
    let store = SessionStore::with_default_root().map_err(|e| e.to_string())?;
    let path = store.root().join(format!("{session_id}.ndjson"));
    let ndjson = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let envelopes = parse_ndjson(&ndjson).map_err(|e| e.to_string())?;
    let entries = timeline::build_timeline(&envelopes);

    // Checkpoints live in the session workspace's shadow repo (D5).
    let checkpoints = match session_cwd(&session_id) {
        Ok(cwd) => CheckpointManager::new(&cwd)
            .and_then(|m| m.list())
            .map(|infos| {
                infos
                    .into_iter()
                    .map(|i| TimelineCheckpoint {
                        id: i.id,
                        message: i.message,
                        timestamp: i.timestamp,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    Ok(TimelineResponse {
        session_id,
        entries,
        checkpoints,
    })
}

/// R2: restore a checkpoint into the session's workspace.
///
/// The checkpoint manager auto-snapshots the current state first, so the
/// restore itself is undoable — the returned `safety_checkpoint_id` can be
/// restored to get back to "before".
#[tauri::command]
fn restore_checkpoint(session_id: String, checkpoint_id: String) -> Result<RestoreOutcome, String> {
    validate_session_id_arg(&session_id)?;
    validate_checkpoint_id_arg(&checkpoint_id)?;
    let cwd = session_cwd(&session_id)?;
    let manager = CheckpointManager::new(&cwd).map_err(|e| e.to_string())?;
    let report = manager
        .restore(checkpoint_id.trim())
        .map_err(|e| e.to_string())?;
    Ok(RestoreOutcome {
        restored_files: report.restored_files,
        safety_checkpoint_id: report.new_checkpoint_id,
    })
}

fn main() {
    tauri::Builder::default()
        // Auto-update. `plugins.updater.pubkey` in tauri.conf.json holds the
        // release minisign public key (docs/keys/zencode-release.pub, see
        // docs/signed-releases.md); updates whose signature does not verify
        // are refused by the plugin.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        // Second launch focuses the existing window instead of opening a new one.
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))
        // NOTE: tauri_plugin_deep_link is excluded until upstream fixes the
        // rust-ini 0.21 compile break (see Cargo.toml). The `zencode://`
        // scheme stays declared in tauri.conf.json.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_positioner::init())
        .manage(AppState {
            sinks: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            run_task,
            list_sessions,
            get_log_since,
            get_timeline,
            restore_checkpoint
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
