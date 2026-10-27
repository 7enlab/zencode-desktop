export const SOVEREIGN = Object.freeze({
  OLLAMA_URL: 'http://localhost:11434/v1',
  POLICY_VERSION: 'sovereign-policy-v1',
  ATTESTATION_SCHEME: 'hash-chain-v1',
});

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

export function canEnable(status) {
  return !!status && !status.enabled && !!status.ollama_reachable;
}

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

        toggleBtn.textContent = 'Resolve findings above first';
        return;
      }
      await tauri.invoke('sovereign_enable');
    }
    await refresh();
  });

  return { refresh };
}
