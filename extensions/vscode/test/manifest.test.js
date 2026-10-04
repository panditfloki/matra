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
