'use strict';
// Run filesystem scans and provider subprocesses away from the desktop UI thread.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
// Packaged helper tools take precedence over inherited Explorer/terminal PATH.
process.env.PATH = [path.join(__dirname, '..', 'tools'), process.env.PATH || ''].join(path.delimiter);
if (process.platform === 'win32') {
  try { process.env.TEMP = process.env.TMP = fs.realpathSync.native(os.tmpdir()); } catch {}
}
const { desktopModel } = require('../shared/desktop-model');
let adapters, pending = false;
function send(message) { process.parentPort.postMessage(message); }
function localAdapters() {
  return adapters ||= { claude: require('../quota'), codex: require('../codex'),
    gemini: require('../gemini'), parser: require('../parser'), fx: require('../fx') };
}
async function update(force = false) {
  if (pending) return;
  pending = true;
  try {
    let data, backend;
    // Reuse an existing Mātrā server's refresh ownership when one is available.
    try {
      if (process.env.MATRA_DESKTOP_STANDALONE === '1') throw new Error('Standalone validation');
      const res = await fetch(`http://127.0.0.1:4317/api/${force ? 'refresh' : 'usage'}`, {
        method: force ? 'POST' : 'GET', signal: AbortSignal.timeout(20000) });
      if (!res.ok) throw new Error('Server unavailable');
      data = await res.json();
      if (!data || !('codex' in data) || !('gemini' in data)) throw new Error('Not a Mātrā response');
      backend = 'Shared local server';
    } catch {
      const a = localAdapters();
      const [quota] = await Promise.all([
        a.claude.quota({ force }).catch(() => null),
        a.codex.refresh({ force }).catch(() => null),
        a.gemini.refresh({ force }).catch(() => null),
      ]);
      data = a.parser.snapshot();
      data.quota = quota;
      data.codex = a.codex.read(); data.gemini = a.gemini.read();
      data.fx = a.fx.read(a.gemini.planConfig().forexMarkupPercent);
      backend = 'Desktop provider engine';
    }
    send({ type: 'snapshot', data, model: desktopModel(data), backend });
  } catch (error) { send({ type: 'error', error: String(error.message).slice(0, 240) }); }
  finally { pending = false; }
}
process.parentPort.on('message', ({ data }) => { if (data.type === 'refresh') update(data.force); });
update();
setInterval(() => update(), 60000);
