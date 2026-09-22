'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { provider } = require('../shared/desktop-model');
test('quota directions become remaining without turning zero into missing', () => {
  const q = provider('claude', {}, { limits: [{ percent: 100 }, { percent: 0, remaining: true }] });
  assert.equal(q.quota.remaining, 0); assert.equal(q.quota.status, 'ready');
});
test('expired evidence cannot become live capacity', () => {
  const q = provider('codex', {}, { limits: [{ percent: 22, resetsAt: 100 }] }, 200);
  assert.equal(q.quota.remaining, null); assert.equal(q.quota.limits[0].expired, true);
});
test('quota failure does not erase valid history', () => {
  const q = provider('gemini', { available: true, totals: { tokens: 0, cost: 0 }, sessions: [] }, null, 200, 'win32');
  assert.equal(q.history.status, 'ready'); assert.equal(q.history.tokens, 0);
  assert.equal(q.quota.status, 'unavailable'); assert.match(q.quota.reason, /Windows/);
});
test('stale quota remains labelled even alongside fresh history', () => {
  const q = provider('claude', { totals: { tokens: 12 } }, { stale: true, limits: [{ percent: 25 }] });
  assert.equal(q.quota.status, 'stale'); assert.equal(q.quota.remaining, 75);
});
