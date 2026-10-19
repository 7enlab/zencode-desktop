/* ZenCode Desktop — embedded terminal tab (D6).
 *
 * Frontend: xterm.js v6.0.0 (package @xterm/xterm) loaded from CDN.
 *   <script src="https://cdn.jsdelivr.net/npm/@xterm/xterm@6.0.0/lib/xterm.js"></script>
 *   <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/@xterm/xterm@6.0.0/css/xterm.css" />
 * Backend (not yet wired): portable-pty per tab, bytes over a Tauri
 * ipc::Channel — see terminal-README.md. Until then, DEMO_MODE provides
 * a fake shell so the UI is testable standalone.
 *
 * SECURITY: terminals are user-driven ONLY. The agent never types into
 * them — every byte sent to the backend comes from the user's keyboard.
 */

'use strict';

/** Set to false once the portable-pty backend is wired (see terminal-README.md). */
const TERMINAL_DEMO_MODE = true;

/**
 * Backend hook contract (stable across demo and real backend):
 *  - write(data: string): bytes typed by the USER -> backend PTY stdin
 *  - onData(cb): register callback receiving PTY stdout bytes -> render
 *  - resize(cols, rows): terminal size changed -> backend master.resize()
 *  - close(): kill the PTY child process
 */
class TerminalSession {
  constructor(id, label, backend) {
    this.id = id;
    this.label = label;
    this.backend = backend; // implements write/onData/resize/close
    this.term = null; // xterm.js Terminal instance
    this._dataHandlers = [];
    this._disposed = false;
  }

  /** Attach an xterm.js instance to a DOM element and wire backend <-> frontend. */
  attach(el) {
    if (typeof Terminal === 'undefined') {
      el.innerHTML = '<div class="term-error">xterm.js failed to load from CDN.</div>';
      return;
    }
    this.term = new Terminal({
      cursorBlink: true,
      scrollback: 10000, // per D6 design
      fontFamily: 'ui-monospace, "Cascadia Code", Menlo, Consolas, monospace',
      fontSize: 13,
      theme: {
        background: '#0d1117',
        foreground: '#e6edf3',
        cursor: '#79c0ff',
        selectionBackground: 'rgba(31, 111, 235, 0.35)',
      },
    });
    this.term.open(el);

    // PTY stdout -> xterm
    this.backend.onData((data) => {
      if (!this._disposed) this.term.write(data);
    });

    // User keystrokes -> backend. USER-DRIVEN ONLY: no programmatic input path.
    this.term.onData((data) => {
      this.backend.write(data);
      for (const h of this._dataHandlers) h(data);
    });

    // Fit terminal to container; report size to backend.
    this._fit();
    new ResizeObserver(() => this._fit()).observe(el);
    this.term.focus();
  }

  /** Extra listener for user-typed bytes (e.g. analytics). Never used to inject input. */
  onData(cb) { this._dataHandlers.push(cb); }

  /** Backend -> frontend bytes (used by demo shell and, later, the PTY channel). */
  write(data) {
    if (this.term && !this._disposed) this.term.write(data);
  }

  resize(cols, rows) {
    if (this.term) this.term.resize(cols, rows);
    this.backend.resize(cols, rows);
  }

  close() {
    this._disposed = true;
    try { this.backend.close(); } catch { /* already dead */ }
    if (this.term) this.term.dispose();
  }

  _fit() {
    if (!this.term) return;
    // Cheap fit without the fit addon: derive cols/rows from element size.
    const el = this.term.element;
    if (!el) return;
    const dims = this.term.dimensions;
    if (!dims || !dims.actualCellWidth) return;
    const rect = el.getBoundingClientRect();
    const cols = Math.max(20, Math.floor(rect.width / dims.actualCellWidth));
    const rows = Math.max(5, Math.floor(rect.height / dims.actualCellHeight));
    this.resize(cols, rows);
  }
}

/**
 * Demo backend: a fake shell. Echoes the user's commands with a `$ ` prompt.
 * Implements the same hook contract as the future portable-pty backend.
 */
class FakeShell {
  constructor(session) {
    this.session = session;
    this._buf = '';
    this._cbs = [];
    this._dead = false;
  }
  onData(cb) { this._cbs.push(cb); }
  write(data) {
    if (this._dead) return;
    for (const ch of data) this._handleChar(ch);
  }
  resize(/* cols, rows */) { /* demo: no-op */ }
  close() { this._dead = true; }

  _emit(s) { for (const cb of this._cbs) cb(s); }

  _handleChar(ch) {
    if (ch === '\r') { // Enter
      this._emit('\r\n');
      this._runCommand(this._buf);
      this._buf = '';
      if (!this._dead) this._emit('$ ');
    } else if (ch === '\u007f') { // Backspace
      if (this._buf.length > 0) {
        this._buf = this._buf.slice(0, -1);
        this._emit('\b \b');
      }
    } else if (ch >= ' ' || ch === '\t') {
      this._buf += ch;
      this._emit(ch);
    }
    // Other control chars ignored in demo.
  }

