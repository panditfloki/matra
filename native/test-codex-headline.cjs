// Run with node --test test-codex-headline.cjs from windows/.
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { test } = require('node:test');

const html = readFileSync(join(__dirname, 'matra/ui/notch.html'), 'utf8');
const scripts = [...html.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/g)];
for (const [, source] of scripts) new vm.Script(source);
function markedSource(document, name) {
  const begin = `// BEGIN TESTABLE ${name} SELECTOR`;
  const end = `// END TESTABLE ${name} SELECTOR`;
  assert.equal(document.split(begin).length, 2, `exactly one ${name} begin marker`);
  assert.equal(document.split(end).length, 2, `exactly one ${name} end marker`);
  const start = document.indexOf(begin) + begin.length;
  const stop = document.indexOf(end);
  assert.ok(stop > start, `${name} markers are ordered`);
  return document.slice(start, stop);
}
const context = vm.createContext({});
vm.runInContext(markedSource(html, 'HEADLINE'), context);
vm.runInContext(markedSource(html, 'WEEKLY'), context);
const pick = (windows, provider = 'codex') => context.headlineOf({ windows }, provider)?.id ?? null;

test('Codex headline selects core primary regardless of extra-window order', () => {
  assert.equal(pick([{ id: 'spark' }, { id: 'secondary' }, { id: 'primary' }]), 'primary');
});
test('Missing Codex primary stays blank, never replaced by weekly or extra quotas', () => {
  assert.equal(pick([{ id: 'spark' }, { id: 'secondary' }]), null);
  assert.equal(pick([{ id: 'secondary' }]), null);
  assert.equal(pick([{ id: 'spark' }, { id: 'code-review' }]), null);
  assert.equal(pick([]), null);
});
test('Codex secondary remains available to the separate weekly ring', () => {
  const windows = [{ id: 'spark' }, { id: 'secondary', used: 0.8 }];
  assert.equal(context.weeklyOf({ windows }, 'codex').id, 'secondary');
  assert.equal(context.weeklyOf({ windows: [{ id: 'primary' }] }, 'codex'), null);
});
test('Marker extraction does not depend on neighbouring function order', () => {
  const block = `// BEGIN TESTABLE HEADLINE SELECTOR\n${markedSource(html, 'HEADLINE')}\n// END TESTABLE HEADLINE SELECTOR`;
  const reordered = `function staleOf() {}\n${block}\nfunction unrelated() {}`;
  const isolated = vm.createContext({});
  vm.runInContext(markedSource(reordered, 'HEADLINE'), isolated);
  assert.equal(isolated.headlineOf({ windows: [{ id: 'secondary' }] }, 'codex'), null);
});
test('Other provider headline selection is unchanged', () => {
  assert.equal(pick([{ id: 'weekly_all' }, { id: 'session' }], 'claude'), 'session');
  assert.equal(pick([{ id: 'on_demand' }, { id: 'included' }], 'cursor'), 'included');
});

const dashboard = require('./matra/ui/dashboard.js');
for (const [surface, selector] of [
  ['notch', (windows, provider) => context.headlineOf({ windows }, provider)],
  ['dashboard', (windows, provider) => dashboard.headlineOf({ windows }, provider)],
]) {
  test(`${surface}: Claude uses the all-models week only without a session`, () => {
    for (const id of ['seven_day', 'weekly_all', 'weekly']) {
      const week = { id, used: 0.89 };
      assert.equal(selector([week], 'claude').id, id);
      assert.equal(Math.round(100 * (1 - selector([week], 'claude').used)), 11);
      for (const sessionID of ['session', 'five_hour']) {
        assert.equal(selector([week, { id: sessionID, used: 0 }], 'claude').id, sessionID);
      }
      assert.equal(selector([week], 'claude').id, id, 'window changes have no sticky state');
    }
  });
  test(`${surface}: a model-specific or missing limit cannot stand for Claude`, () => {
    for (const windows of [[], [{ id: 'weekly_scoped' }], [{ id: 'seven_day_opus' }], [{ id: 'weekly_opus' }]]) {
      assert.equal(selector(windows, 'claude') ?? null, null);
    }
  });
}
