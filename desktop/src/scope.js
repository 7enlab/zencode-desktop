"use strict";
/* ZenCode Desktop - scope declaration card (R3).
 *
 * Renders the agent's declared scope as a checklist before it starts:
 *   renderScopeCard(container, decl, onDecision)
 *     decl: { id, description, files: [], dirs: [], reasons: [] }
 *       reasons: approval reasons from engine needs_approval() (may be empty)
 *     onDecision: ({ id, kind }) => void
 *       kind: "approve" | "expand" | "deny"
 *
 * Standalone (no Tauri dependency): works in demo mode and in the real
 * shell. Mirrors the approval.js card pattern from PR #54 so both cards
 * share visual language; when PR #54 merges, this can share its CSS.
 *
 * Demo: call demoScope() to render a fake card standalone.
 */

const SCOPE_CSS = `
.scope-card {
  border: 1px solid #30363d; border-left: 3px solid #58a6ff;
  border-radius: 6px; background: #0d1117; color: #c9d1d9;
  padding: 12px 14px; margin: 8px 0; font-family: -apple-system, "Segoe UI", sans-serif;
  max-width: 640px;
}
.scope-card.risky { border-left-color: #d29922; }
.scope-card .sc-head { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
.scope-card .sc-title { font-weight: 600; font-size: 14px; }
.scope-card .sc-badge {
  font-size: 11px; padding: 2px 8px; border-radius: 10px; font-weight: 600;
  text-transform: uppercase; letter-spacing: 0.04em;
  background: #58a6ff26; color: #58a6ff; border: 1px solid #58a6ff55;
}
.scope-card.risky .sc-badge { background: #d2992226; color: #d29922; border-color: #d2992255; }
.scope-card .sc-id { margin-left: auto; font-size: 11px; color: #8b949e; font-family: monospace; }
.scope-card .sc-desc { font-size: 13px; color: #8b949e; margin-bottom: 8px; }
.scope-card .sc-list { list-style: none; margin: 0 0 10px; padding: 0; max-height: 180px; overflow-y: auto; }
.scope-card .sc-list li {
  font-family: monospace; font-size: 12px; padding: 3px 8px;
  background: #161b22; border: 1px solid #21262d; border-radius: 4px; margin-bottom: 4px;
  color: #a5d6ff; word-break: break-all;
}
.scope-card .sc-list li.dir { color: #7ee787; }
.scope-card .sc-reasons { font-size: 12px; color: #d29922; margin-bottom: 10px; }
.scope-card .sc-reasons div { margin-bottom: 4px; }
.scope-card .sc-actions { display: flex; gap: 8px; }
.scope-card button {
  padding: 6px 14px; border-radius: 6px; border: 1px solid #30363d;
  background: #21262d; color: #c9d1d9; font-size: 13px; cursor: pointer;
}
.scope-card button:hover { background: #30363d; }
.scope-card button.primary { background: #1f6feb; border-color: #1f6feb; color: #fff; }
.scope-card button.primary:hover { background: #388bfd; }
.scope-card button.danger { color: #f85149; }
`;

let scopeCssInjected = false;
function ensureScopeCss() {
  if (scopeCssInjected || typeof document === "undefined") return;
  const style = document.createElement("style");
  style.textContent = SCOPE_CSS;
  document.head.appendChild(style);
  scopeCssInjected = true;
}

function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

function renderScopeCard(container, decl, onDecision) {
  ensureScopeCss();
  const el = document.createElement("div");
  const risky = decl.reasons && decl.reasons.length > 0;
  el.className = "scope-card" + (risky ? " risky" : "");

  const files = (decl.files || []).map((f) => `<li>📄 ${esc(f)}</li>`).join("");
  const dirs = (decl.dirs || []).map((d) => `<li class="dir">📁 ${esc(d)}/</li>`).join("");
  const reasons = (decl.reasons || []).map((r) => `<div>⚠️ ${esc(r)}</div>`).join("");

  el.innerHTML = `
    <div class="sc-head">
      <span class="sc-title">Scope declaration</span>
      <span class="sc-badge">${risky ? "needs approval" : "declared"}</span>
      <span class="sc-id">${esc(decl.id || "")}</span>
    </div>
    <div class="sc-desc">${esc(decl.description || "The agent will only touch the files below.")}</div>
    <ul class="sc-list">${files}${dirs}</ul>
    ${reasons ? `<div class="sc-reasons">${reasons}</div>` : ""}
    <div class="sc-actions">
      <button class="primary" data-kind="approve">Approve scope</button>
      <button data-kind="expand">Expand…</button>
      <button class="danger" data-kind="deny">Deny</button>
    </div>`;

  el.querySelectorAll("button").forEach((btn) => {
    btn.addEventListener("click", () => {
      const kind = btn.getAttribute("data-kind");
      el.querySelectorAll("button").forEach((b) => { b.disabled = true; });
      onDecision({ id: decl.id, kind });
    });
  });

  container.appendChild(el);
  return el;
}

function demoScope() {
  renderScopeCard(
    document.body,
    {
      id: "scope-demo-1",
      description: "Fix the typo in the README",
      files: ["README.md", "docs/guide.md"],
      dirs: ["src"],
      reasons: ["declaration touches 25 files (limit 20) - confirm the scope"],
    },
    (d) => console.log("scope decision:", d),
  );
}

if (typeof module !== "undefined") module.exports = { renderScopeCard, demoScope };
