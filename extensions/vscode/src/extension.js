'use strict';
const vscode = require('vscode');
const path = require('node:path');
const { randomBytes } = require('node:crypto');
const { UsageStore, IDS, NAMES } = require('./store');
const { escape, resetTime, statusText, headline, finite } = require('../media/view');

const HELP = {
  claude: 'https://code.claude.com/docs/en/setup',
  codex: 'https://developers.openai.com/codex/cli/',
  cursor: 'https://cursor.com/dashboard',
  antigravity: 'https://antigravity.google/'
};
let current;

function activate(context) {
  const views = new Set();
  let panel, store, interval, disposed = false, refreshing = false;
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 50);
  status.name = 'Mātrā AI usage'; status.command = 'matra.open';
  const config = () => vscode.workspace.getConfiguration('matra');
  const mode = () => config().get('resetDisplay') === 'date' ? 'date' : 'remaining';
  const enabled = () => {
    const values = config().get('providers', IDS);
    return IDS.filter(id => Array.isArray(values) && values.includes(id));
  };
  const snapshot = () => store?.snapshot || { schemaVersion: 1, generatedAt: Date.now(), providers: [] };
  function publish() {
    if (disposed) return;
    const data = snapshot();
    status.text = `$(pulse) ${statusText(data)}`;
    const tooltip = new vscode.MarkdownString();
    tooltip.appendText('Mātrā · standalone usage\n\n');
    for (const p of data.providers) {
      tooltip.appendText(`${p.name}${p.account?.plan ? ` (${p.account.plan})` : ''}\n`);
      for (const w of p.windows) tooltip.appendText(`${w.label}: ${finite(w.usedPercent) ? `${Math.round(w.usedPercent * 10) / 10}% used` : 'unavailable'} · ${resetTime(w.resetsAt, mode())}\n`);
      if (p.status !== 'ready') tooltip.appendText(`${p.status}: ${p.message || 'No current reading'}\n`);
      tooltip.appendText('\n');
    }
    tooltip.appendText('Click to open your dashboard. ~ means a stale reading.');
    status.tooltip = tooltip;
    status.accessibilityInformation = { label: statusText(data), role: 'button' };
    const max = Math.max(0, ...data.providers.map(p => headline(p)?.usedPercent).filter(finite));
    status.backgroundColor = max >= 95 ? new vscode.ThemeColor('statusBarItem.errorBackground') : max >= 80 ? new vscode.ThemeColor('statusBarItem.warningBackground') : undefined;
    config().get('statusBar', true) ? status.show() : status.hide();
    for (const webview of views) void webview.postMessage({ type: 'snapshot', snapshot: data, mode: mode(), refreshing });
  }
  async function refresh(force = false) {
    const readingStore = store;
    if (!readingStore || disposed) return;
    refreshing = true; publish();
    try { await readingStore.refresh({ force }); }
    finally { if (readingStore === store) { refreshing = false; publish(); } }
  }
  function start() {
    store?.dispose(); clearInterval(interval);
    const paths = {};
    for (const id of ['claude', 'codex']) {
      const value = config().get(`paths.${id}`, '');
      if (typeof value === 'string' && path.isAbsolute(value)) paths[id] = value;
    }
    const providers = enabled().map(id => require(`./providers/${id}`).createProvider({ storagePath: context.globalStorageUri.fsPath, paths }));
    store = new UsageStore(providers);
    store.subscribe(publish);
    publish(); void refresh();
    const seconds = Math.max(60, Math.min(900, Number(config().get('refreshInterval', 60)) || 60));
    interval = setInterval(() => { if (vscode.window.state.focused) void refresh(); }, seconds * 1000);
  }
  async function chooseProviders() {
    const picked = await vscode.window.showQuickPick(IDS.map(id => ({ label: NAMES[id], id, picked: enabled().includes(id) })), { canPickMany: true, title: 'Mātrā providers', placeHolder: 'Choose the tools whose usage you want to see' });
    if (picked) await config().update('providers', picked.map(item => item.id), vscode.ConfigurationTarget.Global);
  }
  async function setResetMode(value) {
    if (value !== 'date' && value !== 'remaining') return;
    await config().update('resetDisplay', value, vscode.ConfigurationTarget.Global);
  }
  async function chooseMode() {
    const picked = await vscode.window.showQuickPick([{ label: 'Date & time', description: 'The actual reset date, in your local time zone', mode: 'date' }, { label: 'Time left', description: 'Days, hours and minutes until reset', mode: 'remaining' }], { title: 'Mātrā reset display' });
    if (picked) await setResetMode(picked.mode);
  }
  const settings = () => vscode.commands.executeCommand('workbench.action.openSettings', '@ext:dydxfx.matra');
  async function receive(message) {
    if (!message || typeof message !== 'object' || disposed) return;
    switch (message.type) {
      case 'ready': publish(); break;
      case 'refresh': await refresh(true); break;
      case 'setResetMode': await setResetMode(message.mode); break;
      case 'providers': await chooseProviders(); break;
      case 'settings': await settings(); break;
      case 'providerHelp': if (IDS.includes(message.provider)) await vscode.env.openExternal(vscode.Uri.parse(HELP[message.provider])); break;
    }
  }
  function attach(webview, owner) {
    const media = vscode.Uri.joinPath(context.extensionUri, 'media');
    webview.options = { enableScripts: true, localResourceRoots: [media] };
    const nonce = randomBytes(24).toString('base64');
    const resource = name => escape(webview.asWebviewUri(vscode.Uri.joinPath(media, name)).toString());
    webview.html = `<!doctype html><html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${escape(webview.cspSource)}; script-src 'nonce-${nonce}'; img-src ${escape(webview.cspSource)}; font-src ${escape(webview.cspSource)};"><title>Mātrā</title><link rel="stylesheet" href="${resource('style.css')}"></head><body><div id="root"><p>Reading local usage…</p></div><script nonce="${nonce}" src="${resource('view.js')}"></script><script nonce="${nonce}" src="${resource('client.js')}"></script></body></html>`;
    views.add(webview);
    const message = webview.onDidReceiveMessage(m => { void receive(m).catch(() => vscode.window.showErrorMessage('Mātrā could not complete that action. Try again.')); });
    const removed = owner.onDidDispose(() => { views.delete(webview); message.dispose(); removed.dispose(); });
  }
  function open() {
    if (panel) { panel.reveal(); return; }
    panel = vscode.window.createWebviewPanel('matra.dashboard', 'Mātrā Usage', vscode.ViewColumn.Active, { enableScripts: true, retainContextWhenHidden: false });
    attach(panel.webview, panel);
    panel.onDidDispose(() => { panel = undefined; });
  }
  context.subscriptions.push(status,
    vscode.window.registerWebviewViewProvider('matra.usage', { resolveWebviewView(view) { attach(view.webview, view); } }),
    vscode.commands.registerCommand('matra.open', open),
    vscode.commands.registerCommand('matra.refresh', () => refresh(true)),
    vscode.commands.registerCommand('matra.resetMode', chooseMode),
    vscode.commands.registerCommand('matra.providers', chooseProviders),
    vscode.commands.registerCommand('matra.settings', settings),
    vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration('matra.providers') || e.affectsConfiguration('matra.paths') || e.affectsConfiguration('matra.refreshInterval')) start();
      else if (e.affectsConfiguration('matra')) publish();
    }),
    vscode.window.onDidChangeWindowState(s => { if (s.focused) void refresh(); })
  );
  const cleanup = { dispose() { if (disposed) return; disposed = true; clearInterval(interval); store?.dispose(); panel?.dispose(); views.clear(); status.dispose(); } };
  context.subscriptions.push(cleanup); current = cleanup;
  start();
  return { version: 1, getSnapshot: snapshot, refresh: () => refresh(true) };
}
function deactivate() { current?.dispose(); current = undefined; }
module.exports = { activate, deactivate };
