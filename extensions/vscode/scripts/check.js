'use strict';
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const root = path.resolve(__dirname, '..');
function scan(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) scan(file);
    else if (entry.name.endsWith('.js')) execFileSync(process.execPath, ['--check', file], { stdio: 'inherit' });
  }
}
scan(path.join(root, 'src')); scan(path.join(root, 'media'));
const manifest = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
if (manifest.extensionKind.join() !== 'ui') throw new Error('Must run in the local extension host');
if (manifest.dependencies && Object.keys(manifest.dependencies).length) throw new Error('Unexpected runtime dependency');
console.log('Syntax, local extension host and dependency checks passed.');
