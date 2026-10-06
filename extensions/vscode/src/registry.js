'use strict';
// One list of every provider the extension knows, mirroring the notch app's
// provider set. The UI reads names, legends and monograms from here, so adding
// a provider is one entry plus its module — no hardcoded four anywhere else.
//
// `matra.providers` empty (the default) means "auto": show every tool that is
// installed on this machine. A non-empty list is an explicit choice and wins.

const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');

const exists = file => fs.access(file).then(() => true, () => false);
const any = files => Promise.all(files.map(exists)).then(found => found.some(Boolean));
const appSupport = home => process.platform === 'darwin' ? path.join(home, 'Library', 'Application Support')
  : process.platform === 'win32' ? process.env.APPDATA || path.join(home, 'AppData', 'Roaming')
    : process.env.XDG_CONFIG_HOME || path.join(home, '.config');

// Detection for the four original providers, whose modules predate `detect`.
const BUILT_IN_DETECT = {
  claude: (home, paths) => any([paths?.claude || path.join(home, '.claude')]),
  codex: (home, paths) => any([paths?.codex || path.join(home, '.codex')]),
  cursor: home => any([path.join(appSupport(home), 'Cursor'), path.join(home, '.cursor')]),
  antigravity: home => any([path.join(home, '.antigravity'), path.join(home, '.gemini', 'antigravity'),
    '/Applications/Antigravity.app', '/Applications/Antigravity IDE.app']),
};

const PROVIDERS = [
  { id: 'claude', name: 'Claude Code', short: 'Claude', legend: 'C', monogram: 'C', help: 'https://code.claude.com/docs/en/setup' },
  { id: 'codex', name: 'Codex', short: 'Codex', legend: 'X', monogram: 'O', help: 'https://developers.openai.com/codex/cli/' },
  { id: 'cursor', name: 'Cursor', short: 'Cursor', legend: 'CU', monogram: '↗', help: 'https://cursor.com/dashboard' },
  { id: 'antigravity', name: 'Antigravity', short: 'Antigravity', legend: 'AG', monogram: 'A', help: 'https://antigravity.google/' },
  { id: 'gemini', name: 'Gemini CLI', short: 'Gemini', legend: 'GE', monogram: 'G', help: 'https://github.com/google-gemini/gemini-cli' },
  { id: 'copilot', name: 'GitHub Copilot', short: 'Copilot', legend: 'GH', monogram: 'GH', help: 'https://github.com/settings/copilot' },
  { id: 'grok', name: 'Grok', short: 'Grok', legend: 'GR', monogram: 'X', help: 'https://grok.com/?_s=usage' },
  { id: 'kimi', name: 'Kimi Code', short: 'Kimi', legend: 'KI', monogram: 'K', help: 'https://www.kimi.com/code/console' },
  { id: 'opencode', name: 'OpenCode', short: 'OpenCode', legend: 'OC', monogram: 'OC', help: 'https://opencode.ai' },
  { id: 'kiro', name: 'Kiro', short: 'Kiro', legend: 'KR', monogram: 'KR', help: 'https://app.kiro.dev/account/usage' },
  { id: 'commandcode', name: 'Command Code', short: 'Command', legend: 'CC', monogram: '⌘', help: 'https://commandcode.ai' },
  { id: 'amp', name: 'Amp', short: 'Amp', legend: 'AM', monogram: 'A', help: 'https://ampcode.com/settings' },
  { id: 'devin', name: 'Devin', short: 'Devin', legend: 'DV', monogram: 'D', help: 'https://app.devin.ai' },
];

// A provider whose module is missing (not ported yet) is simply not offered.
function load(id) {
  try { return require(`./providers/${id}`); } catch (error) {
    if (error?.code === 'MODULE_NOT_FOUND' && String(error.message).includes(`providers/${id}`)) return null;
    throw error;
  }
}

const AVAILABLE = PROVIDERS.filter(p => load(p.id));
const IDS = AVAILABLE.map(p => p.id);
const BY_ID = Object.fromEntries(PROVIDERS.map(p => [p.id, p]));
const NAMES = Object.fromEntries(PROVIDERS.map(p => [p.id, p.name]));
const meta = id => BY_ID[id] || { id, name: id, short: id, legend: String(id).slice(0, 2).toUpperCase(), monogram: 'M', help: null };

async function detect(id, { homeDir, paths } = {}) {
  const home = homeDir || os.homedir();
  try {
    if (BUILT_IN_DETECT[id]) return await BUILT_IN_DETECT[id](home, paths);
    const mod = load(id);
    return typeof mod?.detect === 'function' ? Boolean(await mod.detect({ homeDir: home, paths })) : false;
  } catch { return false; }
}

// Explicit list → those ids (known ones only), in registry order.
// Empty / missing → every provider detected as installed.
async function resolveEnabled(configured, options = {}) {
  const list = Array.isArray(configured) ? configured.filter(id => typeof id === 'string') : [];
  if (list.length) return IDS.filter(id => list.includes(id));
  const found = await Promise.all(IDS.map(id => detect(id, options)));
  return IDS.filter((_, i) => found[i]);
}

module.exports = { PROVIDERS, IDS, NAMES, meta, load, detect, resolveEnabled };
