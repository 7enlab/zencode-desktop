"use strict";
/* ZenCode Desktop chat UI — vanilla JS.
 * Frontend pipeline (D2 IPC design): onmessage → validate v → seq gap check
 * → push to pending → rAF flush → single DOM update → persist NDJSON. */

const SCHEMA_VERSION = 1;
const state = {
  mode: "mock",           // "mock" | "live"
  sessions: [],
  activeSession: null,
  expectedSeq: 0,
  pending: [],            // envelopes waiting for rAF flush
  rafScheduled: false,
  assistantEl: null,      // current streaming bubble
  log: [],                // persisted NDJSON lines (this run)
};

/* ---------- sessions ---------- */
const MOCK_SESSIONS = [
  { id: "2026-10-17-a1b2c3", title: "Refactor rate_gate.rs", preview: "extract quota persistence…", model: "nemotron-3-nano:free", updated: "17:20" },
  { id: "2026-10-17-d4e5f6", title: "Debug checkpoint restore", preview: "auto-checkpoint before restore…", model: "nemotron-3-nano:free", updated: "16:02" },
  { id: "2026-10-16-9a8b7c", title: "Plan mode spike", preview: "PlanGuardedTool intercepts…", model: "nemotron-3-nano:free", updated: "yesterday" },
];

// Live mode when running inside the Tauri webview; otherwise demo/mock mode.
const TAURI = (typeof window !== "undefined" && window.__TAURI__) || null;
const invoke = TAURI ? TAURI.core.invoke : null;
const Channel = TAURI ? TAURI.core.Channel : null;

// Default backend config for live mode (free-only; override via settings later).
const LIVE_DEFAULTS = {
  model: "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
  baseUrl: "https://ai-gateway.kilo.ai/api/v1",
  cwd: ".",
};

async function loadSessions() {
  if (!invoke) { state.sessions = MOCK_SESSIONS; return; }
  try {
    const sessions = await invoke("list_sessions");
    state.sessions = sessions.map(s => ({
      id: s.id, title: s.title, preview: s.preview,
      model: s.model, updated: (s.updated_at || "").slice(0, 16).replace("T", " "),
    }));
  } catch (e) {
    console.warn("list_sessions failed, using mock:", e);
    state.sessions = MOCK_SESSIONS;
  }
}

function renderSessions(filter = "") {
  const ul = document.getElementById("session-list");
  ul.innerHTML = "";
  state.sessions
    .filter(s => (s.title + s.preview).toLowerCase().includes(filter.toLowerCase()))
    .forEach(s => {
      const li = document.createElement("li");
      if (state.activeSession === s.id) li.classList.add("active");
      li.innerHTML = `<span class="sess-title"></span><span class="sess-meta"></span>`;
      li.querySelector(".sess-title").textContent = s.title;
      li.querySelector(".sess-meta").textContent = `${s.model} · ${s.updated}`;
      li.onclick = () => selectSession(s.id);
      ul.appendChild(li);
    });
}

function selectSession(id) {
  state.activeSession = id;
  document.getElementById("messages").innerHTML = "";
  state.expectedSeq = 0; state.assistantEl = null;
  hideGap();
  renderSessions(document.getElementById("session-search").value);
  if (typeof timelineSessionChanged === "function") timelineSessionChanged(id);
  if (state.mode === "live") replaySession(id); else playDemo(); // mock: replay demo conversation for the session
}

/* R2 timeline tabs. */
document.getElementById("tab-chat").onclick = () => showChatTab();
document.getElementById("tab-timeline").onclick = () => showTimelineTab();

/* Live-mode replay: pull the persisted log, then follow new envelopes. */
async function replaySession(id) {
  try {
    const envs = await invoke("get_log_since", { sessionId: id, since: 0 });
    envs.forEach(onEnvelope);
    setStatus("conn", "● live");
  } catch (e) {
    console.warn("get_log_since failed:", e);
    setStatus("conn", "● live (replay failed)");
  }
}

