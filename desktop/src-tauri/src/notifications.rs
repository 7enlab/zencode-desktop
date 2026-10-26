//! Desktop notifications (D10 design, g6 roadmap).
//!
//! Wraps `tauri-plugin-notification` with the D10 policy rules:
//! 1. Max 1 notification per run, except `approval_needed` (throttled to 5 min).
//! 2. Never notify when the app window is focused - the in-app feed covers it.
//! 3. `approval_needed` plays the default sound; task notifications are silent.
//!
//! The OS handles do-not-disturb automatically (the plugin defers to it).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::{Action, ActionType, NotificationExt};

/// Action type id for session notifications ("View" / "Dismiss" buttons).
pub const SESSION_ACTIONS_ID: &str = "zencode-session";

/// D10 notification kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    TaskFinished,
    TaskFailed,
    ApprovalNeeded,
    StillWorking,
}

/// Per-run notification state (D10 rule 1).
#[derive(Debug, Default)]
struct RunState {
    notified: bool,
    last_approval: Option<Instant>,
    last_milestone: Option<Instant>,
}

/// Enforces the D10 notification policy. Registered as Tauri managed state.
pub struct NotificationManager {
    runs: Mutex<HashMap<String, RunState>>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
        }
    }

    /// D10 rule 2: never notify when the app window is focused.
    pub fn should_notify<R: Runtime>(app: &AppHandle<R>) -> bool {
        match app.get_webview_window("main") {
            Some(window) => !window.is_focused().unwrap_or(false),
            // No window yet (startup) - notifying is harmless.
            None => true,
        }
    }

    /// D10 rule 1: max 1 notification per run; approval_needed repeats at
    /// most every 5 minutes while the agent is blocked; still_working
    /// milestones repeat at most every 15 minutes.
    fn can_notify_run(&self, session_id: &str, kind: NotificationKind) -> bool {
        let mut runs = self
            .runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = runs.entry(session_id.to_string()).or_default();
        match kind {
            NotificationKind::ApprovalNeeded => {
                if let Some(last) = state.last_approval {
                    if last.elapsed() < Duration::from_secs(300) {
                        return false;
                    }
                }
                state.last_approval = Some(Instant::now());
                true
            }
            NotificationKind::StillWorking => {
                if let Some(last) = state.last_milestone {
                    if last.elapsed() < Duration::from_secs(900) {
                        return false;
                    }
                }
                state.last_milestone = Some(Instant::now());
                true
            }
            _ => {
                if state.notified {
                    return false;
                }
                state.notified = true;
                true
            }
        }
    }

    /// Forget a finished run so a future run with the same id starts clean.
    /// Called by the engine wiring when a session is closed (follow-up step).
    #[allow(dead_code)]
    pub fn clear_run(&self, session_id: &str) {
        let mut runs = self
            .runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runs.remove(session_id);
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Register the "View" / "Dismiss" action buttons (D10 content spec).
/// Linux support varies by desktop environment - degrades to click-to-open.
pub fn register_actions<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let action_type = ActionType::builder(SESSION_ACTIONS_ID)
        .actions(vec![
            Action::builder("view", "View").foreground(true).build(),
            Action::builder("dismiss", "Dismiss").build(),
        ])
        .build();
    app.notification()
        .register_action_types(vec![action_type])
        .map_err(|e| e.to_string())
}

fn truncate(text: &str, max_chars: usize) -> String {
    let truncated: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        format!("{truncated}…")
    } else {
        truncated
    }
}

/// Show an OS notification for a finished run (success or failure).
/// Applies D10 rules 1 (max 1/run) and 2 (skip when focused).
pub fn notify_session_end<R: Runtime>(
    app: &AppHandle<R>,
    manager: &NotificationManager,
    session_id: &str,
    summary: &str,
    success: bool,
) -> Result<(), String> {
    let kind = if success {
        NotificationKind::TaskFinished
    } else {
        NotificationKind::TaskFailed
    };
    if !manager.can_notify_run(session_id, kind) {
        return Ok(());
    }
    if !NotificationManager::should_notify(app) {
        return Ok(());
    }
    let title = if success {
        "ZenCode - task finished"
    } else {
        "ZenCode - task failed"
    };
    let body = format!("{} - {}", truncate(summary, 80), session_id);
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .action_type_id(SESSION_ACTIONS_ID)
        .silent() // D10 rule 3: task notifications are silent
        .show()
        .map_err(|e| e.to_string())
}

