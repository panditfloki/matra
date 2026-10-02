const { readFileSync } = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');

const source = readFileSync(path.join(__dirname, '../matra/ui/matra-theme.js'), 'utf8');
// Exercise the production colour selector, including the exact transition edges.
const notch = readFileSync(path.join(__dirname, '../matra/ui/notch.html'), 'utf8');
const colourSource = notch.slice(notch.indexOf('const AMPLE='), notch.indexOf('const metered='));
const tone = vm.runInNewContext(`${colourSource}\ntone;`, { MatraQuota: require('../matra/ui/matra-quota.js') });
for (const [value, token] of [
  [NaN, 'matra-muted'], [Infinity, 'matra-muted'], [-1, 'matra-muted'],
  [0, 'quota-green'], [0.4999, 'quota-green'], [0.50, 'quota-amber'],
  [0.6999, 'quota-amber'], [0.70, 'quota-orange'], [0.8999, 'quota-orange'],
  [0.90, 'quota-red'], [1, 'quota-red'], [1.4, 'quota-red']
]) assert.equal(tone(value), `var(--${token})`);
const css = readFileSync(path.join(__dirname, '../matra/ui/matra-theme.css'), 'utf8');
const quotaDeclarations = [...css.matchAll(/--quota-(?:green|amber|orange|red)\s*:\s*([^;}]+)/g)];
assert.equal(quotaDeclarations.length, 8, 'Four semantic colours in both light and dark');
for (const [, colour] of quotaDeclarations) {
  assert.doesNotMatch(colour, /accent/i, 'Quota tokens must never inherit an app or OS accent');
}
const themes = ['glass', 'darkGlass', 'dark', 'light', 'system'];
const accents = ['system', 'ff33e1', 'eb4236', 'eb8436', 'ffd400', '00ff88',
  '00e5cc', '36a8eb', '6c5ce7', 'b026ff', 'f7f6f5'];
function element(dataset = {}) {
  const classes = new Set();
  const attributes = new Map();
  return {
    dataset, disabled: false, textContent: '', handlers: {},
    classList: { toggle(name, on) { if (on) classes.add(name); else classes.delete(name); },
      contains(name) { return classes.has(name); } },
    setAttribute(name, value) { attributes.set(name, value); },
    getAttribute(name) { return attributes.get(name) ?? null; },
    addEventListener(name, handler) { this.handlers[name] = handler; }
  };
}
async function harness({ dark = true, saved = 'darkGlass' } = {}) {
  const themeButtons = themes.map(theme => element({ theme }));
  const accentButtons = accents.map(accent => element({ accent }));
  const ids = Object.fromEntries(['sw-auto-install', 'sw-notch-motion', 'auto-install-status',
    'matra-theme-status', 'matra-accent-status'].map(id => [id, element()]));
  const properties = new Map();
  const root = { dataset: {}, style: { setProperty(key, value) { properties.set(key, value); } } };
  const stored = new Map([['matra-theme', saved]]);
  const listeners = new Map();
  const requests = [];
  const reject = new Set();
  const values = { get_matra_theme: saved, get_matra_accent: 'system',
    get_matra_system_accent: 'eb4236', get_automatic_updates: false, get_notch_motion: true };
  const media = { matches: dark, addEventListener(_, fn) { this.change = fn; } };
  const api = {
    event: { listen(name, fn) { listeners.set(name, fn); return Promise.resolve(() => {}); } },
    core: { async invoke(name, args) {
      requests.push({ name, args });
      if (reject.has(name)) throw new Error('Test persistence failure');
      if (name === 'set_matra_theme') return values.get_matra_theme = args.theme;
      if (name === 'set_matra_accent') return values.get_matra_accent = args.accent;
      if (name === 'set_automatic_updates') return values.get_automatic_updates = args.on;
      if (name === 'set_notch_motion') return values.get_notch_motion = args.on;
      return values[name];
    } }
  };
  vm.runInNewContext(source, {
    window: { __TAURI__: api }, console, matchMedia: () => media,
    localStorage: { getItem: key => stored.get(key), setItem: (key, value) => stored.set(key, value) },
    document: { documentElement: root, getElementById: id => ids[id] ?? null,
      querySelectorAll: selector => selector === '#matra-theme button' ? themeButtons : accentButtons }
  });
  await new Promise(resolve => setImmediate(resolve));
  return { root, ids, properties, stored, media, listeners, reject, requests, values,
    async theme(theme) { await themeButtons.find(b => b.dataset.theme === theme).handlers.click(); },
    async accent(accent) { await accentButtons.find(b => b.dataset.accent === accent).handlers.click(); },
    buttons: [...themeButtons, ...accentButtons] };
}

