// Local development: debugger port 9337. Disposable CI: discover the fixture's
// independent WebView2 browser environments, each using an ephemeral port.
const fs = require('node:fs');
const path = require('node:path');
const {listNativeTargets} = require('./native-cdp-targets.cjs');
(async () => {
  const targets = await listNativeTargets({page:process.argv[2] || 'notch'});
  const target = targets.find(t => t.url.endsWith(`/${process.argv[2] || 'notch'}.html`));
  if (!target) throw new Error('Native webview not found');
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { ws.close(); reject(new Error('CDP connection timeout')); }, 10000);
    ws.onopen = () => { clearTimeout(timer); resolve(); };
    ws.onerror = error => { clearTimeout(timer); reject(error); };
  });
  let seq = 0; const pending = new Map();
  ws.onmessage = e => { const m = JSON.parse(e.data); if (pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } };
  const send = (method, params) => new Promise((resolve, reject) => {
    const id = ++seq;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('CDP timeout')); }, 15000);
    pending.set(id, m => { clearTimeout(timer); m.error ? reject(new Error(JSON.stringify(m.error))) : resolve(m.result); });
    ws.send(JSON.stringify({id, method, params}));
  });
  try {
    const result = await send('Runtime.evaluate', { expression: process.argv[3] || 'document.title', awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    console.log(JSON.stringify(result.result, null, 2));
    if (process.argv[4]) {
      const shot = await send('Page.captureScreenshot', {format:'png'});
      const dir = path.resolve(__dirname, '../.desktop-test'); fs.mkdirSync(dir, {recursive:true});
      fs.writeFileSync(path.join(dir, path.basename(process.argv[4])), Buffer.from(shot.data,'base64'));
    }
  } finally { ws.close(); }
})().catch(e => { console.error(e); process.exitCode = 1; });
