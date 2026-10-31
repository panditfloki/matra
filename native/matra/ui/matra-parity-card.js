/* The detail card's extra lines for the providers added for parity. Only reported fields are drawn:
   a null measurement has no line at all, and nothing here is estimated. */
((root) => {
  function plainEsc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  }
  function bytes(n) {
    if (!Number.isFinite(n) || n < 0) return null;
    const gb = n / 1073741824;
    return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(n / 1048576)} MB`;
  }
  const num = n => (Number.isFinite(n) ? n.toLocaleString('en-US') : null);
  function modelLine(m) {
    return [m.quantization, bytes(m.memory_bytes), Number.isFinite(m.context_length) ? `${num(m.context_length)} context` : null,
      Number.isFinite(m.tokens_per_second) ? `${m.tokens_per_second.toFixed(1)} tok/s` : null,
      m.phase === 'generating' ? 'Generating' : m.phase === 'processingPrompt' ? 'Reading prompt' : null,
      Number.isFinite(m.queued) && m.queued > 0 ? `${m.queued} queued` : null].filter(Boolean).join(' · ');
  }
  function detailLines(snap, id) {
    const d = snap.details && typeof snap.details === 'object' ? snap.details : {};
    const out = [];
    if (id === 'deepseek') {
      for (const c of Array.isArray(d.currencies) ? d.currencies : []) if (c && c.currency) out.push(`${c.currency} balance: ${c.total}`);
      const phase = d.pricing_phase && d.pricing_phase.phase;
      if (phase) out.push(phase === 'peak' ? 'Peak pricing now' : 'Off-peak pricing now');
    }
    if (id === 'ollama') {
      if (d.local === 'offline') out.push('Local server is offline');
      if (d.cloud === 'noKey') out.push('No cloud key set');
      if (d.cloud === 'needsAuth') out.push('Cloud key was rejected');
    }
    if (id === 'omlx') {
      if (d.runtime_host === 'remote') out.push('Monitoring oMLX on a remote Mac');
      if (d.activity === 'needsKey') out.push('Activity needs an API key');
    }
    if (id === 'higgsfield') {
      if (d.workspace) out.push(`Workspace: ${d.workspace}`);
      if (Number.isFinite(d.credits_left)) out.push(`${num(d.credits_left)} credits left`);
    }
    if (id === 'grokbot' && Number.isFinite(d.percent_used) && d.percent_used > 100) out.push(`${Math.round(d.percent_used)}% used, over the allowance`);
    if (String(id).startsWith('custom:')) {
      if (d.health) out.push(`Health: ${d.health}`);
      if (Number.isFinite(d.latency_ms)) out.push(`${d.latency_ms} ms`);
    }
    return out;
  }
  function html(snap, id, esc = plainEsc) {
    if (!snap || typeof snap !== 'object') return '';
    const rows = [];
    if (snap.plan) rows.push(`<div class="c-note">${esc(snap.plan)}</div>`);
    const local = snap.local_model && Array.isArray(snap.local_model.models) ? snap.local_model.models : [];
    for (const m of local.slice(0, 6)) {
      const line = modelLine(m);
      rows.push(`<div class="win"><div class="w-row"><span class="w-label">${esc(m.name || m.model_key || 'Model')}</span></div>${line ? `<div class="w-used">${esc(line)}</div>` : ''}</div>`);
    }
    if (local.length > 6) rows.push(`<div class="s-more">and ${local.length - 6} more</div>`);
    const p = snap.performance && typeof snap.performance === 'object' ? snap.performance : {};
    const perf = [Number.isFinite(p.generation_tokens_per_second) ? `${p.generation_tokens_per_second.toFixed(1)} tok/s` : null,
      Number.isFinite(p.active_requests) ? `${p.active_requests} active` : null,
      Number.isFinite(p.queued_requests) ? `${p.queued_requests} queued` : null,
      Number.isFinite(p.total_tokens) ? `${num(p.total_tokens)} tokens` : null].filter(Boolean);
    if (perf.length) rows.push(`<div class="w-used">${esc(perf.join(' · '))}</div>`);
    for (const line of detailLines(snap, id)) rows.push(`<div class="w-used">${esc(line)}</div>`);
    return rows.length ? `<div class="c-sessions">${rows.join('')}</div>` : '';
  }
  root.MatraParityCard = { html, bytes };
  if (typeof module !== 'undefined') module.exports = root.MatraParityCard;
})(typeof window === 'undefined' ? globalThis : window);
