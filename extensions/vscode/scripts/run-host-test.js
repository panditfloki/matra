'use strict';
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const os = require('node:os');
const [binary, output, ...extraArgs] = process.argv.slice(2);
if (!binary || !output) throw new Error('Usage: node scripts/run-host-test.js <editor-binary> <artifact-directory>');
const root = path.resolve(__dirname, '..');
const artifacts = path.resolve(output);
// macOS Unix socket paths are limited to 103 bytes; project-local profiles can exceed it.
const profile = fs.mkdtempSync(path.join(process.platform === 'darwin' ? '/tmp' : os.tmpdir(), 'matra-host-'));
fs.mkdirSync(artifacts, { recursive: true });
fs.mkdirSync(path.join(profile, 'User'), { recursive: true });
fs.writeFileSync(path.join(profile, 'User', 'settings.json'), JSON.stringify({ 'matra.providers': [], 'security.workspace.trust.enabled': false, 'workbench.startupEditor': 'none', 'telemetry.telemetryLevel': 'off', 'update.mode': 'none', 'extensions.autoUpdate': false }));
const env = { ...process.env, MATRA_HOST_REPORT: path.join(artifacts, 'report.json') };
delete env.ELECTRON_RUN_AS_NODE;
const logfile = fs.openSync(path.join(artifacts, 'host.log'), 'w');
const child = spawn(binary, ['--user-data-dir', profile, '--extensions-dir', path.join(artifacts, 'extensions'), '--extensionDevelopmentPath', root, '--extensionTestsPath', path.join(root, 'test', 'host'), '--disable-extensions', '--skip-welcome', '--skip-release-notes', '--disable-workspace-trust', '--new-window', ...extraArgs], { env, stdio: ['ignore', logfile, logfile] });
const timeout = setTimeout(() => { child.kill('SIGTERM'); console.error('Extension host timed out'); process.exitCode = 1; }, 150000);
child.on('error', error => { clearTimeout(timeout); console.error(error.message); process.exitCode = 1; });
child.on('exit', code => {
  clearTimeout(timeout); fs.closeSync(logfile);
  const report = path.join(artifacts, 'report.json');
  if (code !== 0 || !fs.existsSync(report)) { console.error(`Extension host failed (${code}); see ${path.join(artifacts, 'host.log')}`); process.exitCode = 1; }
  else console.log(fs.readFileSync(report, 'utf8'));
});
