'use strict';
(function (root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.MatraView = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
  const finite = value => typeof value === 'number' && Number.isFinite(value);
  const escape = value => String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const number = n => finite(n) ? new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 }).format(n) : 'Unavailable';
  const pct = n => finite(n) ? `${Math.round(n * 10) / 10}%` : 'Unavailable';
  function resetTime(value, mode = 'remaining', now = Date.now(), locale, timeZone) {
    if (!finite(value) || !Number.isFinite(new Date(value).getTime())) return 'Reset time unavailable';
    if (value <= now) return 'Resetting…';
    if (mode === 'date') {
      const year = new Intl.DateTimeFormat(locale, { year: 'numeric', timeZone });
      const showYear = year.format(value) !== year.format(now);
      return `Resets ${new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', ...(showYear ? { year: 'numeric' } : {}), hour: 'numeric', minute: '2-digit', timeZone }).format(value)}`;
    }
    const minutes = Math.ceil((value - now) / 60000);
    const days = Math.floor(minutes / 1440), hours = Math.floor(minutes % 1440 / 60), mins = minutes % 60;
    if (days) return `${days} ${days === 1 ? 'day' : 'days'} ${hours}h left`;
    if (hours) return `${hours}h ${mins}m left`;
    return `${minutes}m left`;
  }
  function age(value, now) {
    if (!finite(value)) return 'Not yet read';
    const mins = Math.max(0, Math.floor((now - value) / 60000));
    if (mins < 1) return 'Just now';
    if (mins < 60) return `${mins}m ago`;
    const h = Math.floor(mins / 60);
    return h < 24 ? `${h}h ago` : `${Math.floor(h / 24)}d ago`;
  }
  function headline(provider) { return provider.windows?.find(w => w.id === provider.headlineId) || provider.windows?.[0]; }
  function statusText(snapshot) {
    const items = (snapshot.providers || []).map(p => {
      const h = headline(p);
      return finite(h?.usedPercent) ? `${p.id === 'antigravity' ? 'AG' : p.id === 'claude' ? 'Claude' : p.name} ${pct(h.usedPercent)}${p.status === 'stale' ? '~' : ''}` : null;
    }).filter(Boolean);
    return items.length ? items.join(' · ') : 'Mātrā: connect';
  }
  function windowRow(w, mode, now) {
    const known = finite(w.usedPercent);
    const width = known ? Math.min(100, Math.max(0, w.usedPercent)) : 0;
    const tone = !known ? 'unknown' : w.usedPercent >= 95 ? 'danger' : w.usedPercent >= 80 ? 'warning' : 'healthy';
    return `<div class="quota-window"><div class="window-top"><strong>${escape(w.label)}</strong><span>${known ? `${escape(pct(w.usedPercent))} used` : 'Unavailable'}</span></div>
      <div class="track ${tone}"${known ? ` role="meter" aria-label="${escape(w.label)} usage" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${width}" aria-valuetext="${escape(pct(w.usedPercent))} used"` : ''}><progress max="100" value="${width}" aria-hidden="true"></progress></div>
      <div class="window-bottom"><span>${known ? `${escape(pct(Math.max(0, 100 - w.usedPercent)))} left` : 'No reading'}</span><span class="reset">${escape(resetTime(w.resetsAt, mode, now))}</span></div></div>`;
  }
  function details(provider, now, expanded) {
    const u = provider.usage, sessions = Array.isArray(provider.sessions) ? provider.sessions.slice(0, 20) : [];
    if (!u && !sessions.length) return '';
    const metrics = u ? `<div class="usage-grid"><div><span>${escape(u.period || 'Recorded')} tokens</span><strong>${number(u.totalTokens)}</strong></div><div><span>Input / output</span><strong>${number(u.inputTokens)} / ${number(u.outputTokens)}</strong></div>${finite(u.cachedTokens) ? `<div><span>Cached input</span><strong>${number(u.cachedTokens)}</strong></div>` : ''}${finite(u.costUsd) ? `<div><span>Estimated API cost</span><strong>${escape(new Intl.NumberFormat(undefined, { style: 'currency', currency: 'USD' }).format(u.costUsd))}</strong></div>` : ''}</div>` : '';
    return `<details data-provider="${escape(provider.id)}"${expanded.includes(provider.id) ? ' open' : ''}><summary>Usage & recent sessions <span>${sessions.length || ''}</span></summary>${metrics}${sessions.length ? `<ul class="sessions">${sessions.map(s => `<li><div><strong>${escape(s.label || 'Local session')}</strong><span>${escape(s.detail || '')}</span></div><div class="session-meta"><span>${escape(age(s.lastActiveAt, now))}</span>${finite(s.tokens) ? `<span>${number(s.tokens)} tokens</span>` : ''}</div></li>`).join('')}</ul>` : ''}<p class="footnote">Local records can be partial. Recent activity does not mean a session is still running.${finite(u?.costUsd) ? ' Cost is an API estimate, not your subscription bill.' : ''}</p></details>`;
  }
  function providerCard(p, mode, now, expanded) {
    const states = { ready: 'Connected', stale: 'Stale reading', unavailable: 'Not available', 'needs-auth': 'Sign in needed', error: 'Read failed' };
    const status = Object.hasOwn(states, p.status) ? p.status : 'error';
    const monograms = { claude: 'C', codex: 'O', cursor: '↗', antigravity: 'A' };
    const windows = Array.isArray(p.windows) ? p.windows : [];
    return `<section class="provider" aria-label="${escape(p.name)}"><header class="provider-heading"><div class="provider-mark ${escape(p.id)}" aria-hidden="true">${escape(monograms[p.id] || p.monogram || 'M')}</div><div class="provider-title"><h2>${escape(p.name)}</h2><p>${escape(p.account?.plan || p.source || 'Local usage')}</p></div><span class="state ${status}">${states[status]}</span></header>
      ${p.message ? `<p class="provider-message ${status}">${escape(p.message)}</p>` : ''}
      ${windows.length ? windows.map(w => windowRow(w, mode, now)).join('') : `<p class="empty-reading">${status === 'ready' ? 'No quota window reported.' : 'Usage will appear when this provider is available.'}</p>`}
      <footer class="provider-footer"><span>${escape(age(p.updatedAt, now))}</span><button class="text-button" data-action="providerHelp" data-provider="${escape(p.id)}">Setup help <span aria-hidden="true">↗</span></button></footer>
      ${details(p, now, expanded)}</section>`;
  }
  function render(snapshot, { mode = 'remaining', now = Date.now(), expanded = [], refreshing = false } = {}) {
    const providers = snapshot?.providers || [];
    const count = providers.filter(p => p.status === 'ready').length;
    return `<div class="shell"><header class="masthead"><div><div class="eyebrow">YOUR AI, MEASURED</div><h1>Mātrā<span class="brand-dot" aria-hidden="true"></span></h1><p>Usage across your tools.</p></div><button class="icon-button" data-action="refresh" aria-label="Refresh usage" title="Refresh usage"${refreshing ? ' disabled' : ''}><span aria-hidden="true">↻</span></button></header>
      <div class="overview"><span><i class="dot" aria-hidden="true"></i>${count} of ${providers.length} connected</span><button class="text-button" data-action="providers">Manage providers</button></div>
      <div class="display-control"><span>Reset display</span><div class="segmented" role="group" aria-label="Reset display"><button data-action="setResetMode" data-mode="date" aria-pressed="${mode === 'date'}">Date & time</button><button data-action="setResetMode" data-mode="remaining" aria-pressed="${mode === 'remaining'}">Time left</button></div></div>
      <main class="providers">${providers.length ? providers.map(p => providerCard(p, mode, now, expanded)).join('') : '<section class="empty-state"><h2>Your tools, in one place.</h2><p>Choose the providers you use to see their limits and reset times here.</p><button class="primary-button" data-action="providers">Choose providers</button></section>'}</main>
      <footer class="page-footer"><span>Standalone · Local account connections</span><button class="text-button" data-action="settings">Settings</button></footer><div class="sr-only" role="status" aria-live="polite">${refreshing ? 'Refreshing usage' : 'Usage updated'}</div></div>`;
  }
  return { escape, resetTime, age, headline, statusText, render, finite };
});
