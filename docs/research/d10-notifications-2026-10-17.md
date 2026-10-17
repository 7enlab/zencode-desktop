# D10 — Completion Notifications

**Date:** 2026-10-17 · **Status:** design proposal for Hark 2 (g6) · **Docs only**

## Research

**Tauri notification options.** `tauri-plugin-notification` (official) wraps OS-native
notifications: Windows Toast (Action Center), macOS `NSUserNotification`/UNUserNotificationCenter,
Linux `libnotify` (D-Bus). It supports title, body, icon, and action buttons on
Windows/macOS; Linux action support varies by desktop environment (GNOME: yes,
KDE: yes, minimal WMs: no). Fallback: in-app toast when the OS API is unavailable.

**What deserves a notification.** Long agent runs happen in the background while the
user does other work. Notify on: (a) run finished (success/failure), (b) approval
needed (agent blocked waiting on the user — see D6), (c) run failed with an error
that needs attention, (d) milestone for very long runs (e.g. every 15 min). Never
notify on routine progress (TextDelta, ToolResult) — that's notification spam.

## Ideas

**Trigger on `DesktopEvent`s.** The engine already emits everything needed
(`engine/src/events.rs` on main). The UI maps events → notifications locally; no
engine change required.

| `DesktopEvent` | Notify? | Condition |
|---|---|---|
| `SessionEnd` (PR #4, unmerged) | ✅ | Always — success or failure |
| `PermissionDenied` | ✅ | Always — agent is blocked, needs user |
| `ToolResult { is_error: true }` | ⚠️ | Only if it ends the run (check following events) |
| `ApiRetry { error_status: 429 }` | ⚠️ | Only after 3rd retry — "rate-limited, still trying" |
| `SystemMessage` | ❌ | No — too noisy |
| `AssistantMessage`, `TextDelta` | ❌ | Never |

**Content.** Title: `ZenCode — task finished` / `ZenCode — approval needed` / `ZenCode — task failed`.
Body: one line — first 80 chars of the result summary or the tool name awaiting approval.
Actions: `View` (focus window, open session), `Dismiss`. On click-through, deep-link
to the session id so the right conversation opens.

**OS integration.** Use `tauri-plugin-notification` from day one (add to D3's plugin list).
Respect OS do-not-disturb automatically (the plugin defers to the OS). Add an in-app
"quiet hours" setting (default: respect OS only, no custom schedule in v1).

**Long-run milestones.** If a run exceeds 15 minutes without user focus, send one
"still working…" notification, then silence until completion. Prevents the user from
assuming the app hung.

## Results

**Notification payload schema** (UI-internal, built from the event envelope):

```json
{
  "id": "notif-<uuid>",
  "kind": "task_finished | task_failed | approval_needed | still_working",
  "title": "ZenCode — task finished",
  "body": "Refactored auth.ts (12 files changed)",
  "session_id": "ses_abc123",
  "actions": ["view", "dismiss"],
  "silent": false
}
```

**Rules (v1):**
1. Max 1 notification per run, except `approval_needed` (repeat max every 5 min while blocked).
2. Never notify when the app window is focused — use the in-app event feed instead.
3. `approval_needed` always plays the default sound; others are silent by default.

**Open questions for Hark 2:**
1. Should `still_working` milestones be configurable (interval, on/off), or fixed at 15 min?
2. Linux action-button support is DE-dependent — acceptable to degrade to click-to-open?
3. Do we need notification history in-app, or is the session list sufficient?
