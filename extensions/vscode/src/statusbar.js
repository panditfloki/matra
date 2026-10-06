'use strict';
// Status-bar text and hover card, in the v1.7 style: coloured bars that show
// what is LEFT, and a markdown card with a text meter per quota window.
// Pure functions, no vscode import, so the tests can run them directly.

const finite = value => typeof value === 'number' && Number.isFinite(value);
const { PROVIDERS } = require('./registry');
const LEG = Object.fromEntries(PROVIDERS.map(p => [p.id, p.legend]));
const SHORT = Object.fromEntries(PROVIDERS.map(p => [p.id, p.short]));

// The status bar takes plain text with no per-character colour, so the filled
// segments are coloured emoji squares and the empty track is the neutral one.
const SEGMENTS = 4;
const FILLED = { ok: '🟩', tight: '🟨', over: '🟥' };
const EMPTY = '⬜';

// Untrusted provider text must never become markdown structure or a link.
const md = value => String(value ?? '').replace(/[\\`*_{}[\]()#+\-.!|<>~]/g, c => `\\${c}`);
const tokens = n => n >= 1e9 ? `${(n / 1e9).toFixed(2)}B` : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}k` : String(n);
const usd = n => `$${n < 100 ? n.toFixed(2) : Math.round(n).toLocaleString('en-US')}`;

function headline(p) { return p.windows?.find(w => w.id === p.headlineId) || p.windows?.[0]; }

function left(w) {
  return finite(w?.usedPercent) ? Math.max(0, Math.min(100, Math.round(100 - w.usedPercent))) : null;
}

// One tone scale for the whole extension. The status bar and the hover card must
// never disagree about whether a number is healthy, so both read this.
function toneOf(left) { return left <= 10 ? 'over' : left <= 30 ? 'tight' : 'ok'; }

function bar(value) {
  const filled = Math.round(value / 100 * SEGMENTS);
  return FILLED[toneOf(value)].repeat(filled) + EMPTY.repeat(SEGMENTS - filled);
}

// ── Hover-card bars ─────────────────────────────────────────────────────────
// A status-bar tooltip is a MarkdownString, which cannot draw a real progress
// bar. The emoji meter above works in the bar itself (plain text, no colour
// control) but reads as a row of squares in a card. So the card paints each bar
// as coloured HTML cells: a run of &nbsp; inside a span with a background.
// This needs the caller to set `supportHtml` on the MarkdownString, and the
// colours are VS Code theme variables so the card follows the user's theme
// instead of hard-coding a palette that breaks in light mode.
const BAR_CELLS = 20;
const TRACK = '--vscode-scrollbarSlider-background';
const TONE_VAR = {
  ok: '--vscode-charts-green',
  tight: '--vscode-charts-yellow',
  over: '--vscode-charts-red',
};

const cells = (count, variable) => count > 0
  ? `<span style="background-color:var(${variable});border-radius:3px;">${'&nbsp;'.repeat(count)}</span>`
  : '';

// An unknown reading is a full neutral track, never an empty green one: a bar
// that reads "0% left" and a bar that reads "we could not measure this" must
// not look the same.
function htmlBar(left) {
  if (!finite(left)) return cells(BAR_CELLS, TRACK);
  const value = Math.max(0, Math.min(100, left));
  const filled = value > 0 ? Math.max(1, Math.round(value / 100 * BAR_CELLS)) : 0;
  return cells(filled, TONE_VAR[toneOf(value)]) + cells(BAR_CELLS - filled, TRACK);
}

const toneText = (text, left) => finite(left)
  ? `<span style="color:var(${TONE_VAR[toneOf(left)]});">${text}</span>`
  : text;

// A provider that is not installed or not running (no reading, no account)
// stays off the bar. A signed-in one without a reading, or one that needs
// sign-in or failed, keeps a "—" so the gap stays visible.
function shown(p) {
  if (p.status !== 'unavailable') return true;
  return Boolean(p.windows?.some(w => finite(w.usedPercent)) || p.account?.plan || p.account?.label);
}

// With every installed tool on, the bar would outgrow the window. Pinned ids
// (matra.statusBarProviders) win; otherwise a crowded bar keeps only the
// providers that actually have a reading. The hover card always lists all.
const CROWDED = 4;
function legsFor(snapshot, pinned) {
  const list = (snapshot.providers || []).filter(shown);
  if (Array.isArray(pinned) && pinned.length) return list.filter(p => pinned.includes(p.id));
  return list.length > CROWDED ? list.filter(p => left(headline(p)) !== null) : list;
}

function statusLabel(snapshot, { bars = true, pinned = [] } = {}) {
  const legs = legsFor(snapshot, pinned).map(p => {
    const name = LEG[p.id] || p.name;
    const value = left(headline(p));
    if (value === null) return `${name} —`;
    const stale = p.status === 'stale' ? '~' : '';
    return bars ? `${name} ${bar(value)} ${value}%${stale}` : `${name} ${value}%${stale}`;
  });
  return legs.length ? legs.join(' · ') : 'Mātrā: connect';
}

function accessibleLabel(snapshot) {
  const parts = (snapshot.providers || []).filter(shown).map(p => {
    const value = left(headline(p));
    return `${SHORT[p.id] || p.name} ${value === null ? 'no reading' : `${value}% left`}`;
  });
  return parts.length ? `Mātrā usage: ${parts.join(', ')}` : 'Mātrā: connect a provider';
}

// Same thresholds as before, written in "used" terms, across every window.
function severity(snapshot, warning = 80, error = 95) {
  const used = (snapshot.providers || []).flatMap(p => p.windows || []).map(w => w.usedPercent).filter(finite);
  const highest = used.length ? Math.max(...used) : null;
  return highest !== null && highest >= error ? 'error' : highest !== null && highest >= warning ? 'warning' : 'normal';
}

function meter(used) {
  const filled = Math.round(Math.min(100, Math.max(0, used)) / 10);
  return '█'.repeat(filled) + '░'.repeat(10 - filled);
}

function until(ms) {
  const mins = Math.max(1, Math.ceil(ms / 60000));
  const d = Math.floor(mins / 1440), h = Math.floor(mins % 1440 / 60), m = mins % 60;
  if (d) return h ? `${d}d ${h}h` : `${d}d`;
  return h ? `${h}h ${m}m` : `${m}m`;
}

function reset(at, mode, now, locale, timeZone) {
  if (!finite(at)) return '';
  if (at <= now) return ' · resetting now';
  if (mode === 'date') {
    const year = new Intl.DateTimeFormat(locale, { year: 'numeric', timeZone });
    const showYear = year.format(at) !== year.format(now);
    const when = new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', ...(showYear ? { year: 'numeric' } : {}), hour: 'numeric', minute: '2-digit', timeZone }).format(at);
    return ` · resets ${md(when)}`;
  }
  return ` · resets in ${until(at - now)}`;
}

// The table wants the reset on its own, without the " · resets " glue that the
// old stacked layout needed.
function resetCell(at, mode, now, locale, timeZone) {
  const text = reset(at, mode, now, locale, timeZone);
  return text ? text.replace(/^ · (resets in |resets )?/, '') : '';
}

const STATES = { stale: 'Stale reading', 'needs-auth': 'Sign in needed', error: 'Read failed', unavailable: 'Not available' };

// One table, so every provider's windows line up in the same columns. The
// provider name sits only on its first row, which is what groups the rows
// visually without a heading per provider.
//
// ⚠️ `md()` escapes `|`, so untrusted provider text cannot break out of a cell
// or forge a column. Do not interpolate raw provider strings here.
function tooltipMarkdown(snapshot, { mode = 'remaining', now = Date.now(), locale, timeZone, intervalSeconds = 60 } = {}) {
  const providers = snapshot.providers || [];
  if (!providers.length) return '**$(pulse) Mātrā**\n\nNo providers selected.';

  const rows = [];
  const notes = [];
  let costs = false;

  for (const p of providers) {
    const plan = p.account?.plan ? ` _(${md(p.account.plan)})_` : '';
    let nameCell = `**${md(p.name)}**${plan}`;
    const windows = p.windows || [];

    if (!windows.length) {
      rows.push(`| ${nameCell} | | ${htmlBar(null)} | — | |`);
    }
    for (const w of windows) {
      const value = finite(w.usedPercent) ? Math.max(0, Math.min(100, Math.round(100 - w.usedPercent))) : null;
      const percent = value === null ? '—' : toneText(`**${value}%**`, value);
      rows.push(`| ${nameCell} | ${md(w.label)} | ${htmlBar(value)} | ${percent} | ${md(resetCell(w.resetsAt, mode, now, locale, timeZone))} |`);
      nameCell = '';   // only the first row of a provider carries its name
    }

    const detail = [];
    if (p.account?.label) detail.push(md(p.account.label));
    if (p.status !== 'ready') detail.push(`_${STATES[p.status] || 'Read failed'}${p.message ? `: ${md(p.message)}` : ''}_`);
    const u = p.usage;
    if (u && finite(u.totalTokens)) {
      const cost = finite(u.costUsd) ? `${usd(u.costUsd)} · ` : '';
      if (cost) costs = true;
      const sessions = Array.isArray(p.sessions) && p.sessions.length
        ? ` over ${p.sessions.length} ${p.sessions.length === 1 ? 'session' : 'sessions'}` : '';
      detail.push(`${md(u.period || 'Recorded')}: ${cost}${tokens(u.totalTokens)} tokens${sessions}`);
    }
    if (detail.length) notes.push(`**${md(p.name)}** · ${detail.join(' · ')}`);
  }

  const out = [
    '**$(pulse) Mātrā**',
    `| | | | % left | Resets |\n|:--|:--|:--|--:|:--|\n${rows.join('\n')}`,
  ];
  if (notes.length) out.push(notes.join('\n\n'));
  if (costs) out.push('*Costs are equivalent API cost, a burn proxy, not your subscription bill.*');

  // Command links need `isTrusted` on the MarkdownString. Stating the cadence
  // is not decoration: without it a card that has not moved looks broken.
  out.push([
    '[$(dashboard) Dashboard](command:matra.open)',
    '[$(refresh) Refresh](command:matra.refresh)',
    `_${cadence(intervalSeconds)}_`,
  ].join(' · '));
  return out.join('\n\n');
}

// Returns the whole phrase, not just the interval: "every 0 seconds" is not a
// cadence, it is auto-refresh being off, and that needs different words.
function cadence(seconds) {
  if (!finite(seconds) || seconds <= 0) return 'updates manually';
  if (seconds < 60) return `updates every ${Math.round(seconds)}s`;
  const mins = Math.round(seconds / 60);
  return mins === 1 ? 'updates every minute' : `updates every ${mins} min`;
}

module.exports = { statusLabel, accessibleLabel, severity, tooltipMarkdown, meter, htmlBar, toneOf };
