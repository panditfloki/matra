'use strict';
// An explicit allowlist prevents private local files entering desktop packages.
const fs = require('node:fs');
const path = require('node:path');
const root = path.join(__dirname, '..');
const version = require('../desktop/version.json').version;
const target = path.join(root, 'dist', `Matra-${version}-${process.platform}-${process.arch}`);
if (fs.existsSync(target)) throw new Error(`Build directory already exists: ${target}. Use a new version or move the old build aside.`);
const runtime = path.dirname(require('electron'));
fs.cpSync(runtime, target, { recursive: true });
const appDir = path.join(target, 'resources', 'app'); fs.mkdirSync(appDir, { recursive: true });
const files = ['desktop', 'shared', 'media', 'parser.js', 'quota.js', 'codex.js', 'gemini.js', 'fx.js', 'refresh-sync.js', 'statusbar.js', 'LICENSE', 'README.md'];
for (const file of files) fs.cpSync(path.join(root, file), path.join(appDir, file), { recursive: true });
fs.writeFileSync(path.join(appDir, 'package.json'), JSON.stringify({ name: 'matra-desktop', productName: 'Mātrā', version, main: 'desktop/main.js', author: 'Pandit Floki', license: 'MIT' }, null, 2));
if (process.platform === 'win32') {
  const toolsDir = path.join(appDir, 'tools'); fs.mkdirSync(toolsDir);
  const ccusage = path.join(process.env.APPDATA, 'npm', 'node_modules', 'ccusage', 'node_modules', '@ccusage', `ccusage-win32-${process.arch}`, 'bin', 'ccusage.exe');
  const packages = path.join(process.env.LOCALAPPDATA, 'Microsoft', 'WinGet', 'Packages');
  const sqliteDir = fs.readdirSync(packages).find(name => name.startsWith('SQLite.SQLite_'));
  if (!sqliteDir || !fs.existsSync(ccusage)) throw new Error('Install ccusage and official SQLite before building this Windows package.');
  fs.copyFileSync(ccusage, path.join(toolsDir, 'ccusage.exe'));
  fs.copyFileSync(path.join(packages, sqliteDir, 'sqlite3.exe'), path.join(toolsDir, 'sqlite3.exe'));
  fs.writeFileSync(path.join(toolsDir, 'NOTICE.txt'), 'Helper tools bundled from this build machine:\nccusage 20.0.19: MIT, https://github.com/ccusage/ccusage\nSQLite 3.53.4: public domain, https://sqlite.org/copyright.html\nThis preview is a local build. Audit third-party notices before distribution.\n');
}
if (process.platform === 'win32') fs.renameSync(path.join(target, 'electron.exe'), path.join(target, 'Matra.exe'));
console.log(target);