/* ---------- envelope pipeline ---------- */
function onEnvelope(env) {
  if (env.v !== SCHEMA_VERSION) { console.warn("unsupported schema v", env.v); return; }
  if (env.seq !== state.expectedSeq) showGap(env.seq);
  state.expectedSeq = Math.max(state.expectedSeq, env.seq + 1);
  state.pending.push(env);
  state.log.push(JSON.stringify(env));
  scheduleFlush();
}

function scheduleFlush() {
  if (state.rafScheduled) return;
  state.rafScheduled = true;
  requestAnimationFrame(() => { state.rafScheduled = false; flushBatch(); });
}

function flushBatch() {
  const batch = state.pending.splice(0);
  for (const env of batch) applyEnvelope(env);
  const box = document.getElementById("messages");
  box.scrollTop = box.scrollHeight;
}

function showGap(got) {
  const w = document.getElementById("gap-warning");
  document.getElementById("gap-seqs").textContent = `${state.expectedSeq}..${got - 1}`;
  w.classList.remove("hidden");
}
function hideGap() { document.getElementById("gap-warning").classList.add("hidden"); }
document.getElementById("gap-replay").onclick = async () => {
  if (state.mode === "live" && state.activeSession && invoke) {
    try {
      const envs = await invoke("get_log_since", { sessionId: state.activeSession, since: state.expectedSeq });
      hideGap();
      envs.forEach(onEnvelope);
      return;
    } catch (e) { console.warn("gap replay failed:", e); }
  }
  // mock fallback
  hideGap(); state.expectedSeq = state.log.length;
};

/* ---------- rendering ---------- */
function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function applyEnvelope(env) {
  const box = document.getElementById("messages");
  switch (env.type) {
    case "text_delta": {
      if (!state.assistantEl) {
        state.assistantEl = el("div", "msg assistant typing");
        state.assistantEl.append(el("div", "role", "ZenCode"), el("div", "body"));
        box.append(state.assistantEl);
      }
      state.assistantEl.querySelector(".body").textContent += env.text || "";
      break;
    }
    case "assistant_message":
      state.assistantEl = null;
      box.append(wrapMsg("assistant", "ZenCode", env.content || ""));
      break;
    case "tool_result": {
      state.assistantEl = null;
      const d = el("details", "msg tool");
      const ok = !env.error;
      d.innerHTML = `<summary></summary><pre></pre>`;
      const s = d.querySelector("summary");
      s.append(el("span", ok ? "ok" : "err", ok ? "✓ " : "✗ "), document.createTextNode(`${env.tool_name || "tool"} `));
      s.append(el("span", "", env.tool_use_id ? `· ${env.tool_use_id.slice(0, 8)}` : ""));
      d.querySelector("pre").textContent = (env.output || env.error || "").slice(0, 4000);
      box.append(d);
      break;
    }
    case "permission_denied":
      box.append(wrapMsg("assistant denied", "ZenCode", `Permission denied for ${env.tool_name || "tool"}: ${env.reason || env.text || ""}`));
      break;
    case "system_message":
      box.append(wrapMsg("system", "", env.text || env.event || ""));
      break;
    case "api_retry":
      setStatus("conn", `↻ retry ${env.attempt || ""}`.trim());
      break;
    case "compact_boundary":
      box.append(wrapMsg("system", "", "— context compacted —"));
      break;
    default:
      console.warn("unknown envelope type", env.type);
  }
}

function wrapMsg(cls, role, text) {
  const m = el("div", "msg " + cls);
  if (role) m.append(el("div", "role", role));
  const b = el("div", "body", text);
  m.append(b);
  return m;
}

/* ---------- input ---------- */
async function sendInput() {
  const input = document.getElementById("input");
  const text = input.value.trim();
  if (!text) return;
  input.value = "";
  const box = document.getElementById("messages");
  const m = el("div", "msg user", text);
  box.append(m);
  box.scrollTop = box.scrollHeight;
  if (state.mode === "mock") { mockRespond(text); return; }
  // live mode: invoke the Tauri `run_task` command with a Channel
  await liveRun(text);
}

