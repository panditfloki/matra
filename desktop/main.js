'use strict';
const { app, BrowserWindow, ipcMain, screen, Tray, Menu, nativeImage, utilityProcess } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const VERSION = require('./version.json').version;
app.setName('Matra Desktop');
const smoke = process.argv.includes('--smoke-test');
if (smoke) app.setPath('userData', path.join(__dirname, '..', '.desktop-test', 'profile'));
if (!smoke && !app.requestSingleInstanceLock()) app.quit();
else {
  let widget, dashboard, tray, worker, preferences, prefsFile, expanded = false, pinned = false;
  let state = { version: VERSION, loading: true, model: null, backend: 'Connecting', error: null };
  function publish() {
    state.preferences = preferences; state.expanded = expanded; state.pinned = pinned;
    if (widget && !widget.isDestroyed()) widget.webContents.send('state', state);
  }
  function save() { fs.writeFileSync(prefsFile, JSON.stringify(preferences, null, 2)); publish(); }
  function place() {
    if (!widget || widget.isDestroyed()) return;
    const display = screen.getAllDisplays().find(d => d.id === preferences.display) || screen.getPrimaryDisplay();
    const a = display.workArea;
    const horizontal = ['top', 'bottom'].includes(preferences.edge);
    const width = expanded ? 420 : horizontal ? 318 : 90;
    const height = expanded ? Math.min(700, a.height - 10) : horizontal ? 90 : 330;
    let x = a.x + Math.round((a.width - width) / 2), y = a.y + Math.round((a.height - height) / 2);
    if (preferences.edge === 'right') x = a.x + a.width - width;
    if (preferences.edge === 'left') x = a.x;
    if (preferences.edge === 'top') y = a.y;
    if (preferences.edge === 'bottom') y = a.y + a.height - height;
    widget.setBounds({ x, y, width, height }); publish();
  }
  function openDashboard() {
    if (dashboard && !dashboard.isDestroyed()) { dashboard.show(); dashboard.focus(); return; }
    dashboard = new BrowserWindow({ title: 'Mātrā | Dashboard', width: 1200, height: 820,
      backgroundColor: '#111214', autoHideMenuBar: true,
      webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, nodeIntegration: false, sandbox: true } });
    protect(dashboard); dashboard.loadFile(path.join(__dirname, '..', 'media', 'dashboard.html'));
  }
  function protect(win) {
    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    win.webContents.on('will-navigate', event => event.preventDefault());
  }
  function startWorker() {
    worker = utilityProcess.fork(path.join(__dirname, 'worker.js'), [], { serviceName: 'Matra providers' });
    worker.on('message', message => {
      if (message.type === 'snapshot') {
        state = { ...state, ...message, loading: false, error: null };
        if (dashboard && !dashboard.isDestroyed()) dashboard.webContents.send('dashboard-data', message.data);
      } else { state.loading = false; state.error = message.error; }
      publish();
    });
    worker.on('exit', code => { if (!app.isQuitting) { state.error = `Provider engine stopped (${code}). Restart from the tray.`; publish(); } });
  }
  app.whenReady().then(async () => {
    prefsFile = path.join(app.getPath('userData'), 'desktop-settings.json');
    preferences = { edge: 'right', display: screen.getPrimaryDisplay().id, startAtLogin: false };
    try { preferences = { ...preferences, ...JSON.parse(fs.readFileSync(prefsFile, 'utf8')) }; } catch {}
    if (!['left', 'right', 'top', 'bottom'].includes(preferences.edge)) preferences.edge = 'right';
    widget = new BrowserWindow({ width: 90, height: 330, show: false, frame: false, transparent: true,
      resizable: false, maximizable: false, minimizable: false, skipTaskbar: true, alwaysOnTop: true,
      backgroundColor: '#00000000', hasShadow: false,
      webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, nodeIntegration: false, sandbox: true } });
    protect(widget);
    await widget.loadFile(path.join(__dirname, 'index.html'));
    if (!fs.existsSync(prefsFile)) { expanded = true; pinned = true; }
    place(); if (!smoke) widget.showInactive();
    const pixels = Buffer.alloc(32 * 32 * 4);
    for (let y = 0; y < 32; y++) for (let x = 0; x < 32; x++) {
      const distance = Math.hypot(x - 15.5, y - 15.5), i = (y * 32 + x) * 4;
      const ring = distance > 10 && distance < 14;
      pixels[i] = ring ? 191 : 20; pixels[i + 1] = ring ? 239 : 25;
      pixels[i + 2] = ring ? 180 : 17; pixels[i + 3] = distance < 15 ? 255 : 0;
    }
    tray = new Tray(nativeImage.createFromBitmap(pixels, { width: 32, height: 32 }));
    tray.setToolTip(`Mātrā ${VERSION}`);
    tray.setContextMenu(Menu.buildFromTemplate([
      { label: 'Show Mātrā', click: () => { expanded = true; pinned = true; place(); widget.show(); } },
      { label: 'Open dashboard', click: openDashboard },
      { label: 'Refresh', click: () => worker.postMessage({ type: 'refresh', force: true }) },
      { label: 'Restart provider engine', click: () => { worker.removeAllListeners('exit'); worker.kill(); startWorker(); } },
      { type: 'separator' }, { label: 'Quit Mātrā', click: () => app.quit() },
    ]));
    tray.on('double-click', openDashboard);
    screen.on('display-metrics-changed', place); screen.on('display-removed', place);
    startWorker();
    if (smoke) {
      expanded = true; place();
      const deadline = Date.now() + 90000;
      while (state.loading && Date.now() < deadline) await new Promise(r => setTimeout(r, 250));
      await new Promise(r => setTimeout(r, 1000));
      const out = process.env.MATRA_DESKTOP_TEST_OUT || path.join(__dirname, '..', '.desktop-test'); fs.mkdirSync(out, { recursive: true });
      fs.writeFileSync(path.join(out, 'widget.png'), (await widget.webContents.capturePage()).toPNG());
      const checks = [];
      for (const id of ['codex', 'gemini']) {
        await widget.webContents.executeJavaScript(`document.querySelectorAll('#tabs button')[${id === 'codex' ? 1 : 2}].click()`);
        checks.push({ provider: id, content: await widget.webContents.executeJavaScript("document.getElementById('content').innerText") });
      }
      await widget.webContents.executeJavaScript("document.getElementById('dashboard').click()");
      while (!dashboard || dashboard.webContents.isLoading()) await new Promise(r => setTimeout(r, 100));
      await new Promise(r => setTimeout(r, 1500));
      const dashboardStatus = await dashboard.webContents.executeJavaScript("document.getElementById('status').textContent");
      fs.writeFileSync(path.join(out, 'dashboard.png'), (await dashboard.webContents.capturePage()).toPNG());
      const placements = [];
      for (const edge of ['left', 'right', 'top', 'bottom']) {
        preferences.edge = edge; expanded = false; place();
        placements.push({ edge, bounds: widget.getBounds() });
      }
      expanded = true; place();
      fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify({ version: VERSION, loading: state.loading,
        error: state.error, backend: state.backend, model: state.model, checks, dashboardStatus, placements,
        ui: await widget.webContents.executeJavaScript('document.body.innerText') }, null, 2));
      app.quit();
    }
  });
  ipcMain.handle('state', () => state);
  ipcMain.handle('action', (_event, action, value) => {
    if (action === 'expand') { expanded = true; place(); }
    if (action === 'collapse' && !pinned) { expanded = false; place(); }
    if (action === 'pin') { pinned = !pinned; publish(); }
    if (action === 'dashboard') openDashboard();
    if (action === 'refresh') { state.loading = true; publish(); worker?.postMessage({ type: 'refresh', force: true }); }
    if (action === 'edge' && ['left', 'right', 'top', 'bottom'].includes(value)) { preferences.edge = value; save(); place(); }
    if (action === 'display') { const all = screen.getAllDisplays(); const index = all.findIndex(d => d.id === preferences.display); preferences.display = all[(index + 1) % all.length].id; save(); place(); }
    if (action === 'startup' && app.isPackaged) { preferences.startAtLogin = !!value; app.setLoginItemSettings({ openAtLogin: !!value }); save(); }
    return { packaged: app.isPackaged };
  });
  ipcMain.on('dashboard-message', (_event, message) => {
    if (message === 'ready' && state.data) dashboard?.webContents.send('dashboard-data', state.data);
    if (['refresh', 'refreshClaude', 'refreshCodex', 'refreshGemini', 'refreshAll'].includes(message)) worker?.postMessage({ type: 'refresh', force: true });
  });
  app.on('second-instance', () => { expanded = true; place(); widget?.show(); });
  app.on('window-all-closed', () => {});
  app.on('before-quit', () => { app.isQuitting = true; worker?.kill(); });
}
