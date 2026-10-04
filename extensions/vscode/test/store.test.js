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
