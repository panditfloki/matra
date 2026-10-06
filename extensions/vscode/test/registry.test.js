'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { PROVIDERS, IDS, resolveEnabled, meta } = require('../src/registry');
const { statusLabel } = require('../src/statusbar');
const manifest = require('../package.json');

const home = () => fs.mkdtempSync(path.join(os.tmpdir(), 'matra-registry-'));

test('every offered provider has a module, a name, a legend and setup help', () => {
  for (const id of IDS) {
    const p = meta(id);
    assert.ok(p.name && p.legend && p.monogram && p.help, id);
    assert.equal(typeof require(`../src/providers/${id}`).createProvider, 'function', id);
  }
  assert.deepEqual(IDS.slice(0, 4), ['claude', 'codex', 'cursor', 'antigravity']);
});

test('the providers setting accepts every registry id and defaults to auto', () => {
  const setting = manifest.contributes.configuration.properties['matra.providers'];
  assert.deepEqual(setting.default, []);
  for (const p of PROVIDERS) assert.ok(setting.items.enum.includes(p.id), p.id);
});

test('auto mode shows only tools installed on this machine', async () => {
  const dir = home();
  assert.deepEqual(await resolveEnabled([], { homeDir: dir }), IDS.filter(id => id === 'antigravity' && fs.existsSync('/Applications/Antigravity IDE.app')));
  fs.mkdirSync(path.join(dir, '.claude')); fs.mkdirSync(path.join(dir, '.codex'));
  const found = await resolveEnabled([], { homeDir: dir });
  assert.ok(found.includes('claude') && found.includes('codex'));
  assert.ok(!found.includes('cursor'));
});

test('an explicit list wins over detection and ignores unknown ids', async () => {
  const dir = home();
  assert.deepEqual(await resolveEnabled(['codex', 'nonsense', 'claude'], { homeDir: dir }), ['claude', 'codex']);
});

test('a crowded status bar keeps providers with readings, and pins win', () => {
  const p = (id, used) => ({ id, name: id, status: 'ready', headlineId: 'w', windows: used === null ? [] : [{ id: 'w', label: 'w', usedPercent: used }] });
  const snapshot = { providers: [p('claude', 10), p('codex', null), p('cursor', 50), p('antigravity', null), p('grok', 20)] };
  const text = statusLabel(snapshot, { bars: false });
  assert.match(text, /C 90%/); assert.match(text, /CU 50%/); assert.match(text, /GR 80%/);
  assert.doesNotMatch(text, /X —/);
  assert.equal(statusLabel(snapshot, { bars: false, pinned: ['codex'] }), 'X —');
});
