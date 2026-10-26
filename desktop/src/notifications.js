/**
 * Desktop notifications - frontend (D10 design, g6 roadmap).
 *
 * Listens for backend engine events and routes them to OS notifications
 * (via the Tauri `cmd_notify_*` commands) or, in demo/standalone mode,
 * to an in-page toast.
 *
 * D10 rules enforced backend-side; this module only maps events → calls:
 * - SessionEnd            → cmd_notify_session_end (always)
 * - PermissionDenied      → cmd_notify_approval_needed (always)
 * - ToolResult {is_error} → cmd_notify_session_end(success=false) only if it ends the run
 * - Everything else       → no notification
 */

// ---------------------------------------------------------------------------
// Environment detection
// ---------------------------------------------------------------------------

/** True when running inside the Tauri webview. */
export const isTauri = typeof window !== "undefined" && !!window.__TAURI__;

let invokeFn = null;
if (isTauri) {
  // Lazily loaded - the Tauri API is only present in the webview.
  import("@tauri-apps/api/core")
    .then((m) => {
      invokeFn = m.invoke;
    })
    .catch(() => {
      invokeFn = null;
    });
}

// ---------------------------------------------------------------------------
// In-page toast (demo mode / fallback)
// ---------------------------------------------------------------------------

function ensureToastContainer() {
  let el = document.getElementById("zc-toast-container");
  if (!el) {
    el = document.createElement("div");
    el.id = "zc-toast-container";
    el.style.cssText =
      "position:fixed;top:16px;right:16px;z-index:9999;display:flex;" +
      "flex-direction:column;gap:8px;max-width:360px;";
    document.body.appendChild(el);
  }
  return el;
}

/**
 * Show an in-page toast. Used in demo mode and as a fallback when the
 * OS notification API is unavailable.
 */
export function showToast(title, body, kind = "info") {
  const container = ensureToastContainer();
  const toast = document.createElement("div");
  const accent =
    kind === "approval"
      ? "#f0883e"
      : kind === "failed"
        ? "#f85149"
        : "#3fb950";
  toast.style.cssText =
    "background:#161b22;border:1px solid #30363d;border-left:3px solid " +
    accent +
    ";border-radius:8px;padding:10px 14px;color:#e6edf3;" +
    "font-family:system-ui,sans-serif;font-size:13px;" +
    "box-shadow:0 8px 24px rgba(0,0,0,.5);cursor:pointer;";
  toast.innerHTML =
    `<div style="font-weight:600;margin-bottom:2px;">${escapeHtml(title)}</div>` +
    `<div style="color:#8b949e;">${escapeHtml(body)}</div>`;
  toast.addEventListener("click", () => toast.remove());
  container.appendChild(toast);
  // Auto-dismiss after 8s (approval toasts stay until clicked).
  if (kind !== "approval") {
    setTimeout(() => toast.remove(), 8000);
  }
  return toast;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => {
    const map = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
    return map[c];
  });
}

// ---------------------------------------------------------------------------
// Backend invocation
// ---------------------------------------------------------------------------

async function invokeOrToast(command, args, toastTitle, toastBody, toastKind) {
  if (invokeFn) {
    try {
      await invokeFn(command, args);
      return;
    } catch (e) {
      console.warn(`[notifications] ${command} failed, falling back to toast:`, e);
    }
  }
  showToast(toastTitle, toastBody, toastKind);
}

/** Notify that a run finished. `envelope` is the SessionEnd IpcEnvelope. */
export function notifySessionEnd(envelope) {
  const payload = envelope.payload || {};
  const sessionId = envelope.session_id || payload.session_id || "unknown";
  const summary = payload.summary || payload.result || "task completed";
  const success = payload.success !== false;
  return invokeOrToast(
    "cmd_notify_session_end",
    { sessionId, summary, success },
    success ? "ZenCode - task finished" : "ZenCode - task failed",
    `${summary} (${sessionId})`,
    success ? "info" : "failed",
  );
}

/** Notify that the agent is blocked waiting for approval. */
export function notifyApprovalNeeded(toolName, sessionId) {
  return invokeOrToast(
    "cmd_notify_approval_needed",
    { toolName, sessionId },
    "ZenCode - approval needed",
    `Agent is waiting for approval: ${toolName}`,
    "approval",
  );
}

// ---------------------------------------------------------------------------
// Event router - maps engine IpcEnvelopes to notifications (D10 trigger table)
// ---------------------------------------------------------------------------

/**
 * Route one engine event envelope to the right notification, if any.
 * Returns true when a notification was (or would be) shown.
 *
 * @param {object} envelope - IpcEnvelope {v, seq, type, session_id, payload}
 * @param {object} opts - { runEnded: boolean } - set when this event ends the run
 */
export function routeEvent(envelope, opts = {}) {
  const type = envelope.type || "";
  const payload = envelope.payload || {};

  if (type === "session_end" || type === "SessionEnd") {
    notifySessionEnd(envelope);
    return true;
  }
  if (type === "permission_denied" || type === "PermissionDenied") {
    const tool = payload.tool_name || payload.tool || "unknown tool";
    notifyApprovalNeeded(tool, envelope.session_id || "unknown");
    return true;
  }
  if (
    (type === "tool_result" || type === "ToolResult") &&
    payload.is_error === true &&
    opts.runEnded === true
  ) {
    notifySessionEnd({ ...envelope, payload: { ...payload, success: false } });
    return true;
  }
  // SystemMessage, AssistantMessage, TextDelta: never notify (D10).
  return false;
}

// ---------------------------------------------------------------------------
// Demo mode - simulate the trigger table without a backend
// ---------------------------------------------------------------------------

/** Play a short demo of the notification triggers (standalone mode). */
export function demoNotifications() {
  const sessionId = "demo-" + Math.floor(Math.random() * 1000);
  showToast("ZenCode - task finished", `Refactored auth.ts (12 files changed) (${sessionId})`, "info");
  setTimeout(() => {
    showToast(
      "ZenCode - approval needed",
      "Agent is waiting for approval: Bash (rm -rf)",
      "approval",
    );
  }, 1500);
}
