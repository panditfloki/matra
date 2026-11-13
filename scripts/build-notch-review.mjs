// Package the real Windows notch renderer with an inert sample bridge.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ui = path.join(root, 'native/matra/ui');
const out = path.resolve(process.argv[2] || path.join(root, 'artifacts/mac-first-notch'));
const allowed = path.resolve(root, '..');
if (out !== allowed && !out.startsWith(allowed + path.sep)) throw new Error('Review output must stay inside the development task.');
fs.mkdirSync(out, { recursive: true });
if (!fs.realpathSync(out).startsWith(fs.realpathSync(allowed) + path.sep)) throw new Error('Review output resolves outside development task.');

const readAsset = name => {
  const file = path.resolve(ui, name);
  if (!file.startsWith(ui + path.sep)) throw new Error('Unexpected asset path: ' + name);
  if (process.argv.includes('--baseline')) return execFileSync('git', ['show', '121746dcf4a0e619fa05ddd9b81f9b2e64f00da1:native/matra/ui/' + name], { cwd: root, encoding: 'utf8' });
  return fs.readFileSync(file, 'utf8');
};
const policy = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; font-src data:; connect-src 'none'; frame-src 'self' about:";
let component = readAsset('notch.html');
component = component.replace(/<script([^>]*?)\bsrc="([^"]+)"([^>]*)><\/script>/g, (_, before, name, after) => {
  // Inline deferred theme code at DOMContentLoaded, preserving its original timing.
  let script = readAsset(name);
  if (/\bdefer\b/.test(before + after)) script = `window.addEventListener('DOMContentLoaded',()=>{\n${script}\n});`;
  return `<script>${script.replace(/<\/script/gi, '<\\/script')}</script>`;
}).replace(/<link\s+rel="stylesheet"\s+href="([^"]+)"\s*>/g, (_, name) => `<style>${readAsset(name)}</style>`);
const bridge = fs.readFileSync(path.join(root, 'scripts/fixtures/mac-first-notch-bridge.js'), 'utf8');
const glyphs = Object.fromEntries(['claude', 'codex', 'cursor', 'gemini', 'grok'].map(id => {
  const svg = fs.readFileSync(path.join(root, 'native/matra/glyphs', id + '.svg'), 'utf8');
  if (/<script|\bon\w+\s*=/i.test(svg)) throw new Error('Unexpected executable glyph content');
  return [id, { kind: 'svg', svg, url: '' }];
}));
const fixturePrelude = `window.__matraSampleGlyphs=${JSON.stringify(glyphs).replace(/</g, '\\u003c')};\n${bridge}`;
component = component.replace('<head>', `<head><meta http-equiv="Content-Security-Policy" content="${policy}"><script>${fixturePrelude}</script>`);
const componentPath = path.join(out, 'windows-notch-component.html');
fs.writeFileSync(componentPath, component);

const shell = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><meta http-equiv="Content-Security-Policy" content="${policy}"><title>Mātrā · Windows notch development 0.2.0</title><style>
*{box-sizing:border-box}body{margin:0;background:#111316;color:#f4f4f5;font:14px system-ui,sans-serif}main{max-width:1100px;margin:auto;padding:28px}h1{font-size:22px;margin:0 0 8px}p{color:#b9bec7;line-height:1.5}label{display:inline-flex;align-items:center;gap:8px;margin:8px 16px 8px 0}select,input{font:inherit}select{padding:6px;border:1px solid #565c66;border-radius:5px;background:#20252d;color:#fff}.stage{margin-top:18px;padding:18px;background:linear-gradient(145deg,#34465c,#677880);border-radius:12px;overflow:auto}iframe{display:block;width:650px;height:650px;margin:auto;border:1px solid #aab4c244;background:transparent}small{color:#aeb7c4}</style></head><body><main>
<h1>Mātrā · Windows notch · 0.2.0 development</h1>
<p>Browser component proof using sample readings. Reach the screen-edge pill, then hover a provider to inspect its card. The starting layout follows the observed Mac settings: left edge, six sources, Small size and Liquid Glass.</p>
<label>Edge <select id="edge"><option>right</option><option selected>left</option><option>top</option><option>bottom</option></select></label>
<label>Providers <select id="count"><option>1</option><option>2</option><option>3</option><option>4</option><option>5</option><option selected>6</option><option>10</option></select></label>
<label>Size <select id="size"><option value="0.8" selected>Small</option><option value="1">Medium</option><option value="1.25">Large</option></select></label>
<label>Theme <select id="theme"><option value="glass" selected>Liquid Glass</option><option value="darkGlass">Dark Glass</option><option value="dark">Solid Dark</option><option value="light">Light</option><option value="system">System</option></select></label>
<label><input id="fold" type="checkbox" checked>Fold when away</label><label><input id="readings" type="checkbox" checked>Percentage readings</label><label><input id="stale" type="checkbox">Stale reading</label>
<div class="stage"><iframe id="component" title="Windows notch component"></iframe></div>
<p><small>This is the Windows component, with browser glass and sample accounts. Native materials, pointer carry and hover behavior still need Windows testing and comparison with the Mac notch. Your installed app, accounts and settings are unchanged.</small></p>
<script>const frame=document.getElementById('component');function configure(){const edge=document.getElementById('edge').value;const size=Number(document.getElementById('size').value);frame.style.width=['left','right'].includes(edge)?'460px':'650px';frame.style.zoom=String(size);frame.contentWindow.__matraSample.configure({edge,size,count:Number(document.getElementById('count').value),theme:document.getElementById('theme').value,fold:document.getElementById('fold').checked,readings:document.getElementById('readings').checked,stale:document.getElementById('stale').checked});}frame.addEventListener('load',configure);for(const control of document.querySelectorAll('select,input'))control.addEventListener('change',configure);frame.srcdoc=${JSON.stringify(component).replace(/</g, '\\u003c')};</script>
</main></body></html>`;
fs.writeFileSync(path.join(out, 'index.html'), shell);
console.log(JSON.stringify({ review: path.join(out, 'index.html'), component: componentPath, nativeApp: false, sampleData: true }));