(async () => {
  const h = await harness();
  assert.equal(h.root.dataset.matraTheme, 'dark');
  assert.equal(h.root.dataset.matraMaterial, 'glass');
  assert.equal(h.root.dataset.matraGlass, 'darkGlass');
  assert.equal(h.properties.get('--matra-user-accent'), '#eb4236');
  await h.theme('light');
  assert.equal(h.root.dataset.matraTheme, 'light');
  await h.theme('system');
  assert.equal(h.root.dataset.matraTheme, 'dark', 'Light -> System must use the dark OS');
  assert.equal(h.stored.get('matra-theme'), 'system', 'Persist choice, not resolved dark/light');
  h.media.matches = false; h.media.change();
  assert.equal(h.root.dataset.matraTheme, 'light', 'Follow OS changes while open');
  h.media.matches = true; h.media.change();
  assert.equal(h.root.dataset.matraTheme, 'dark');
  await h.theme('light');
  h.media.change();
  assert.equal(h.root.dataset.matraTheme, 'light', 'Explicit Light must ignore OS changes');
  for (const theme of ['glass', 'darkGlass']) {
    await h.theme(theme);
    assert.equal(h.root.dataset.matraTheme, 'dark');
    assert.equal(h.root.dataset.matraMaterial, 'glass');
    assert.equal(h.root.dataset.matraGlass, theme);
  }
  h.reject.add('set_matra_theme');
  await h.theme('system');
  assert.equal(h.stored.get('matra-theme'), 'darkGlass');
  assert.match(h.ids['matra-theme-status'].textContent, /could not be saved/);
  assert(h.buttons.every(b => !b.disabled));
  h.listeners.get('matra-theme')({ payload: 'system' });
  assert.equal(h.root.dataset.matraTheme, 'dark', 'Other window theme broadcast is applied');
  h.listeners.get('matra-accent')({ payload: 'ffd400' });
  assert.equal(h.properties.get('--matra-user-accent'), '#ffd400');
  assert.equal(h.properties.get('--matra-user-ink'), '#14110f', 'Light accents need dark text');
  assert(![...h.properties.keys()].some(key => key.startsWith('--quota-')), 'Accent bridge cannot overwrite usage tokens');
  h.reject.add('set_matra_accent');
  await h.accent('6c5ce7');
  assert.equal(h.properties.get('--matra-user-accent'), '#ffd400');
  assert.match(h.ids['matra-accent-status'].textContent, /previous colour is unchanged/);
  const toggle = h.ids['sw-auto-install'];
  assert.equal(toggle.getAttribute('aria-checked'), 'false');
  await toggle.handlers.click();
  assert.equal(toggle.getAttribute('aria-checked'), 'true');
  assert.equal(h.values.get_automatic_updates, true);
  assert.match(h.ids['auto-install-status'].textContent, /restart/);
  h.reject.add('set_automatic_updates');
  await toggle.handlers.click();
  assert.equal(toggle.getAttribute('aria-checked'), 'true');
  assert.equal(toggle.disabled, false);
  assert.match(h.ids['auto-install-status'].textContent, /previous choice is unchanged/);
  h.reject.delete('set_automatic_updates');
  await toggle.handlers.click();
  assert.equal(toggle.getAttribute('aria-checked'), 'false');
  h.listeners.get('matra-automatic-updates')({ payload: true });
  assert.equal(toggle.getAttribute('aria-checked'), 'true');
  const lightSystem = await harness({ dark: false, saved: 'system' });
  assert.equal(lightSystem.root.dataset.matraTheme, 'light');
  console.log('PASS: theme transitions, OS following, accent broadcasts, auto-update toggle and truthful save failures');
})().catch(error => { console.error(error); process.exitCode = 1; });
