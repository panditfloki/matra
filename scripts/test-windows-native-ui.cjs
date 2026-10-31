/* Disposable Windows runner only. Actual packaged WebView2 + Tauri IPC. */
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const path = require('node:path');
const {testNativeHandles} = require('./native-handle-test.js');
if (process.env.GITHUB_ACTIONS !== 'true' || process.platform !== 'win32') throw new Error('Disposable Windows CI only');
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
function evaluate(page, expression, shot) {
  return JSON.parse(execFileSync(process.execPath, [path.join(__dirname, 'native-inspect.js'), page, expression, ...(shot ? [shot] : [])], {encoding:'utf8', timeout:30000})).value;
}
async function ready(page) {
  let diagnostic = 'No response yet';
  for (let n = 0; n < 120; n++) {
    try {
      const targets = await (await fetch('http://127.0.0.1:9337/json/list')).json();
      diagnostic = JSON.stringify(targets.map(({type, title, url}) => ({type, title, url})));
      if (targets.some(target => target.url.endsWith(`/${page}.html`)) &&
          evaluate(page, `document.readyState === 'complete' && !!window.__TAURI__?.core && !!document.querySelector('${page === 'settings' ? '#matra-theme' : '#pill'}')`)) return;
    } catch (error) { diagnostic = String(error); }
    await pause(500);
  }
  throw new Error(`${page} WebView2 did not become available: ${diagnostic}`);
}
async function saved(command, predicate) {
  for (let n = 0; n < 30; n++) {
    const value = evaluate('settings', `window.__TAURI__.core.invoke('${command}')`);
    if (predicate(value)) return value;
    await pause(100);
  }
  throw new Error(`${command} did not persist the UI choice`);
}
(async () => {
  await ready('notch');
  evaluate('notch', `invoke('open_settings')`);
  await ready('settings');
  // Seed local fixture readings only for visual rendering, not provider claims.
  evaluate('notch', `providers=()=>[{id:'claude',base:'claude',name:'Claude',glyph:'C',snap:{status:'ok',fetched_at:Date.now(),windows:[{id:'session',used:.12},{id:'seven_day',used:.82}]}}];weeklyRing='outside';onHover=false;renderRing();unfold();`);
  evaluate('settings', `document.querySelector('#tab-appearance').click()`);
  const choices = evaluate('settings', `[...document.querySelectorAll('#matra-theme button')].map(b=>b.dataset.theme)`);
  assert.deepEqual(choices, ['glass','darkGlass','dark','light','system']);
  const themeFits = evaluate('settings', `(()=>{const group=document.querySelector('#matra-theme').closest('.group').getBoundingClientRect();return [...document.querySelectorAll('#matra-theme button')].every(button=>{const r=button.getBoundingClientRect();return r.left>=group.left && r.right<=group.right && r.right<=innerWidth && r.width>0 && button.scrollWidth<=button.clientWidth+1;});})()`);
  assert.equal(themeFits, true, 'All five theme choices fit the actual Settings window without clipping');
  for (const theme of choices) {
    evaluate('settings', `document.querySelector('[data-theme="${theme}"]').click()`);
    await saved('get_matra_theme', value => value === theme);
    await pause(150);
    const result = evaluate('notch', `({theme:document.documentElement.dataset.matraTheme,material:document.documentElement.dataset.matraMaterial,system:matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light',ring:getComputedStyle(document.querySelector('svg.ring circle')).fill})`, `windows-185-${theme}-notch.png`);
    const glass = ['glass','darkGlass'].includes(theme);
    assert.equal(result.theme, glass ? 'dark' : theme === 'system' ? result.system : theme);
    assert.equal(result.material, glass ? 'glass' : 'solid');
    if (glass) assert.equal(result.ring, 'rgba(0, 0, 0, 0)', 'Glass icon backing is transparent');
    evaluate('settings', 'document.title', `windows-185-${theme}-settings.png`);
  }
  const before = evaluate('notch', `[...document.querySelectorAll('.quota-current,.quota-weekly')].map(n=>n.getAttribute('stroke'))`);
  assert.equal(before.length, 2, 'The fixture renders both quota rings');
  for (const accent of ['eb4236', '36a8eb']) {
    evaluate('settings', `document.querySelector('[data-accent="${accent}"]').click()`);
    await saved('get_matra_accent', value => value === accent);
    assert.deepEqual(evaluate('notch', `[...document.querySelectorAll('.quota-current,.quota-weekly')].map(n=>n.getAttribute('stroke'))`), before);
  }
  evaluate('settings', `document.querySelector('#quota-watch').value=40;document.querySelector('#quota-watch').dispatchEvent(new Event('change'));`);
  await saved('get_quota_colors', value => value.watch === .4);
  evaluate('settings', `document.querySelector('#quota-critical').value=80;document.querySelector('#quota-critical').dispatchEvent(new Event('change'));`);
  await saved('get_quota_colors', value => value.critical === .8);
  evaluate('settings', `document.querySelector('[data-mode="ramp"]').click()`);
  await saved('get_quota_colors', value => value.mode === 'ramp');
  assert.match(evaluate('notch', 'tone(.2)'), /color-mix/);
  evaluate('settings', `document.querySelector('#quota-reset').click()`);
  await saved('get_quota_colors', value => value.mode === 'step' && value.watch === .5 && value.critical === .7);
  evaluate('settings', `document.querySelector('#quota-critical').value=85;document.querySelector('#quota-critical').dispatchEvent(new Event('change'));`);
  await saved('get_quota_colors', value => value.critical === .85);
  evaluate('settings', `document.querySelector('#quota-watch').value=80;document.querySelector('#quota-watch').dispatchEvent(new Event('change'));`);
  await saved('get_quota_colors', value => value.watch === .8);
  evaluate('settings', `document.querySelector('#close').click()`);
  await pause(300);
  evaluate('notch', `invoke('open_settings')`);
  await ready('settings');
  await saved('get_quota_colors', value => value.watch === .8 && value.critical === .85);
  assert.deepEqual(evaluate('settings', `[document.querySelector('#quota-watch').value,document.querySelector('#quota-critical').value]`), ['80','85']);
  evaluate('settings', `document.querySelector('#quota-reset').click()`);
  await saved('get_quota_colors', value => value.watch === .5 && value.critical === .7);
  assert.equal(evaluate('settings', `document.querySelector('#quota-critical').value`), '70');
  evaluate('settings', `document.querySelector('#tab-general').click();document.querySelector('#sw-auto-install').click()`);
  await saved('get_automatic_updates', value => value === true);
  assert.equal(evaluate('settings', `document.querySelector('#sw-auto-install').getAttribute('aria-checked')`), 'true');
  evaluate('settings', `document.querySelector('#sw-auto-install').click()`);
  await saved('get_automatic_updates', value => value === false);
  evaluate('settings', `document.querySelector('#close').click()`);
  await pause(300);
  evaluate('notch', `invoke('open_settings')`);
  await ready('settings');
  await saved('get_matra_accent', value => value === '36a8eb');
  await saved('get_automatic_updates', value => value === false);
  // Real IPC for the new Windows controls, with fixture credentials only on this disposable runner.
  evaluate('settings', `document.querySelector('#tab-appearance').click()`);
  const parityBefore=evaluate('settings', `invoke('get_parity_settings')`);
  evaluate('settings', `invoke('set_parity_settings',{settings:{...${JSON.stringify(parityBefore)},shows_remaining:true,ring_order:'burnDeficit',shows_limits_in_tray:true}})`);
  await saved('get_parity_settings',s=>s.shows_remaining && s.ring_order==='burnDeficit' && s.shows_limits_in_tray);
  const hidden=evaluate('settings', `invoke('set_ui_flags',{notchVisible:true,trayVisible:false,notchOnHover:false})`);
  assert.equal(hidden.tray_visible,false,'The tray can actually be hidden');
  const shown=evaluate('settings', `invoke('set_ui_flags',{notchVisible:true,trayVisible:true,notchOnHover:false})`);
  assert.equal(shown.tray_visible,true,'The tray can be restored');
  evaluate('settings', `invoke('set_parity_settings',{settings:${JSON.stringify(parityBefore)}})`);
  evaluate('settings', `document.querySelector('#tab-accounts').click()`);
  assert.equal(evaluate('settings', `!!window.MatraProviders`),true);
  const keyResult=evaluate('settings', `(async()=>{
    const endpoint=MatraProviders.draftFrom({name:'CI credential fixture',base_url:'https://first.example.invalid/v1',is_enabled:false});
    const saved=await invoke('save_custom_endpoint',{endpoint,secret:'fixture-only-never-real'});
    try {
      const listed=await invoke('list_custom_endpoints');
      const moved=await invoke('save_custom_endpoint',{endpoint:{...saved,base_url:'https://second.example.invalid/v1'},secret:null});
      return {saved:saved.has_key,listed:listed.find(e=>e.id===saved.id).has_key,leaked:JSON.stringify(listed).includes('fixture-only-never-real'),moved:moved.has_key};
    } finally {await invoke('delete_custom_endpoint',{id:saved.id});}
  })()`);
  assert.deepEqual(keyResult,{saved:true,listed:true,leaked:false,moved:false},'Credential Manager persists a key without exposing or forwarding it to another origin');
  evaluate('settings','document.title','windows-190-sources.png');
  evaluate('settings',`document.querySelector('#tab-costs').click()`);
  const report=evaluate('settings',`invoke('get_activity_report',{range:'week',account:null,project:null,date:null})`);
  assert.ok(Array.isArray(report.rows));
  assert.equal(report.currency,undefined);
  assert.equal(report.cost_basis,'api_list_price_estimate');
  await pause(200);
  evaluate('settings','document.title','windows-190-activity.png');
  evaluate('settings',`invoke('open_activity')`);
  for(let n=0;n<40;n++){
    try {if(evaluate('activity',`!!document.querySelector('#activity-root') && document.readyState==='complete'`))break;}catch{}
    await pause(250);
  }
  assert.equal(evaluate('activity',`!!window.MatraActivity && !!document.querySelector('#activity-root')`),true);
  evaluate('activity','document.title','windows-190-activity-window.png');
  evaluate('activity',`window.__TAURI__.window.getCurrentWindow().close()`);
  const controls=await testNativeHandles(expression=>evaluate('notch',expression));
  assert.equal(controls.cases,16,'Six/ten provider fixtures fit four native edges with and without bottom/right work insets');
  console.log('PASS: far-end settings/grip pair, always-visible grip, exact native hot payload, disabled grip and usable viewport', JSON.stringify(controls));
  // Exercise the new card inside the packaged renderer with synthetic data.
  // This checks rendering and hover handling, not live availability or OS hover.
  evaluate('notch', `(()=>{
    const now=Date.now(), days=CodexDetails.calendar(now);
    providers=()=>[{id:'codex',base:'codex',name:'Codex',glyph:'C',snap:{status:'ok',fetched_at:now,
      windows:[{id:'secondary',label:'Weekly limit',used:.12,resets_at:now+86400000}],
      codex_details:{plan:'pro',fetched_at:now,credits:{available_count:3,next_expiry_ms:now+86400000,fetched_at:now},
        statistics:{lifetime_tokens:1234567890,peak_daily_tokens:12000000,longest_running_turn_sec:7380,
          current_streak_days:12,longest_streak_days:20,daily_usage_buckets:days.map((day,i)=>({start_date:day.key,tokens:(i+1)*100000}))}}}}];
    activity=[{provider:'codex',name:'Release fixture title\\nThis prompt body must stay hidden',state:'running',since:now}];
    hoverId='codex';onHover=false;renderRing();unfold();showCard();
  })()`);
  const codex = evaluate('notch', `(()=>{
    showCard();const r=card.getBoundingClientRect(),text=card.textContent;
    const day=card.querySelector('[data-codex-day="29"]');day.dispatchEvent(new MouseEvent('mouseenter'));
    return {rich:card.classList.contains('codex-rich'),bars:card.querySelectorAll('.cx-day').length,
      fields:['Unused resets','Lifetime tokens','Peak daily tokens','Longest chat','Current streak','Longest streak','Today','30-day tokens'].every(label=>text.includes(label)),
      inside:r.left>=-1&&r.top>=-1&&r.right<=innerWidth+1&&r.bottom<=innerHeight+1,
      exactDay:card.querySelector('.cx-day-readout').textContent,
      hiddenBody:!text.includes('This prompt body must stay hidden'),
      activityRows:card.querySelectorAll('.s-state').length,
      status:[...card.querySelectorAll('.s-state')].every(node=>getComputedStyle(node).whiteSpace==='nowrap')};
  })()`, 'windows-185-codex-fixture.png');
  assert.equal(codex.rich, true);
  assert.equal(codex.bars, 30);
  assert.equal(codex.fields, true);
  assert.equal(codex.inside, true, 'Rich Codex card fits the packaged native viewport');
  assert.match(codex.exactDay, /3[,.]000[,.]000 tokens/);
  assert.equal(codex.hiddenBody, true);
  assert.equal(codex.activityRows, 1);
  assert.equal(codex.status, true);
  console.log('PASS: packaged Windows UI, five themes, glass circles, accent isolation, quota controls, auto-update toggle, reopen persistence and Codex detail fixture');
})().catch(error => { console.error(error); process.exitCode = 1; });
