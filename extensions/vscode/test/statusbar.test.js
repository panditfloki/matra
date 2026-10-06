'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { statusLabel, accessibleLabel, severity, tooltipMarkdown, meter, htmlBar, toneOf } = require('../src/statusbar');
const now = Date.parse('2026-10-05T10:00:00Z');
const snapshot = { providers: [
  { id: 'claude', name: 'Claude Code', status: 'ready', headlineId: 'session', account: { plan: 'Max (20x)', label: 'us…@example.com' },
    windows: [{ id: 'session', label: 'Current session', usedPercent: 13, resetsAt: now + (3 * 60 + 44) * 60000 },
      { id: 'weekly_all', label: 'Weekly · all models', usedPercent: 36, resetsAt: now + 104 * 60000 }],
    usage: { period: 'Today (local device)', totalTokens: 31.5e6, costUsd: null }, sessions: [{}, {}] },
  { id: 'codex', name: 'Codex', status: 'ready', headlineId: 'session', windows: [{ id: 'session', label: 'Weekly limit', usedPercent: 53, resetsAt: now + (5 * 24 + 1) * 3600000 }] },
  { id: 'cursor', name: 'Cursor', status: 'needs-auth', message: 'Sign in again in Cursor, then refresh.', windows: [] },
  { id: 'antigravity', name: 'Antigravity', status: 'unavailable', message: 'Not running.', windows: [] }
] };

test('status bar shows coloured bars of quota left, like v1.7', () => {
  assert.equal(statusLabel(snapshot), 'C 🟩🟩🟩⬜ 87% · X 🟩🟩⬜⬜ 47% · CU —');
  assert.equal(statusLabel(snapshot, { bars: false }), 'C 87% · X 47% · CU —');
  assert.equal(statusLabel({ providers: [] }), 'Mātrā: connect');
});
test('bar colour tightens as quota runs out, and over-limit is clamped', () => {
  const one = used => statusLabel({ providers: [{ id: 'codex', status: 'ready', windows: [{ usedPercent: used }] }] });
  assert.equal(one(75), 'X 🟩⬜⬜⬜ 25%'.replace('🟩', '🟨'));
  assert.equal(one(95), 'X ⬜⬜⬜⬜ 5%');
  assert.equal(one(120), 'X ⬜⬜⬜⬜ 0%');
  assert.match(statusLabel({ providers: [{ id: 'claude', status: 'stale', windows: [{ usedPercent: 50 }] }] }), /50%~$/);
});
test('a signed-in provider without a reading keeps its dash', () => {
  assert.equal(statusLabel({ providers: [{ id: 'claude', status: 'unavailable', account: { plan: 'Max' }, windows: [] }] }), 'C —');
});
test('screen readers get words, not emoji', () => {
  assert.equal(accessibleLabel(snapshot), 'Mātrā usage: Claude 87% left, Codex 47% left, Cursor no reading');
});
test('warning colour follows the highest used window', () => {
  assert.equal(severity(snapshot), 'normal');
  assert.equal(severity({ providers: [{ windows: [{ usedPercent: 82 }] }] }), 'warning');
  assert.equal(severity({ providers: [{ windows: [{ usedPercent: 97 }] }] }), 'error');
});
test('hover card is one aligned table, with the plan inline and the name grouping its windows', () => {
  const card = tooltipMarkdown(snapshot, { now });
  assert.match(card, /^\*\*\$\(pulse\) Mātrā\*\*/);
  assert.match(card, /\| \| \| \| % left \| Resets \|\n\|:--\|:--\|:--\|--:\|:--\|/);
  // Plan rides in the name cell, as in the reference card: "Claude Code (Max (20x))".
  assert.match(card, /\| \*\*Claude Code\*\* _\(Max \\\(20x\\\)\)_ \| Current session \|/);
  // The second window of the same provider repeats no name: that is the grouping.
  assert.match(card, /\n\|  \| Weekly · all models \|/);
  assert.match(card, /3h 44m \|/);
  assert.match(card, /5d 1h \|/);
  assert.match(card, /31\.5M tokens over 2 sessions/);
  assert.match(card, /_Sign in needed: Sign in again in Cursor, then refresh\\\._/);
  // Footer is real commands, not the old "Click to open the dashboard." prose.
  assert.match(card, /\[\$\(dashboard\) Dashboard\]\(command:matra\.open\)/);
  assert.match(card, /\[\$\(refresh\) Refresh\]\(command:matra\.refresh\)/);
  assert.ok(!card.includes('—'.repeat(2)));
});

test('the card shows what is LEFT, agreeing with the status bar on the same window', () => {
  // The old card said 13% (used) for the window the status bar called 87% (left).
  // One number, one direction, everywhere.
  const card = tooltipMarkdown(snapshot, { now });
  const label = statusLabel(snapshot, { pinned: ['claude'] });
  assert.match(label, /87%/);
  assert.match(card, />\*\*87%\*\*</);
  assert.ok(!card.includes('**13%**'), 'the card must not report the used figure');
});

