'use strict';

// Presentation contract shared by desktop platforms. Preserve zero and unknown.
function provider(id, raw = {}, quota, now = Date.now(), platform = process.platform) {
  const limits = (quota?.limits || []).filter(l => Number.isFinite(l.percent)).map(l => {
    const expired = !!l.resetsAt && Number(l.resetsAt) <= now;
    return { label: l.label || l.kind, remaining: Math.max(0, Math.min(100,
      l.remaining === true ? l.percent : 100 - l.percent)), resetsAt: l.resetsAt || null, expired };
  });
  const valid = limits.filter(l => !l.expired);
  const unavailable = id === 'gemini' && platform === 'win32'
    ? 'Live Gemini quota is not implemented on Windows yet. History is separate.'
    : id === 'codex' ? 'No current limit window found. Use Codex, then refresh.'
    : 'No quota reading available. Check Claude sign-in and try again.';
  const stale = !!quota?.stale;
  return {
    id, name: { claude: 'Claude', codex: 'Codex', gemini: 'Gemini' }[id],
    plan: quota?.plan || null,
    quota: { status: valid.length ? (stale ? 'stale' : 'ready') : 'unavailable',
      remaining: valid.length ? Math.min(...valid.map(l => l.remaining)) : null,
      limits, fetchedAt: quota?.fetchedAt || null,
      reason: valid.length ? (quota?.staleReason || null) : unavailable },
    history: { status: raw.available === false ? 'unavailable' : raw.stale ? 'stale' : raw.totals ? 'ready' : 'unavailable',
      reason: raw.staleReason || null, fetchedAt: raw.fetchedAt || null,
      tokens: Number.isFinite(raw.totals?.tokens) ? raw.totals.tokens : null,
      cost: Number.isFinite(raw.totals?.cost) ? raw.totals.cost : null,
      sessions: Array.isArray(raw.sessions) ? raw.sessions.length : null },
  };
}

function desktopModel(data, now = Date.now(), platform = process.platform) {
  const totals = data.totals ? { ...data.totals, tokens: ['input', 'output', 'cacheRead', 'cacheWrite']
    .reduce((sum, key) => sum + (Number(data.totals[key]) || 0), 0) } : null;
  return { observedAt: now, providers: [
    provider('claude', { totals }, data.quota, now, platform),
    provider('codex', data.codex, data.codex?.quota, now, platform),
    provider('gemini', data.gemini, data.gemini?.quota, now, platform),
  ] };
}
module.exports = { provider, desktopModel };
