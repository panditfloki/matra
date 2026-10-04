'use strict';
const vscode = require('vscode');
const assert = require('node:assert/strict');
const fs = require('node:fs');

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
  const report = { passed: true, vscodeVersion: vscode.version, appName: vscode.env.appName, extensionVersion: extension.packageJSON.version, localExtensionHost: extension.extensionKind === vscode.ExtensionKind.UI, commands: 5, resetPreferencePersistence: true, dashboardOpened: true, sidebarOpened: true, generatedAt: new Date().toISOString() };
  assert.ok(report.localExtensionHost);
  if (process.env.MATRA_HOST_REPORT) fs.writeFileSync(process.env.MATRA_HOST_REPORT, JSON.stringify(report, null, 2));
  console.log('MATRA_HOST_ACCEPTANCE', JSON.stringify(report));
}
module.exports = { run };
