'use strict';
const $ = id => document.getElementById(id);
let current, selected = 'claude', collapseTimer;
const names = { claude: 'Claude', codex: 'Codex', gemini: 'Gemini' };
function text(tag, value, className) { const e = document.createElement(tag); e.textContent = value; if (className) e.className = className; return e; }
function age(value) { if (!value) return 'No reading'; const minutes = Math.max(0, Math.floor((Date.now() - value) / 60000)); return minutes < 1 ? 'Just now' : `${minutes}m ago`; }
function reset(value) { if (!value) return 'Reset time unavailable'; const mins = Math.ceil((value - Date.now()) / 60000); if (mins <= 0) return 'Window expired'; return mins < 60 ? `Resets in ${mins}m` : `Resets in ${Math.floor(mins / 60)}h ${mins % 60}m`; }
function render(s) {
  current = s;
  document.body.classList.toggle('expanded', !!s.expanded);
  document.body.classList.toggle('horizontal', ['top', 'bottom'].includes(s.preferences?.edge));
  document.body.classList.toggle('left', s.preferences?.edge === 'left');
  $('pin').classList.toggle('on', !!s.pinned); $('pin').textContent = s.pinned ? 'Pinned' : 'Pin';
  $('refresh').disabled = !!s.loading;
  $('connection').textContent = s.loading ? 'Refreshing…' : `${s.backend} · ${age(s.model?.observedAt)}`;
  $('error').textContent = s.error || '';
  $('edge').value = s.preferences?.edge || 'right'; $('startup').checked = !!s.preferences?.startAtLogin;
  $('build').textContent = `Desktop ${s.version} · Local data · Preview`;
  $('rings').replaceChildren(); $('tabs').replaceChildren();
  const providers = s.model?.providers || Object.keys(names).map(id => ({ id, name: names[id], quota: { remaining: null, status: 'unavailable' } }));
  for (const p of providers) {
    const button = text('button', '', `cell ${p.quota.remaining === null ? 'unknown' : ''} ${p.quota.status} ${selected === p.id ? 'active' : ''}`);
    button.title = `${p.name}: ${p.quota.remaining === null ? 'quota unavailable' : `${Math.round(p.quota.remaining)}% remaining`}`;
    button.setAttribute('aria-label', button.title);
    const ring = text('div', '', 'ring');
    // The only HTML here is fixed markup and a bounded numeric stroke value.
    const percent = Number.isFinite(p.quota.remaining) ? p.quota.remaining : 0;
    ring.innerHTML = `<svg viewBox="0 0 44 44" aria-hidden="true"><circle class="track" cx="22" cy="22" r="19"/><circle class="progress" cx="22" cy="22" r="19" stroke-dasharray="${percent * 1.194} 119.4"/></svg>`;
    ring.append(text('span', { claude: '✳', codex: 'C', gemini: '✦' }[p.id], 'glyph'));
    button.append(ring, text('span', p.quota.remaining === null ? 'N/A' : `${Math.round(percent)}%`, 'reading'));
    button.addEventListener('mouseenter', () => {
      const changed = selected !== p.id; selected = p.id;
      if (!s.expanded) window.matra.action('expand');
      else if (changed) render(current);
    });
    button.addEventListener('click', () => { selected = p.id; window.matra.action('expand'); });
    $('rings').append(button);
    const tab = text('button', p.name, selected === p.id ? 'selected' : '');
    tab.onclick = () => { selected = p.id; render(current); }; $('tabs').append(tab);
  }
  const p = providers.find(p => p.id === selected);
  if (!p?.history) return;
  const content = $('content'); content.replaceChildren();
  const head = text('div', '', 'headline');
  if (p.quota.remaining !== null) {
    const number = text('div', String(Math.round(p.quota.remaining)), 'big'); number.append(text('small', '%'));
    head.append(number, text('span', p.quota.status === 'stale' ? 'STALE READING' : 'REMAINING', 'badge'));
    content.append(head, text('div', p.plan || p.name, 'caption'));
  } else content.append(text('div', 'Quota unavailable', 'muted'));
  if (p.quota.reason) content.append(text('p', p.quota.reason, 'notice'));
  for (const l of p.quota.limits) {
    const row = text('div', '', 'limit'), title = text('div', '', 'limit-title');
    title.append(text('span', l.label), text('span', l.expired ? 'Expired' : `${Math.round(l.remaining)}% left`));
    const bar = text('div', '', 'meter'), fill = text('div', '', 'fill'); fill.style.width = `${l.expired ? 0 : l.remaining}%`; bar.append(fill);
    row.append(title, bar, text('span', reset(l.resetsAt), 'reset')); content.append(row);
  }
  const stats = text('div', '', 'stats');
  for (const [label, value] of [['Recorded tokens', p.history.tokens === null ? 'Unavailable' : Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 }).format(p.history.tokens)], ['API equivalent', p.history.cost === null ? 'Unavailable' : `$${p.history.cost.toFixed(2)}`]]) {
    const stat = text('div', ''); stat.append(text('div', label, 'stat-label'), text('div', value, 'stat-value')); stats.append(stat);
  }
  content.append(stats, text('div', `Local history · ${p.history.status}. Cost is an estimate, not your bill.`, 'footnote'));
  if (p.history.reason) content.append(text('div', p.history.reason, 'footnote'));
  if (p.quota.fetchedAt) content.append(text('div', `Quota checked ${age(p.quota.fetchedAt)}`, 'footnote'));
}
document.addEventListener('mouseenter', () => clearTimeout(collapseTimer));
document.addEventListener('mouseleave', () => { collapseTimer = setTimeout(() => window.matra.action('collapse'), 500); });
document.addEventListener('keydown', e => { if (e.key === 'Escape') window.matra.action('collapse'); });
$('expand').onclick = () => window.matra.action('expand');
$('brand').onclick = $('dashboard').onclick = () => window.matra.action('dashboard');
$('pin').onclick = () => window.matra.action('pin');
$('refresh').onclick = () => window.matra.action('refresh');
$('settingsToggle').onclick = () => { $('settings').hidden = !$('settings').hidden; };
$('edge').onchange = e => window.matra.action('edge', e.target.value);
$('display').onclick = () => window.matra.action('display');
$('startup').onchange = async e => { const result = await window.matra.action('startup', e.target.checked); if (!result.packaged) { e.target.checked = false; $('build').textContent = 'Start at login is available in the packaged app.'; } };
window.matra.onState(render); window.matra.state().then(render);
setInterval(() => { if (current) render(current); }, 60000);
