const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const quota = require('../matra/ui/matra-quota.js');
const { test } = require('node:test');

test('exact boundaries, custom thresholds, unknown and exhausted quota', () => {
  const settings = { watch: .4, critical: .8, mode: 'step' };
  for (const [value, color] of [[0,'green'], [.39999,'green'], [.4,'amber'], [.79999,'amber'],
    [.8,'orange'], [.89999,'orange'], [.9,'red'], [1,'red'], [1.2,'red']]) {
    assert.equal(quota.tone(value, settings), `var(--quota-${color})`);
  }
  for (const value of [null, undefined, NaN, Infinity, -1, '0.5']) assert.equal(quota.tone(value), 'var(--matra-muted)');
  for (const settings of [null, {}, {watch: .8, critical: .5, mode: 'step'},
    {watch: .5, critical: .7, mode: 'accent'}, {watch: .5, critical: .99, mode: 'step'}]) {
    assert.deepEqual(quota.normalize(settings), quota.defaults);
  }
});

test('ramp is continuous between semantic stops, never app accent', () => {
  const settings = { watch: .4, critical: .8, mode: 'ramp' };
  assert.equal(quota.tone(0, settings), 'var(--quota-green)');
  assert.equal(quota.tone(.4, settings), 'var(--quota-amber)');
  assert.equal(quota.tone(.8, settings), 'var(--quota-orange)');
  assert.equal(quota.tone(.9, settings), 'var(--quota-red)');
  for (const value of [.2, .6, .85]) assert.match(quota.tone(value, settings), /color-mix\(in oklab, var\(--quota-\w+\) 50\.000%, var\(--quota-\w+\)\)/);
  assert.doesNotMatch(readFileSync(path.join(__dirname, '../matra/ui/matra-quota.js'), 'utf8'), /var\(--(?:matra-user-)?accent/);
});

function element(dataset = {}) {
  const attrs = new Map();
  return { dataset, handlers: {}, disabled: false, value: '', textContent: '',
    classList: { toggle() {} }, setAttribute: (k,v) => attrs.set(k,v),
    addEventListener(name, fn) { this.handlers[name] = fn; } };
}

test('native persistence, reload, broadcast, reset and failed-save rollback', async () => {
  const ids = Object.fromEntries(['quota-watch','quota-critical','quota-reset','quota-status',
    'quota-watch-value','quota-critical-value'].map(id => [id, element()]));
  const buttons = ['step', 'ramp'].map(mode => element({mode}));
  const events = new Map();
  let stored = {watch: .3, critical: .6, mode: 'ramp'};
  let reject = false;
  const api = { event: { listen(name, fn) { events.set(name, fn); return Promise.resolve(); } },
    core: { async invoke(name, args) {
      if (name === 'get_quota_colors') return stored;
      assert.equal(name, 'set_quota_colors');
      if (reject) throw new Error('disk write failed');
      stored = args.colors;
      return stored;
    } } };
  const boot = async () => {
    vm.runInNewContext(readFileSync(path.join(__dirname, '../matra/ui/matra-quota-settings.js'), 'utf8'), {
      MatraQuota: quota, window: {__TAURI__:api}, document: {getElementById: id => ids[id], querySelectorAll: () => buttons}
    });
    await new Promise(resolve => setImmediate(resolve));
  };
  await boot();
  assert.equal(ids['quota-watch'].value, 30);
  assert.equal(ids['quota-critical'].value, 60);
  ids['quota-watch'].value = 45;
  await ids['quota-watch'].handlers.change();
  assert.equal(stored.watch, .45);
  await boot();
  assert.equal(ids['quota-watch'].value, 45, 'Reopen reads persisted native config');
  reject = true;
  ids['quota-watch'].value = 50;
  await ids['quota-watch'].handlers.change();
  assert.equal(ids['quota-watch'].value, 45, 'Write failure restores actual saved value');
  assert.match(ids['quota-status'].textContent, /previous settings are unchanged/);
  assert(Object.values(ids).every(el => !el.disabled));
  reject = false;
  await ids['quota-reset'].handlers.click();
  assert.deepEqual(JSON.parse(JSON.stringify(stored)), quota.defaults);
  events.get('matra-quota-colors')({payload: {watch: .2, critical: .8, mode:'ramp'}});
  assert.equal(ids['quota-watch'].value, 20);
  assert.equal(ids['quota-critical'].min, 21);
});