/// Show an OS notification when the agent is blocked waiting for approval.
/// D10 rule 3: always plays the default sound.
pub fn notify_approval_needed<R: Runtime>(
    app: &AppHandle<R>,
    manager: &NotificationManager,
    tool_name: &str,
    session_id: &str,
) -> Result<(), String> {
    if !manager.can_notify_run(session_id, NotificationKind::ApprovalNeeded) {
        return Ok(());
    }
    if !NotificationManager::should_notify(app) {
        return Ok(());
    }
    app.notification()
        .builder()
        .title("ZenCode - approval needed")
        .body(format!(
            "Agent is waiting for approval: `{}` ({})",
            truncate(tool_name, 60),
            session_id
        ))
        .action_type_id(SESSION_ACTIONS_ID)
        // No `.silent()` - D10 rule 3: approval_needed plays the default sound.
        .show()
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tauri commands (called from the frontend via `invoke`)
// ---------------------------------------------------------------------------

/// Frontend-callable: notify that a run is still working (D10 long-run milestone).
/// Send at most once per 15 minutes per run - the caller enforces the interval.
#[tauri::command]
pub fn cmd_notify_still_working<R: Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, NotificationManager>,
    session_id: String,
    elapsed_mins: u64,
) -> Result<(), String> {
    if !manager.can_notify_run(&session_id, NotificationKind::StillWorking) {
        return Ok(());
    }
    if !NotificationManager::should_notify(&app) {
        return Ok(());
    }
    app.notification()
        .builder()
        .title("ZenCode - still working…")
        .body(format!(
            "Run {} has been going for {} min - no action needed.",
            session_id, elapsed_mins
        ))
        .silent()
        .show()
        .map_err(|e| e.to_string())
}

/// Frontend-callable: notify that a run finished.
#[tauri::command]
pub fn cmd_notify_session_end<R: Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, NotificationManager>,
    session_id: String,
    summary: String,
    success: bool,
) -> Result<(), String> {
    notify_session_end(&app, &manager, &session_id, &summary, success)
}

/// Frontend-callable: notify that the agent needs approval.
#[tauri::command]
pub fn cmd_notify_approval_needed<R: Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, NotificationManager>,
    tool_name: String,
    session_id: String,
) -> Result<(), String> {
    notify_approval_needed(&app, &manager, &tool_name, &session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_one_notification_per_run() {
        let manager = NotificationManager::new();
        assert!(manager.can_notify_run("s1", NotificationKind::TaskFinished));
        assert!(!manager.can_notify_run("s1", NotificationKind::TaskFinished));
        // Different run is unaffected.
        assert!(manager.can_notify_run("s2", NotificationKind::TaskFailed));
    }

    #[test]
    fn approval_needed_throttled_to_5min() {
        let manager = NotificationManager::new();
        assert!(manager.can_notify_run("s1", NotificationKind::ApprovalNeeded));
        // Immediate repeat is blocked.
        assert!(!manager.can_notify_run("s1", NotificationKind::ApprovalNeeded));
    }

    #[test]
    fn still_working_throttled_to_15min() {
        let manager = NotificationManager::new();
        assert!(manager.can_notify_run("s1", NotificationKind::StillWorking));
        // Immediate repeat is blocked.
        assert!(!manager.can_notify_run("s1", NotificationKind::StillWorking));
        // But a task-finished notification for the same run is independent.
        assert!(manager.can_notify_run("s1", NotificationKind::TaskFinished));
    }

    #[test]
    fn clear_run_resets_state() {
        let manager = NotificationManager::new();
        assert!(manager.can_notify_run("s1", NotificationKind::TaskFinished));
        assert!(!manager.can_notify_run("s1", NotificationKind::TaskFinished));
        manager.clear_run("s1");
        assert!(manager.can_notify_run("s1", NotificationKind::TaskFinished));
    }

    #[test]
    fn truncate_short_text_unchanged() {
        assert_eq!(truncate("hello", 80), "hello");
    }

    #[test]
    fn truncate_long_text_gets_ellipsis() {
        let long = "a".repeat(100);
        let out = truncate(&long, 80);
        assert_eq!(out.chars().count(), 81); // 80 chars + ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_unicode_safe() {
        // Must not split a multi-byte char.
        let s = "héllo wörld - test ✓";
        let out = truncate(s, 8);
        assert!(out.chars().count() <= 9);
    }
}
