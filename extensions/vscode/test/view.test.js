'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { resetTime, render, statusText } = require('../media/view');
const now = Date.parse('2026-10-05T10:00:00Z');
test('countdown days, hours, minutes and expired reset are honest', () => {
  assert.equal(resetTime(now + (5 * 24 + 3) * 3600000, 'remaining', now), '5 days 3h left');
  assert.equal(resetTime(now + 61 * 60000, 'remaining', now), '1h 1m left');
  assert.equal(resetTime(now + 1000, 'remaining', now), '1m left');
  assert.equal(resetTime(now - 1, 'remaining', now), 'Resetting…');
  for (const value of [null, undefined, NaN, Infinity, '2026-01-01']) assert.equal(resetTime(value, 'date', now), 'Reset time unavailable');
});
test('date includes date and clock in local zone, including across years', () => {
  const result = resetTime(now + 3600000, 'date', now, 'en-IN', 'Asia/Kolkata');
  assert.match(result, /5 Oct/); assert.match(result, /4:30/);
  assert.match(resetTime(Date.parse('2027-01-01T00:01:00Z'), 'date', now, 'en-IN', 'Asia/Kolkata'), /2027/);
});
test('untrusted provider strings are escaped; absent readings are not zero', () => {
  const html = render({ providers: [{ id: 'claude', name: '<script>alert(1)</script>', status: 'ready', windows: [{ id: 'session', label: '<img onerror=x>', usedPercent: null, resetsAt: null }], sessions: [{ label: '<iframe>', detail: '<x>', lastActiveAt: now }] }] }, { now });
  assert.ok(!html.includes('<script>')); assert.ok(!html.includes('<img ')); assert.ok(!html.includes('<iframe>'));
  assert.match(html, /&lt;script&gt;/); assert.match(html, /No reading/); assert.ok(!html.includes('0% used'));
});
test('over-limit actual value is preserved and meter is bounded', () => {
  const snapshot = { providers: [{ id: 'claude', name: 'Claude', status: 'stale', windows: [{ label: 'Session', usedPercent: 120, resetsAt: now + 1000 }] }] };
  assert.match(render(snapshot, { now }), /120% used/); assert.match(render(snapshot, { now }), /aria-valuenow="100"/);
  assert.equal(statusText(snapshot), 'Claude 120%~');
});
test('empty selected-provider state has a working setup action', () => {
  assert.match(render({ providers: [] }), /data-action="providers"/);
});
