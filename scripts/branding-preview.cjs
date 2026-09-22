// Renders actual Settings HTML with sample data, not the running native app.
const { app, BrowserWindow } = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const root = path.resolve(__dirname, '..');
app.setPath('userData', path.join(root, '.desktop-test', 'brand-preview-profile'));
app.whenReady().then(async () => {
  const out = path.join(root, 'docs', 'screenshots');
  fs.mkdirSync(out, { recursive: true });
  const win = new BrowserWindow({ width: 680, height: 520, useContentSize: true, show: false,
    webPreferences: { preload: path.join(__dirname, 'branding-preview-preload.cjs'),
      contextIsolation: false, nodeIntegration: false, sandbox: false } });
  win.webContents.session.webRequest.onBeforeRequest((details, done) =>
    done({ cancel: /^https?:/.test(details.url) }));
  await win.loadFile(path.join(root, 'native/codenotch/ui/settings.html'));
  for (const [tab, theme] of [['appearance', 'dark'], ['appearance', 'light'], ['accounts', 'dark'], ['general', 'dark']]) {
    await win.webContents.executeJavaScript(`showTab(${JSON.stringify(tab)}); document.querySelector('[data-theme="${theme}"]').click();`);
    await new Promise(resolve => setTimeout(resolve, 250));
    const shot = await win.webContents.capturePage();
    fs.writeFileSync(path.join(out, `settings-${tab}-${theme}.png`), shot.toPNG());
    console.log(tab, theme, await win.webContents.executeJavaScript('document.querySelector("#side").innerText'));
  }
  app.quit();
}).catch(error => { console.error(error); app.exit(1); });
