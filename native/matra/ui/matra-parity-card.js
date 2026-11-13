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
  function counts(t) {
    if (!t || !Number.isFinite(t.requests)) return '';
    const parts = [`${num(t.requests)} observed responses`];
    for (const [key, label, measured] of [['input_tokens','input','input_measured'],['output_tokens','output','output_measured'],['reasoning_tokens','reasoning','reasoning_measured'],['draft_tokens','draft','draft_measured'],['accepted_draft_tokens','accepted draft','accepted_draft_measured']]) {
      if (Number.isFinite(t[key])) parts.push(`${num(t[key])} ${label}${measured && t[measured] < t.requests ? ` (${t[measured]}/${t.requests} measured)` : ''}`);
    }
    if (t.reasoning_measured===t.requests && t.output_measured===t.requests && t.output_tokens > 0 && t.reasoning_tokens <= t.output_tokens) parts.push(`${Math.round(t.reasoning_tokens/t.output_tokens*100)}% reported reasoning share`);
    if (t.draft_measured===t.requests && t.accepted_draft_measured===t.requests && t.draft_tokens > 0 && t.accepted_draft_tokens <= t.draft_tokens) parts.push(`${Math.round(t.accepted_draft_tokens/t.draft_tokens*100)}% draft acceptance`);
    return parts.join(' · ');
  }
  function response(p) {
    if (!p) return '';
    return [Number.isFinite(p.input_tokens) ? `${num(p.input_tokens)} input` : 'input unknown',
      Number.isFinite(p.output_tokens) ? `${num(p.output_tokens)} output` : 'output unknown',
      Number.isFinite(p.reasoning_tokens) ? `${num(p.reasoning_tokens)} reasoning` : null,
      Number.isFinite(p.tokens_per_second) ? `${p.tokens_per_second.toFixed(1)} tok/s` : null,
      Number.isFinite(p.time_to_first_token) ? `${p.time_to_first_token.toFixed(2)} s to first token` : null,
      Number.isFinite(p.generation_seconds) ? `${p.generation_seconds.toFixed(2)} s generation` : null].filter(Boolean).join(' · ');
  }
  function historyHtml(history, esc = plainEsc, detailed = false, range = 'month', now = Date.now()) {
    if (!history || typeof history !== 'object') return '';
    const h = history, rows = [`<p class="c-note">${esc(h.message || 'Response history is unavailable.')}</p>`];
    if (h.save_failed) rows.push('<p class="c-note">Counts could not be saved. This reading remains in memory.</p>');
    if (h.model_list_truncated) rows.push('<p class="c-note">Showing the 128 most recently observed models. Daily totals include all retained observations.</p>');
    if (h.today && h.today.requests > 0) rows.push(`<p class="w-used">Today: ${esc(counts(h.today))}</p>`);
    else if (h.state === 'ready') rows.push('<p class="w-used">No completed responses observed today.</p>');
    if (!detailed) {
      for(const model of (Array.isArray(h.models)?h.models:[]).slice(0,3))if(model.last)rows.push(`<p class="w-used">Last ${esc(model.model)} response: ${esc(response(model.last))}</p>`);
      return `<section class="local-history" aria-label="Local response history">${rows.join('')}</section>`;
    }
    const since = new Date(now); since.setHours(0,0,0,0);
    if (range === 'week') since.setDate(since.getDate() - (since.getDay()+6)%7); else if (range !== 'day') since.setDate(1);
    const inRange = day => typeof day === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(day) && new Date(day + 'T00:00:00').getTime() >= since.getTime();
    for (const model of (Array.isArray(h.models) ? h.models : []).slice(0,128)) {
      const days = (Array.isArray(model.days) ? model.days : []).filter(d => inRange(d.day)).slice(-60).reverse();
      if (!days.length) continue;
      rows.push(`<h3 class="dash-h">${esc(model.model)}</h3>`);
      if (model.last && Number.isFinite(model.last.at)) rows.push(`<p class="w-used">Last observed response · ${esc(new Date(model.last.at).toLocaleString())}<br>${esc(response(model.last))}</p>`);
      if (Number.isFinite(model.last_context_fraction) && Number.isFinite(model.context_length)) rows.push(`<p class="w-used">Last input: ${esc(Math.round(model.last_context_fraction*100))}% of the reported ${esc(num(model.context_length))}-token context.</p>`);
      rows.push(`<table style="width:100%;text-align:left;font-size:12px"><caption>Observed tokens by local day</caption><thead><tr><th>Day</th><th>Counts</th></tr></thead><tbody>${days.map(d=>`<tr><td>${esc(d.day)}</td><td>${esc(counts(d.totals))}</td></tr>`).join('')}</tbody></table>`);
    }
    const recent = (Array.isArray(h.recent) ? h.recent : []).filter(p=>Number.isFinite(p.at) && p.at >= since.getTime()).slice(0,50);
    if (recent.length) rows.push(`<details><summary>Recent completed responses (${recent.length})</summary>${recent.map(p=>`<p class="w-used"><b>${esc(p.model)}</b> · ${esc(new Date(p.at).toLocaleString())}<br>${esc(response(p))}</p>`).join('')}</details>`);
    return `<section class="local-history dash-gapped" aria-label="Local response history"><h2 class="dash-h">Local response history</h2>${rows.join('')}</section>`;
  }
  function modelLine(m, id) {
    return [m.quantization, bytes(m.memory_bytes) && `${id === 'lmstudio' ? 'Model size' : 'Memory'}: ${bytes(m.memory_bytes)}`, Number.isFinite(m.context_length) ? `${num(m.context_length)} context` : null,
      Number.isFinite(m.tokens_per_second) ? `${m.tokens_per_second.toFixed(1)} tok/s` : null,
      m.phase === 'generating' ? 'Generating' : m.phase === 'processingPrompt' ? 'Reading prompt' : m.phase === 'thinking' ? 'Thinking' : null,
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
      const line = modelLine(m, id);
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
    if (snap.details?.local_history) rows.push(historyHtml(snap.details.local_history, esc));
    return rows.length ? `<div class="c-sessions">${rows.join('')}</div>` : '';
  }
  root.MatraParityCard = { html, bytes, historyHtml, counts, response };
  if (typeof module !== 'undefined') module.exports = root.MatraParityCard;
})(typeof window === 'undefined' ? globalThis : window);
