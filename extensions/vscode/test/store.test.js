'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { UsageStore } = require('../src/store');
test('one failed provider does not hide other providers or expose its error', async () => {
  const store = new UsageStore([{ id: 'claude', read: async () => { throw new Error('secret-token'); } }, { id: 'codex', read: async () => ({ id: 'codex', windows: [], status: 'ready' }) }]);
  const data = await store.refresh();
  assert.equal(data.providers[1].status, 'ready'); assert.equal(data.providers[0].status, 'error');
  assert.ok(!JSON.stringify(data).includes('secret-token')); store.dispose();
});
test('simultaneous refreshes share one read and disposal prevents late updates', async () => {
  let resolve, reads = 0, updates = 0;
  const store = new UsageStore([{ id: 'claude', read: () => { reads++; return new Promise(r => { resolve = r; }); } }]);
  store.subscribe(() => updates++);
  const a = store.refresh(), b = store.refresh(); await Promise.resolve();
  assert.equal(a, b); assert.equal(reads, 1); store.dispose();
  resolve({ id: 'claude', windows: [] }); await a; assert.equal(updates, 0);
});
test('hung provider is bounded', async () => {
  const store = new UsageStore([{ id: 'cursor', read: () => new Promise(() => {}) }], { timeoutMs: 10 });
  assert.equal((await store.refresh()).providers[0].status, 'error'); store.dispose();
});
test('a cached window past its reset is stale even when its reader still reports ready', async () => {
  const store = new UsageStore([{ id: 'cursor', read: async () => ({ id: 'cursor', status: 'ready', updatedAt: 800, windows: [{ usedPercent: 99, resetsAt: 900 }] }) }], { now: () => 1000 });
  const record = (await store.refresh()).providers[0];
  assert.equal(record.status, 'stale'); assert.equal(record.updatedAt, 800);
  assert.equal(record.windows[0].usedPercent, 99); store.dispose();
});
