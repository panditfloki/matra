'use strict';
const vscode = require('vscode');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

async function run() {
  const cfg = vscode.workspace.getConfiguration('matra');
  await cfg.update('providers', [], vscode.ConfigurationTarget.Global);
  const extension = vscode.extensions.getExtension('dydxfx.matra');
  assert.ok(extension, 'Mātrā extension registered');
  const api = await extension.activate();
  assert.equal(api.version, 1);
  await api.refresh();
  assert.equal(api.getSnapshot().providers.length, 0);
  const commands = await vscode.commands.getCommands(true);
  for (const name of ['matra.open', 'matra.refresh', 'matra.resetMode', 'matra.providers', 'matra.settings']) assert.ok(commands.includes(name), `${name} registered`);
  await vscode.commands.executeCommand('matra.open');
  await vscode.commands.executeCommand('workbench.view.extension.matra');
  for (const mode of ['date', 'remaining']) {
    await cfg.update('resetDisplay', mode, vscode.ConfigurationTarget.Global);
    assert.equal(vscode.workspace.getConfiguration('matra').get('resetDisplay'), mode);
  }
  await vscode.commands.executeCommand('matra.refresh');
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'matra-host-codex-'));
  try {
    const now = Date.now();
    fs.mkdirSync(path.join(fixture, 'sessions'), { recursive: true });
    fs.writeFileSync(path.join(fixture, 'auth.json'), JSON.stringify({ tokens: { account_id: 'fixture-account', access_token: 'fixture-only-not-a-real-token' } }));
    fs.utimesSync(path.join(fixture, 'auth.json'), new Date(now - 3600000), new Date(now - 3600000));
    fs.writeFileSync(path.join(fixture, 'sessions', 'fixture.jsonl'), [
      { type: 'session_meta', timestamp: new Date(now - 60000).toISOString(), payload: { id: 'fixture', cwd: '/sample/Example', account_id: 'fixture-account' } },
      { type: 'event_msg', timestamp: new Date(now - 1000).toISOString(), payload: { type: 'token_count', rate_limits: { primary: { used_percent: 12, window_minutes: 300, resets_at: (now + 3600000) / 1000 } }, info: { total_token_usage: { input_tokens: 100, output_tokens: 20, cached_input_tokens: 30, total_tokens: 120 } } } }
    ].map(row => JSON.stringify(row)).join('\n') + '\n');
    await cfg.update('paths.codex', fixture, vscode.ConfigurationTarget.Global);
    await cfg.update('providers', ['codex'], vscode.ConfigurationTarget.Global);
    for (let i = 0; i < 100 && api.getSnapshot().providers[0]?.id !== 'codex'; i++) await new Promise(resolve => setTimeout(resolve, 20));
    await api.refresh();
    const record = api.getSnapshot().providers[0];
    assert.equal(record.status, 'ready');
    assert.equal(record.windows[0].usedPercent, 12);
    assert.equal(record.usage.totalTokens, 120);
    await cfg.update('providers', [], vscode.ConfigurationTarget.Global);
    await cfg.update('paths.codex', '', vscode.ConfigurationTarget.Global);
  } finally { fs.rmSync(fixture, { recursive: true, force: true }); }
  const report = { passed: true, vscodeVersion: vscode.version, appName: vscode.env.appName, extensionVersion: extension.packageJSON.version, localExtensionHost: extension.extensionKind === vscode.ExtensionKind.UI, commands: 5, resetPreferencePersistence: true, dashboardOpened: true, sidebarOpened: true, offlineProviderIntegration: true, generatedAt: new Date().toISOString() };
  assert.ok(report.localExtensionHost);
  if (process.env.MATRA_HOST_REPORT) fs.writeFileSync(process.env.MATRA_HOST_REPORT, JSON.stringify(report, null, 2));
  console.log('MATRA_HOST_ACCEPTANCE', JSON.stringify(report));
}
module.exports = { run };
