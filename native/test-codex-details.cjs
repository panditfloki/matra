'use strict';

// Synthetic only: run with `node native/test-codex-details.cjs` from the repository root.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, 'matra/ui/notch.html'), 'utf8');
const match = source.match(/\/\/ CODEX_DETAILS_HELPERS_START([\s\S]*?)\/\/ CODEX_DETAILS_HELPERS_END/);
assert.ok(match, 'the card exposes its pure helper block');
const context = vm.createContext({ Date, Intl });
vm.runInContext(match[1] + '\nglobalThis.helpers = CodexDetails;', context);
const h = context.helpers;
let passed = 0;
function check(name, run) {
  run();
  passed++;
  console.log('PASS ' + name);
}
const now = new Date(2026, 8, 30, 12).getTime();

check('old caches and null history remain unknown', () => {
  for (const details of [undefined, null, {}, { statistics: null }]) {
    const result = h.html(details, now, 'en-US');
    assert.equal(result.history.available, false);
    assert.equal(result.history.total, null);
    assert.equal(result.history.today, null);
    assert.match(result.html, /Today<strong>Not reported/);
    assert.match(result.html, /30-day tokens<strong>Unavailable/);
    assert.doesNotMatch(result.html, /Statistics updated/);
    assert.equal((result.html.match(/data-codex-day=/g) || []).length, 30);
  }
});
check('explicit zero remains zero in every known field', () => {
  const result = h.html({
    credits: { available_count: 0, next_expiry_ms: null },
    statistics: { lifetime_tokens: 0, peak_daily_tokens: 0, longest_running_turn_sec: 0,
      current_streak_days: 0, longest_streak_days: 0,
      daily_usage_buckets: [{ start_date: '2026-09-30', tokens: 0 }] }
  }, now, 'en-US');
  assert.equal(result.history.today, 0);
  assert.equal(result.history.total, 0);
  assert.equal(result.history.count, 1);
  assert.match(result.html, /Unused resets<\/span><strong>0/);
  assert.match(result.html, />0 sec<\/dd>/);
  assert.match(result.html, />0 days<\/dd>/);
  assert.match(result.html, />0 tokens<\/strong>/);
  assert.match(result.html, />0 tokens reported<\/strong>/);
});
check('empty or out-of-window history does not assert zero', () => {
  for (const buckets of [[], [{ start_date: '2026-08-31', tokens: 20 }], [{ start_date: '2026-10-01', tokens: 40 }]]) {
    const result = h.html({ statistics: { daily_usage_buckets: buckets } }, now, 'en-US');
    assert.equal(result.history.available, true);
    assert.equal(result.history.total, null);
    assert.equal(result.history.count, 0);
    assert.match(result.html, /Today<strong>Pending/);
    assert.match(result.html, /30-day tokens<strong>Unavailable/);
  }
});
check('partial sums are reported and duplicate dates use the last value', () => {
  const result = h.html({ statistics: { daily_usage_buckets: [
    { start_date: '2026-09-01', tokens: 50 },
    { start_date: '2026-09-15', tokens: 700 },
    { start_date: '2026-09-15', tokens: 20 },
    { start_date: '2026-10-01', tokens: 1000 }
  ] } }, now, 'en-US');
  assert.equal(result.history.count, 2);
  assert.equal(result.history.total, 70);
  assert.equal(result.history.today, null);
  assert.match(result.html, />70 tokens reported<\/strong>/);
  assert.match(result.html, /2\/30 days reported; missing days are unknown/);
  assert.match(result.html, /Sep 15, 2026: 20 tokens/);
});
check('all thirty observations produce a complete sum', () => {
  const buckets = Array.from(h.calendar(now), day => ({ start_date: day.key, tokens: 10 }));
  const result = h.html({ statistics: { daily_usage_buckets: buckets } }, now, 'en-US');
  assert.equal(result.history.count, 30);
  assert.equal(result.history.total, 300);
  assert.equal(result.history.partial, false);
  assert.match(result.html, /All 30 days reported/);
  assert.doesNotMatch(result.html, /tokens reported<\/strong>/);
});
check('invalid counts and invalid dates are never coerced into known values', () => {
  for (const value of [undefined, null, '', '0', false, -1, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
    assert.equal(h.integer(value), null);
    assert.equal(h.formatCount(value, 'en-US'), 'Not reported');
  }
  for (const date of ['2026-02-29', '2026-09-31', '2026-9-1', '2026-09-01T00:00:00Z', 'bad']) {
    assert.equal(h.parseDay(date), null);
  }
  assert.ok(h.parseDay('2024-02-29'));
  const result = h.history([{ start_date: '2026-09-30', tokens: 40 }, { start_date: '2026-09-30', tokens: null }], now);
  assert.equal(result.today, null);
  assert.equal(result.total, null);
});
check('compact tokens retain exact semantic labels and day counts', () => {
  assert.equal(h.formatTokens(2380000000, 'en-US'), '2.38B');
  assert.equal(h.formatTokens(279000000, 'en-US'), '279.0M');
  const result = h.html({ statistics: { lifetime_tokens: 2380000000, daily_usage_buckets: [{ start_date: '2026-09-30', tokens: 279000000 }] } }, now, 'en-US');
  assert.match(result.html, /aria-label="2,380,000,000 lifetime tokens"/);
  assert.match(result.html, /Sep 30, 2026: 279,000,000 tokens/);
});
check('chat duration has explicit units and respects zero and missing', () => {
  assert.equal(h.duration(undefined), 'Not reported');
  assert.equal(h.duration(0), '0 sec');
  assert.equal(h.duration(61), '1 min 1 sec');
  assert.equal(h.duration(7380), '2 hr 3 min');
  assert.equal(h.duration(90000), '1 d 1 hr');
});
check('labels and auxiliary errors are escaped and bounded', () => {
  const result = h.html({ plan: '<img src=x onerror=alert(1)>', statistics_error: '<script>bad</script>', credits_error: '" & \'' }, now, 'en-US');
  assert.doesNotMatch(result.html, /<img|<script/);
  assert.match(result.html, /&lt;img/);
  assert.match(result.html, /&lt;script/);
  assert.match(result.html, /&quot; &amp; &#39;/);
  assert.equal(Array.from(h.bounded('🧭'.repeat(200), 72)).length, 72);
});
check('past known credit expiry asks for refresh without mutating count', () => {
  const result = h.html({ credits: { available_count: 7, next_expiry_ms: now - 1, fetched_at: now - 1000 }, fetched_at: now }, now, 'en-US');
  assert.match(result.html, /Unused resets<\/span><strong>7/);
  assert.match(result.html, /Reported expiry passed; refresh required/);
  assert.doesNotMatch(result.html, /Statistics updated/);
  const future = h.html({ credits: { available_count: 7, next_expiry_ms: now + 86400000 } }, now, 'en-US');
  assert.match(future.html, /Next expiry: Oct 1/);
});
check('activity uses bounded first-line titles and separate status', () => {
  const label = h.activityLabel('🧭'.repeat(200) + '\nSECRET PROMPT BODY');
  assert.equal(Array.from(label).length, 72);
  assert.doesNotMatch(label, /SECRET/);
  assert.equal(h.activityLabel('Short title\nLong prompt body'), 'Short title …');
  assert.match(source, /\.s-name\{[^}]*-webkit-line-clamp:2/);
  assert.match(source, /\.s-state\{[^}]*min-width:44px[^}]*white-space:nowrap/);
  assert.doesNotMatch(source, /\$\{esc\((?:a\.name|s\.title)\)\}/);
});
check('local calendar covers leap days, midnight and daylight-saving changes', () => {
  const prior = process.env.TZ;
  try {
    process.env.TZ = 'America/New_York';
    const midnight = Date.parse('2026-09-30T03:59:00Z');
    assert.equal(h.calendar(midnight)[29].key, '2026-09-29');
    assert.equal(h.calendar(midnight + 60000)[29].key, '2026-09-30');
    const spring = h.calendar(new Date(2026, 2, 15, 12).getTime());
    assert.equal(spring.length, 30);
    assert.equal(new Set(Array.from(spring, day => day.key)).size, 30);
    assert.equal(spring[0].key, '2026-02-14');
    assert.ok(Array.from(spring, day => day.key).includes('2026-03-08'));
    const fall = h.calendar(new Date(2026, 10, 15, 12).getTime());
    assert.equal(new Set(Array.from(fall, day => day.key)).size, 30);
    process.env.TZ = 'Asia/Kolkata';
    const leap = h.calendar(new Date(2024, 2, 1, 0, 1).getTime());
    assert.equal(leap[0].key, '2024-02-01');
    assert.equal(leap[28].key, '2024-02-29');
    assert.equal(leap[29].key, '2024-03-01');
  } finally {
    if (prior === undefined) delete process.env.TZ;
    else process.env.TZ = prior;
  }
});
check('existing page scripts parse and rich sizing preserves other providers', () => {
  const scripts = [...source.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/g)];
  for (const script of scripts) new vm.Script(script[1]);
  assert.match(source, /const DESIGN_W_UPRIGHT=460, DESIGN_W_FLAT=650;/);
  assert.match(source, /#card\.codex-rich\{max-width:346px\}/);
  assert.match(source, /max-width:246px;margin-left:auto/);
  assert.match(source, /body\[data-edge="top"\] #card\.codex-rich,body\[data-edge="bottom"\] #card\.codex-rich\{width:346px\}/);
  assert.match(source, /overflow-y:auto/);
});
check('four-edge card room honors covered work areas and small viewports', () => {
  for (const height of [650, 360]) {
    for (const edge of ['left', 'right', 'top', 'bottom']) {
      const insets = [12, 0, 40, 0];
      const pill = edge === 'bottom' ? { top: height - 86, bottom: height } : { top: 0, bottom: 86 };
      const room = h.cardSpace(edge, edge === 'left' || edge === 'right' ? 460 : 650, height, insets, pill);
      assert.ok(room.topLimit >= 20, edge + ' clears top inset');
      assert.ok(room.bottomLimit <= height - 48, edge + ' clears bottom inset');
      assert.equal(room.maxHeight, room.bottomLimit - room.topLimit);
      assert.ok(room.maxHeight > 100, edge + ' retains a readable scroll viewport');
      if (edge === 'top') assert.ok(room.topLimit >= pill.bottom + 30);
      if (edge === 'bottom') assert.ok(room.bottomLimit <= pill.top - 30);
    }
  }
  assert.match(source, /const W=innerWidth\/zoom,H=innerHeight\/zoom;/);
  assert.match(source, /card\.style\.maxHeight=room\.maxHeight/);
  assert.match(source, /if\(card\.classList\.contains\('show'\)\)placeCard\(\)/);
});
console.log(`${passed} synthetic Codex detail checks passed. No accounts or installed app accessed.`);
