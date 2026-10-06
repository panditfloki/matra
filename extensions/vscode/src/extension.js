'use strict';
const vscode = require('vscode');
const path = require('node:path');
const { randomBytes } = require('node:crypto');
const { UsageStore } = require('./store');
const { IDS, NAMES, meta, load, detect, resolveEnabled } = require('./registry');
const { escape } = require('../media/view');
const { statusLabel, accessibleLabel, severity, tooltipMarkdown } = require('./statusbar');

let current;

function activate(context) {
  const views = new Set();
  let panel, store, interval, disposed = false, refreshing = false;
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 50);
  status.name = 'Mātrā AI usage'; status.command = 'matra.open';
  const config = () => vscode.workspace.getConfiguration('matra');
  const mode = () => config().get('resetDisplay') === 'date' ? 'date' : 'remaining';
  const configured = () => { const v = config().get('providers', []); return Array.isArray(v) ? v : []; };
  let active = [];
  const snapshot = () => store?.snapshot || { schemaVersion: 1, generatedAt: Date.now(), providers: [] };
  function publish() {
    if (disposed) return;
    const data = snapshot();
    const level = severity(data);
    status.text = `${level === 'normal' ? '$(pulse)' : '$(flame)'} ${statusLabel(data, { bars: config().get('statusBarBars', true), pinned: config().get('statusBarProviders', []) })}`;
    // The hover card needs all three: theme icons for $(pulse)/$(dashboard),
    // HTML for the coloured quota bars (a MarkdownString cannot draw one any
    // other way), and trust for the Dashboard/Refresh command links.
    //
    // ⚠️ This was deliberately untrusted and HTML-free. It is safe to enable
    // ONLY because every provider-sourced string in tooltipMarkdown() goes
    // through md(), which escapes < > [ ] ( ) and |. The test
    // "provider text stays escaped, which is what lets the card enable HTML at
    // all" guards exactly that, and goes red if the escaper is weakened.
    // If that test ever fails, turn these flags off before shipping.
    const card = new vscode.MarkdownString(
      tooltipMarkdown(data, { mode: mode(), intervalSeconds: Number(config().get('refreshInterval', 60)) || 60 }),
      true,
    );
    card.supportHtml = true;
    card.isTrusted = { enabledCommands: ['matra.open', 'matra.refresh'] };
    status.tooltip = card;
    status.accessibilityInformation = { label: accessibleLabel(data), role: 'button' };
    status.backgroundColor = level === 'error' ? new vscode.ThemeColor('statusBarItem.errorBackground') : level === 'warning' ? new vscode.ThemeColor('statusBarItem.warningBackground') : undefined;
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
  const paths = () => {
    const out = {};
    for (const id of ['claude', 'codex']) {
      const value = config().get(`paths.${id}`, '');
      if (typeof value === 'string' && path.isAbsolute(value)) out[id] = value;
    }
    return out;
  };
  let generation = 0;
  async function start() {
    const mine = ++generation;
    const ids = await resolveEnabled(configured(), { paths: paths() });
    if (mine !== generation || disposed) return;
    store?.dispose(); clearInterval(interval);
    active = ids;
    const providers = ids.map(id => load(id).createProvider({ storagePath: context.globalStorageUri.fsPath, paths: paths() }));
    store = new UsageStore(providers);
    store.subscribe(publish);
    publish(); void refresh();
    const seconds = Math.max(60, Math.min(900, Number(config().get('refreshInterval', 60)) || 60));
    interval = setInterval(() => { if (vscode.window.state.focused) { void redetect(); void refresh(); } }, seconds * 1000);
  }
  // Auto mode only: a tool installed (or removed) while the editor is open
  // shows up on the next background read, without a reload.
  async function redetect() {
    if (configured().length || disposed) return;
    const ids = await resolveEnabled([], { paths: paths() });
    if (ids.join() !== active.join()) await start();
  }
  async function chooseProviders() {
    const installed = await Promise.all(IDS.map(id => detect(id, { paths: paths() })));
    const auto = { label: '$(sparkle) Every installed tool (auto)', description: 'Default. New tools appear when you install them.', auto: true, picked: !configured().length };
    const items = IDS.map((id, i) => ({ label: NAMES[id], id, description: installed[i] ? 'installed' : 'not found on this machine', picked: configured().length ? active.includes(id) : false }));
    const picked = await vscode.window.showQuickPick([auto, { label: '', kind: vscode.QuickPickItemKind.Separator }, ...items], { canPickMany: true, title: 'Mātrā providers', placeHolder: 'Auto shows everything installed, or pick specific tools' });
    if (!picked) return;
    const ids = picked.filter(item => item.id).map(item => item.id);
    await config().update('providers', picked.some(item => item.auto) || !ids.length ? [] : ids, vscode.ConfigurationTarget.Global);
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
      case 'providerHelp': if (IDS.includes(message.provider) && meta(message.provider).help) await vscode.env.openExternal(vscode.Uri.parse(meta(message.provider).help)); break;
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
      if (e.affectsConfiguration('matra.providers') || e.affectsConfiguration('matra.paths') || e.affectsConfiguration('matra.refreshInterval')) void start();
      else if (e.affectsConfiguration('matra')) publish();
    }),
    vscode.window.onDidChangeWindowState(s => { if (s.focused) void refresh(); })
  );
  const cleanup = { dispose() { if (disposed) return; disposed = true; clearInterval(interval); store?.dispose(); panel?.dispose(); views.clear(); status.dispose(); } };
  context.subscriptions.push(cleanup); current = cleanup;
  void start();
  return { version: 1, getSnapshot: snapshot, refresh: () => refresh(true) };
}
function deactivate() { current?.dispose(); current = undefined; }
module.exports = { activate, deactivate };