async function liveRun(text) {
  const channel = new Channel();
  channel.onmessage = onEnvelope;
  setStatus("conn", "● live · running…");
  try {
    const sessionId = await invoke("run_task", {
      prompt: text,
      model: LIVE_DEFAULTS.model,
      baseUrl: LIVE_DEFAULTS.baseUrl,
      cwd: LIVE_DEFAULTS.cwd,
      onEvent: channel,
    });
    // Track the new session in the list.
    state.sessions.unshift({ id: sessionId, title: text.slice(0, 60), preview: "", model: LIVE_DEFAULTS.model, updated: "now" });
    state.activeSession = sessionId;
    renderSessions(document.getElementById("session-search").value);
  } catch (e) {
    box_append_error(String(e));
    setStatus("conn", "● live · error");
  }
}

function box_append_error(text) {
  const box = document.getElementById("messages");
  box.append(wrapMsg("assistant denied", "ZenCode", "Error: " + text));
  box.scrollTop = box.scrollHeight;
}

document.getElementById("send").onclick = sendInput;
document.getElementById("input").addEventListener("keydown", e => {
  if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendInput(); }
});
document.getElementById("new-session").onclick = () => {
  const id = "draft-" + Date.now().toString(36);
  state.sessions.unshift({ id, title: "New session", preview: "", model: "nemotron-3-nano:free", updated: "now" });
  renderSessions(); selectSession(id);
};
document.getElementById("session-search").addEventListener("input", e => renderSessions(e.target.value));

function setStatus(part, text) {
  document.getElementById("status-" + part).textContent = text;
}

/* ---------- mock backend: demo.ndjson player ---------- */
let demoTimers = [];
async function playDemo() {
  demoTimers.forEach(clearTimeout); demoTimers = [];
  setStatus("model", "model: nemotron-3-nano:free");
  setStatus("quota", "quota: 12/50 today");
  setStatus("conn", "● mock");
  let lines;
  try {
    const res = await fetch("demo.ndjson");
    lines = (await res.text()).trim().split("\n");
  } catch { return; } // no demo file (e.g. packaged build) — stay empty
  let delay = 400, seq = 0;
  for (const line of lines) {
    const env = JSON.parse(line);
    env.seq = seq++;
    const isDelta = env.type === "text_delta";
    delay += isDelta ? 60 : 500;
    demoTimers.push(setTimeout(() => onEnvelope(env), delay));
  }
}

function mockRespond(text) {
  const base = state.expectedSeq;
  const reply = [
    { v: 1, type: "text_delta", text: "Mock mode — no engine connected yet. " },
    { v: 1, type: "text_delta", text: `You said: "${text.slice(0, 60)}". ` },
    { v: 1, type: "assistant_message", content: `Mock mode — no engine connected yet. You said: "${text.slice(0, 60)}".` },
  ];
  reply.forEach((env, i) => { env.seq = base + i; setTimeout(() => onEnvelope(env), 300 * (i + 1)); });
}

/* ---------- boot ---------- */
(async function boot() {
  if (TAURI && invoke) {
    state.mode = "live";
    setStatus("conn", "● live");
    setStatus("model", "model: " + LIVE_DEFAULTS.model);
    await loadSessions();
  } else {
    state.sessions = MOCK_SESSIONS;
  }
  renderSessions();
  if (state.sessions.length) selectSession(state.sessions[0].id);

  /* R8 60-second onboarding: show once for first-time users. */
  try {
    if (typeof maybeStartOnboarding === "function" && typeof localStorage !== "undefined") {
      maybeStartOnboarding({
        document,
        storage: localStorage,
        onFinish: (result) => {
          if (result.completed && result.modelId) {
            setStatus("model", "model: " + result.modelId);
          }
        },
      });
    }
  } catch (e) {
    console.warn("onboarding failed to start:", e);
  }
})();
