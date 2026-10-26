/**
 * Sovereign Mode (R5) UI: toggle, status badge, preflight findings, attestation.
 *
 * Mirrors engine/src/sovereign.rs - the engine is the source of truth.
 * This file duplicates the *presentation* logic so the UI can render without
 * a backend round-trip; policy decisions themselves always come from the
 * engine via the Tauri commands below.
 *
 * Expected Tauri commands (engine side):
 *   - sovereign_status()        -> SovereignStatus JSON
 *   - sovereign_enable()        -> { ok } | { error, findings[] }
 *   - sovereign_disable()       -> attestation JSON
 *   - sovereign_preflight()     -> findings[]
 */

export const SOVEREIGN = Object.freeze({
  OLLAMA_URL: 'http://localhost:11434/v1',
  POLICY_VERSION: 'sovereign-policy-v1',
  ATTESTATION_SCHEME: 'hash-chain-v1',
});

/**
 * Pure logic: badge text + tone for a SovereignStatus object.
 * status = { enabled, session_id, ollama_endpoint, ollama_reachable,
 *            allowed_egress, blocked_egress }
 */
export function badgeFor(status) {
  if (!status) return { text: 'Sovereign: unknown', tone: 'muted' };
  if (status.enabled) {
    const blocked = status.blocked_egress || 0;
    return {
      text: `Sovereign ON · ${blocked} egress blocked`,
      tone: 'sovereign-on',
    };
  }
  if (!status.ollama_reachable) {
    return { text: 'Sovereign OFF · Ollama not running', tone: 'warn' };
  }
  return { text: 'Sovereign OFF', tone: 'muted' };
}

/**
 * Pure logic: can the Enable button be pressed right now?
 * Ollama must be reachable; otherwise enabling would promise local
 * inference the engine cannot deliver.
 */
export function canEnable(status) {
  return !!status && !status.enabled && !!status.ollama_reachable;
}

/**
 * Render preflight findings into a container element.
 * findings: [{ kind, detail, acknowledgable }]
 * Returns the number of *blocking* (non-acknowledgable) findings.
 */
export function renderPreflight(container, findings) {
  container.innerHTML = '';
  if (!findings || findings.length === 0) {
    const p = document.createElement('p');
    p.className = 'preflight-clean';
    p.textContent = 'Preflight clean - no remote endpoints or API keys found.';
    container.appendChild(p);
    return 0;
  }
  let blocking = 0;
  const ul = document.createElement('ul');
  ul.className = 'preflight-findings';
  for (const f of findings) {
    const li = document.createElement('li');
    li.className = f.acknowledgable ? 'finding-ack' : 'finding-block';
    const kind = document.createElement('strong');
    kind.textContent = f.acknowledgable ? 'Warning: ' : 'Must fix: ';
    const detail = document.createElement('span');
    detail.textContent = f.detail;
    li.appendChild(kind);
    li.appendChild(detail);
    if (!f.acknowledgable) {
      blocking += 1;
      const label = document.createElement('label');
      label.className = 'ack-row';
      const cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.disabled = true;
      cb.title = 'This finding cannot be acknowledged - fix the configuration instead.';
      const span = document.createElement('span');
      span.textContent = 'Cannot be acknowledged';
      label.appendChild(cb);
      label.appendChild(span);
      li.appendChild(label);
    } else {
      const label = document.createElement('label');
      label.className = 'ack-row';
      const cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.dataset.findingKind = f.kind;
      cb.className = 'ack-checkbox';
      const span = document.createElement('span');
      span.textContent = 'I understand - enable anyway';
      label.appendChild(cb);
      label.appendChild(span);
      li.appendChild(label);
    }
    ul.appendChild(li);
  }
  container.appendChild(ul);
  return blocking;
}

/**
 * Wire the toggle UI. Elements:
 *   - toggleBtn: the enable/disable button
 *   - badgeEl: status badge element
 *   - preflightEl: container for findings
 *   - tauri: object with async invoke(cmd, args)
 */
export function wireSovereignToggle({ toggleBtn, badgeEl, preflightEl, tauri }) {
  async function refresh() {
    const status = await tauri.invoke('sovereign_status');
    const badge = badgeFor(status);
    badgeEl.textContent = badge.textContent;
    badgeEl.dataset.tone = badge.tone;
    toggleBtn.textContent = status.enabled ? 'Disable Sovereign Mode' : 'Enable Sovereign Mode';
    toggleBtn.disabled = !status.enabled && !canEnable(status);
    toggleBtn.title = !status.enabled && !status.ollama_reachable
      ? 'Start Ollama (`ollama serve`) first - Sovereign Mode needs local inference.'
      : '';
    return status;
  }

  toggleBtn.addEventListener('click', async () => {
    const status = await tauri.invoke('sovereign_status');
    if (status.enabled) {
      const att = await tauri.invoke('sovereign_disable');
      // Offer the attestation for download - the compliance receipt.
      const blob = new Blob([JSON.stringify(att, null, 2)], { type: 'application/json' });
      const a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = `sovereign-attestation-${att.session_id || 'session'}.json`;
      a.click();
      URL.revokeObjectURL(a.href);
    } else {
      const findings = await tauri.invoke('sovereign_preflight');
      const blocking = renderPreflight(preflightEl, findings);
      const acked = [...preflightEl.querySelectorAll('.ack-checkbox')].every((cb) => cb.checked);
      const ackable = findings.filter((f) => f.acknowledgable).length;
      if (blocking > 0 || (ackable > 0 && !acked)) {
        // Do not engage until findings are resolved/acknowledged.
        toggleBtn.textContent = 'Resolve findings above first';
        return;
      }
      await tauri.invoke('sovereign_enable');
    }
    await refresh();
  });

  return { refresh };
}
