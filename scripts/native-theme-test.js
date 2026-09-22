// Integration check against the running native WebView2 app, never a mocked bridge.
const {execFileSync} = require('node:child_process');
const assert = require('node:assert/strict');
const path = require('node:path');
const inspect = path.join(__dirname, 'native-inspect.js');
function evaluate(page, expression, screenshot) {
  return JSON.parse(execFileSync(process.execPath, [inspect, page, expression, ...(screenshot ? [screenshot] : [])], {encoding:'utf8'})).value;
}
(async () => {
  const original = evaluate('notch', "window.__TAURI__.core.invoke('get_matra_theme')");
  evaluate('notch', "window.__TAURI__.core.invoke('open_settings')");
  let ready = false;
  for (let i=0; i<30; i++) {
    const targets = await (await fetch('http://127.0.0.1:9337/json/list')).json();
    if (targets.some(t=>t.url.endsWith('/settings.html'))) { ready=true; break; }
    await new Promise(r=>setTimeout(r,200));
  }
  assert.ok(ready, 'native Settings must open');
  const results = [];
  try {
    const author = evaluate('settings', "document.querySelector('#tab-general').click();({name:document.querySelector('#author').textContent,url:document.querySelector('#author').title,brand:document.querySelector('.matra-brand').textContent,choices:[...document.querySelectorAll('#matra-theme button')].map(b=>b.textContent)})", 'native-author.png');
    assert.equal(author.name, 'Pandit Floki');
    assert.equal(author.url, 'https://github.com/panditfloki');
    assert.equal(author.brand, 'dy/dx.f(Mātrā)');
    assert.deepEqual(author.choices, ['Light','Dark','Glass','System']);
    for (const theme of ['light','dark','glass','system']) {
      evaluate('settings', `document.querySelector('#tab-appearance').click();document.querySelector('[data-theme=${theme}]').click();new Promise(r=>setTimeout(()=>r(document.documentElement.dataset.matraTheme),400))`, `theme-${theme}-settings.png`);
      const value = evaluate('notch', `unfold();new Promise(r=>setTimeout(async()=>r({theme:document.documentElement.dataset.matraTheme,system:matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light',material:document.documentElement.dataset.matraMaterial,saved:await window.__TAURI__.core.invoke('get_matra_theme'),bg:getComputedStyle(pill).backgroundColor,text:getComputedStyle(document.querySelector('.pct')).color,ring:getComputedStyle(document.querySelector('svg.ring circle')).fill,edge:document.body.dataset.edge,filter:getComputedStyle(pill).backdropFilter,corner:getComputedStyle(pill,'::before').display,brackets:[...document.querySelectorAll('.h-rest')].map(x=>getComputedStyle(x).display)}),300))`, `theme-${theme}-notch.png`);
      assert.equal(value.theme, theme==='glass' ? 'light' : theme==='system' ? value.system : theme, 'Glass stays light, System follows OS, explicit themes stay selected');
      assert.equal(value.saved, theme, 'native config persisted choice');
      // Reference colours from live dydxfx.com stylesheet inspected on 2026-09-22.
      assert.equal(value.text, value.theme==='light' ? 'rgb(20, 17, 15)' : 'rgb(248, 244, 237)');
      assert.equal(value.ring, theme==='glass' ? 'oklch(0.84 0.014 65)' : value.theme==='light' ? 'rgb(244, 241, 237)' : 'rgb(26, 23, 20)');
      assert.equal(value.material, theme==='glass' ? 'glass' : 'solid');
      if(theme==='glass') assert.equal(value.bg,'oklch(0.76 0.014 65 / 0.84)','Glass is smoked grey, not the old white tint');
      else assert.match(value.bg,/^rgb\(/,'solid themes remain opaque');
      assert.equal(value.corner, 'none');
      assert.deepEqual(value.brackets, ['none','none']);
      results.push(value);
    }
    const folded = evaluate('notch', "setFolded(true);({folded:document.body.classList.contains('folded'),width:document.querySelector('#rest').offsetWidth,height:document.querySelector('#rest').offsetHeight})");
    assert.ok(folded.folded);
    assert.equal(Math.min(folded.width, folded.height), 5, 'recessed idle lip');
    console.log(JSON.stringify({passed:true,author,results,folded},null,2));
  } finally {
    evaluate('notch', `window.__TAURI__.core.invoke('set_matra_theme',{theme:${JSON.stringify(original)}})`);
    evaluate('settings', "document.querySelector('#close').click()");
  }
})().catch(e=>{console.error(e);process.exitCode=1;});
