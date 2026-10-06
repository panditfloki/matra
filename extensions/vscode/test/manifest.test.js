'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const manifest = require('../package.json');
test('provider paths remain local and cannot travel through Settings Sync', () => {
  for (const key of ['matra.paths.claude', 'matra.paths.codex']) {
    const setting = manifest.contributes.configuration.properties[key];
    assert.equal(setting.scope, 'application');
    assert.equal(setting.ignoreSync, true);
  }
  assert.deepEqual(manifest.extensionKind, ['ui']);
  assert.equal(manifest.capabilities.untrustedWorkspaces.supported, false);
});

test('the provider picker tells the truth about which ids actually read', () => {
  // The catalogue is deliberately wider than the implementation: PROVIDERS
  // lists every tool worth supporting, IDS is the subset with a reader. The
  // picker offers all of them, so each one must say which it is, or a user
  // selects "grok", every reading vanishes, and nothing explains why.
  const { IDS, PROVIDERS } = require('../src/registry');
  const items = manifest.contributes.configuration.properties['matra.providers'].items;

  // enumDescriptions aligns with enum BY INDEX. Adding an id without a
  // description shifts every later label onto the wrong provider, which reads
  // as a confident, wrong answer rather than a missing one.
  assert.equal(items.enum.length, items.enumDescriptions.length,
    'every enum entry needs its own description, or the labels shift');
  assert.deepEqual(items.enum, PROVIDERS.map(p => p.id),
    'the picker must offer exactly the catalogue, in catalogue order');

  for (const [i, id] of items.enum.entries()) {
    const text = items.enumDescriptions[i];
    const implemented = IDS.includes(id);
    assert.equal(/^Planned\./.test(text), !implemented,
      `${id} is ${implemented ? 'implemented' : 'not implemented'} but its picker label says "${text}"`);
  }
});