  _runCommand(line) {
    const [cmd, ...args] = line.trim().split(/\s+/).filter(Boolean);
    if (!cmd) return;
    if (cmd === 'exit') {
      this._emit('logout\r\n');
      this._dead = true;
      return;
    }
    if (cmd === 'clear') {
      this.session.term?.clear();
      return;
    }
    if (cmd === 'help') {
      this._emit('demo shell commands: help, clear, echo <text>, exit\r\n');
      return;
    }
    if (cmd === 'echo') {
      this._emit(args.join(' ') + '\r\n');
      return;
    }
    // Unknown command: echo it back so the demo visibly "runs" user input.
    this._emit(`$ ${line}\r\n(executed in demo mode — wire portable-pty for a real shell)\r\n`);
  }

  greet() {
    this._emit('ZenCode terminal (demo shell — no real backend yet)\r\n');
    this._emit('Type `help` for demo commands. The agent cannot type here.\r\n$ ');
  }
}

/** Demo backend factory — replaced by the Tauri portable-pty wiring later. */
function createBackend(session) {
  if (TERMINAL_DEMO_MODE) return new FakeShell(session);
  // Real backend: see terminal-README.md. Throws until implemented.
  throw new Error('portable-pty backend not wired yet');
}

/** Manages the terminal tab bar + sessions. One tab = one session. */
class TerminalManager {
  constructor(tabsEl, panesEl) {
    this.tabsEl = tabsEl;
    this.panesEl = panesEl;
    this.sessions = new Map();
    this.activeId = null;
    this._nextId = 1;
  }

  newTab() {
    const id = 'term-' + this._nextId++;
    const label = 'Terminal ' + (this._nextId - 1);
    const session = new TerminalSession(id, label, null);
    session.backend = createBackend(session);
    this.sessions.set(id, session);

    const tab = document.createElement('button');
    tab.className = 'term-tab';
    tab.dataset.tabId = id;
    tab.innerHTML = `<span>${label}</span><span class="term-tab-close" title="Close">×</span>`;
    tab.querySelector('.term-tab-close').addEventListener('click', (e) => {
      e.stopPropagation();
      this.closeTab(id);
    });
    tab.addEventListener('click', () => this.activate(id));
    this.tabsEl.appendChild(tab);

    const pane = document.createElement('div');
    pane.className = 'term-pane hidden';
    pane.dataset.paneId = id;
    this.panesEl.appendChild(pane);

    session.attach(pane);
    if (session.backend.greet) session.backend.greet();
    this.activate(id);
    return session;
  }

  activate(id) {
    this.activeId = id;
    for (const t of this.tabsEl.querySelectorAll('.term-tab')) {
      t.classList.toggle('active', t.dataset.tabId === id);
    }
    for (const p of this.panesEl.querySelectorAll('.term-pane')) {
      p.classList.toggle('hidden', p.dataset.paneId !== id);
    }
    const s = this.sessions.get(id);
    if (s && s.term) s.term.focus();
  }

  closeTab(id) {
    const s = this.sessions.get(id);
    if (s) s.close();
    this.sessions.delete(id);
    this.tabsEl.querySelector(`[data-tab-id="${id}"]`)?.remove();
    this.panesEl.querySelector(`[data-pane-id="${id}"]`)?.remove();
    const remaining = [...this.sessions.keys()];
    if (remaining.length > 0) this.activate(remaining[remaining.length - 1]);
    else this.activeId = null;
  }
}

// Boot when the terminal view becomes visible (lazy: don't load xterm until needed).
let _termManager = null;
function initTerminal() {
  if (_termManager) return _termManager;
  const tabsEl = document.getElementById('term-tabs');
  const panesEl = document.getElementById('term-panes');
  if (!tabsEl || !panesEl) return null;
  _termManager = new TerminalManager(tabsEl, panesEl);
  document.getElementById('term-new-tab')?.addEventListener('click', () => _termManager.newTab());
  _termManager.newTab();
  return _termManager;
}

if (typeof module !== 'undefined') module.exports = { TerminalSession, FakeShell, TerminalManager, TERMINAL_DEMO_MODE };

/* Terminal tab show/hide (follows timeline.js / network.js pattern). */
function showTerminalTab() {
  document.getElementById("messages").classList.add("hidden");
  document.getElementById("composer").classList.add("hidden");
  const tp = document.getElementById("timeline-panel");
  if (tp) tp.classList.add("hidden");
  const np = document.getElementById("network-panel");
  if (np) np.classList.add("hidden");
  document.getElementById("terminal-view").classList.remove("hidden");
  document.getElementById("tab-chat").classList.remove("active");
  const tt = document.getElementById("tab-timeline");
  if (tt) tt.classList.remove("active");
  const nt = document.getElementById("tab-network");
  if (nt) nt.classList.remove("active");
  document.getElementById("tab-terminal").classList.add("active");
  if (typeof initTerminal === "function") initTerminal();
}

function hideTerminalTab() {
  document.getElementById("terminal-view").classList.add("hidden");
  document.getElementById("tab-terminal").classList.remove("active");
}