test('bars are coloured HTML cells, and an unknown reading is a full neutral track', () => {
  // A MarkdownString cannot draw a bar, so each one is a run of &nbsp; inside a
  // span. Twenty cells, filled from the tone scale, remainder on the track.
  assert.equal((htmlBar(100).match(/&nbsp;/g) || []).length, 20);
  assert.equal((htmlBar(0).match(/&nbsp;/g) || []).length, 20);
  assert.match(htmlBar(90), /--vscode-charts-green/);
  assert.match(htmlBar(25), /--vscode-charts-yellow/);
  assert.match(htmlBar(5), /--vscode-charts-red/);
  // 0% left still earns a red reading elsewhere, but its own bar has no fill.
  assert.ok(!htmlBar(0).includes('--vscode-charts-'), 'an empty bar paints no tone');
  // Unknown is a full neutral track, never an empty coloured one: "nothing left"
  // and "we could not measure this" must not look the same.
  const unknown = htmlBar(null);
  assert.equal((unknown.match(/&nbsp;/g) || []).length, 20);
  assert.ok(!unknown.includes('--vscode-charts-'));
  assert.equal(unknown, htmlBar(undefined));
});

test('the tone scale is shared with the status bar', () => {
  assert.equal(toneOf(100), 'ok');
  assert.equal(toneOf(31), 'ok');
  assert.equal(toneOf(30), 'tight');
  assert.equal(toneOf(11), 'tight');
  assert.equal(toneOf(10), 'over');
  assert.equal(toneOf(0), 'over');
});

test('date mode shows the reset date instead of a countdown', () => {
  const card = tooltipMarkdown(snapshot, { now, mode: 'date', locale: 'en-IN', timeZone: 'Asia/Kolkata' });
  assert.match(card, /5 Oct/);
  assert.ok(!card.includes('resets 5 Oct'), 'the table column is the label, so the cell is bare');
});

test('provider text cannot inject markdown, a command link, or a table column', () => {
  const card = tooltipMarkdown({ providers: [{ id: 'claude', name: '[x](command:evil)', status: 'error', message: '# heading', windows: [{ label: 'a|b|c', usedPercent: null }] }] }, { now });
  assert.ok(!card.includes('[x](command:evil)'));
  assert.match(card, /\\\[x\\\]\\\(command:evil\\\)/);
  // The pipe is the one that matters here: an unescaped one forges a column and
  // silently shifts every later value into the wrong heading.
  assert.ok(!card.includes('a|b|c'));
  assert.match(card, /a\\\|b\\\|c/);
});

test('provider text stays escaped, which is what lets the card enable HTML at all', () => {
  // The card is rendered with supportHtml AND isTrusted so the bars can be
  // coloured spans and the footer can be command links. Those two flags are
  // only safe because every provider-sourced string goes through md(). If this
  // test ever fails, turn both flags off in extension.js before shipping: a
  // provider name would otherwise be able to inject a tag or a command link.
  const evil = '<img src=x onerror=alert(1)> [go](command:workbench.action.terminal.new)';
  const card = tooltipMarkdown({ providers: [{ id: 'claude', name: evil, status: 'ready', windows: [{ label: evil, usedPercent: 50 }] }] }, { now });
  const escaped = '\\<img src=x onerror=alert\\(1\\)\\> \\[go\\]\\(command:workbench\\.action\\.terminal\\.new\\)';
  assert.ok(card.includes(escaped), 'provider text must render fully backslash-escaped');
  // And the only raw spans in the card are the ones this module writes itself.
  const ours = /^<span style="(background-color|color):var\(--vscode-[A-Za-z-]+\)/;
  const spans = card.match(/<span [^>]*>/g) || [];
  assert.ok(spans.length > 0, 'the card should contain our own bar spans');
  const foreign = spans.filter(s => !ours.test(s));
  assert.deepEqual(foreign, [], 'the only raw spans in the card must be the ones this module writes');
});

test('the footer states the real refresh cadence', () => {
  assert.match(tooltipMarkdown(snapshot, { now }), /_updates every minute_/);
  assert.match(tooltipMarkdown(snapshot, { now, intervalSeconds: 300 }), /_updates every 5 min_/);
  assert.match(tooltipMarkdown(snapshot, { now, intervalSeconds: 30 }), /_updates every 30s_/);
  assert.match(tooltipMarkdown(snapshot, { now, intervalSeconds: 0 }), /_updates manually_/);
});
test('text meter is bounded', () => {
  assert.equal(meter(-5), '░'.repeat(10));
  assert.equal(meter(150), '█'.repeat(10));
});
