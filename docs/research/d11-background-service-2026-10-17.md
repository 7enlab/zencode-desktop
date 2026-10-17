# D11 — Background Service (tray icon, headless tasks)

*Design only. Author: Muse 2, 2026-10-17. For Hark 2 (g6).*

## Research

Tauri 2 ships tray support in core (`tauri::tray::TrayIcon`, no plugin needed since
2.0) plus official plugins that matter here:

- `tauri-plugin-single-instance` — second launch focuses the running instance
  instead of starting a duplicate (prevents two agents writing the same session).
- `tauri-plugin-autostart` — "launch at login", per-OS (Login Items on macOS,
  Startup folder/registry on Windows, XDG autostart `.desktop` on Linux).
- `tauri-plugin-notification` — completion alerts when the window is hidden (D10).

OS differences that shape the design:

| OS | Tray/menu-bar | Close-button norm | Risk |
|----|---------------|-------------------|------|
| Windows | System tray, always available | Minimize-to-tray is common | Low |
| macOS | Menu-bar extras; dock icon separate | Cmd+Q quits, red dot hides | Users expect "quit means quit" |
| Linux | AppIndicator (GNOME needs extension), native on KDE | Varies | Tray may not exist — must degrade to a plain background process |

OpenCode 2.0 validates the pattern: one background service shared by TUI/web/desktop,
with background subagents (see `docs/research/competitors-2026-10-17.md`).

## Ideas

**Lifecycle.** The app has three states: `foreground` (window open), `tray`
(window hidden, process alive), `stopped`. Closing the window moves to `tray`
(never quits); quitting happens only via tray menu → Quit (or Cmd+Q on macOS,
which must show a confirm dialog when a task is running).

**Tray icon states.** The icon itself is the status light:

- `idle` — default icon.
- `working` — icon with a dot/badge; tooltip shows current task ("Running: fix login bug").
- `error` — icon with warning badge; tooltip shows the failure; click opens the window to the failing run.

**Background execution.** Agent runs continue headless in `tray` state via the
same engine `run_task` path (D1 `StreamEmitter` keeps recording to NDJSON, so
nothing is lost). On `SessionEnd`, fire a notification (D10) — success and
failure both notify, since the user isn't watching. Long-running tasks get the
D2 reconnect story for free: reopening the window replays the NDJSON log.

**Tray menu structure:**

```
ZenCode — idle | ● working: <task title> | ⚠ error: <last error>
─────────────────────────────────
Open ZenCode
Pause current task / Resume
─────────────────────────────────
Start at login  [✓/ ]
─────────────────────────────────
Quit
```

**Autostart.** Opt-in only (default off). When enabled, the app starts directly
into `tray` state (no window) — a window popping up at every login is the
fastest way to get uninstalled.

**Single instance.** Mandatory: `tauri-plugin-single-instance` with a
`zencode://` deep-link handler so "open second copy" just focuses the tray icon
and opens the window.

## Results

**Background lifecycle spec:**

1. `window.close-requested` → if a task is running, hide to tray (no dialog —
   the task is the user's intent); if idle, hide to tray silently.
2. In `tray` state the event loop keeps running; engine tasks proceed normally.
3. `SessionEnd` in `tray` state → notification (D10) + tray icon flips to
   `idle` (success) or `error` (failure).
4. Tray menu → Quit → if a task is running, confirm dialog ("A task is running.
   Quit anyway?"); otherwise quit immediately.
5. Autostart launches straight into `tray` state; first manual window-open after
   autostart shows a one-time hint ("ZenCode is running in the tray").
6. Linux without a tray: fall back to a headless background process; a
   `zencode://open` deep link (or re-running the binary, via single-instance)
   restores the window.

**Open questions for Hark 2:**

1. macOS: should Cmd+Q quit outright (platform norm) or also confirm when idle?
   Leaning: confirm only when a task is running.
2. Pause/Resume in the tray menu needs engine cancellation support (D9 notes
   `run_task` is single-turn on main) — tray ships first with Pause disabled
   until the engine catches up?
3. Update flow (D3): when the updater downloads a new version while in `tray`
   state, install on next quit or prompt immediately?
