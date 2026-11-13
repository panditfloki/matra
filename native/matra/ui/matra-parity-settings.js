/* Display and AI sources controls for the parity settings. Every change sends the complete object,
   merged from what was last loaded. A failed load disables the controls instead of offering defaults
   that would overwrite saved choices, and a failed save restores what is actually saved. */
(() => {
  const api = window.__TAURI__;
  const byId = id => document.getElementById(id);
  const P = window.MatraParity;
  const t = (key) => (window.matraUi ? window.matraUi(key, key) : key);
  const SWITCHES = { 'sw-show-remaining': 'shows_remaining', 'sw-hide-depleted': 'hide_depleted',
    'sw-weekly-headline':'weekly_headline','sw-weekly-reading':'weekly_reading','sw-weekly-dashed':'weekly_ring_dashed',
    'sw-daily-pace':'claude_daily_pace','sw-usage-pace':'show_usage_pace','sw-extra-limits':'show_codex_extra_limits',
    'sw-refresh-look':'asks_provider_on_look','sw-fullscreen':'folds_for_fullscreen','sw-custom-scale':'uses_custom_scale',
    'sw-shut-rings': 'shut_rings_when_spent', 'sw-readings': 'shows_notch_readings', 'sw-tray-limits': 'shows_limits_in_tray' };
  const SELECTS = { 'menu-reading': 'menu_bar_reading', 'menu-click': 'menu_bar_click_action', 'menu-activity': 'menu_bar_activity_style' };
  const ORDER_WHY = {
    manual: 'The notch draws rings in the order below. Use the arrows on a source to move it.',
    remaining: 'Most remaining first, by the window that leads each ring. A high number that cannot be spent yet sinks below usable accounts but stays above empty ones.',
    burnDeficit: 'Quota expiring soonest first: remaining quota minus remaining time. 70% left with 23h of a week left (about 56 to burn) outranks 100% with 6 days left (about 14 to burn).'
  };
  const orderButtons = [...document.querySelectorAll('#seg-ring-order button')];
  let saved = P.normalize(null), loaded = false, busy = false, sources = [], accounts=[], fullscreenAvailable=false;

  function status(text) { const el = byId('parity-status'); if (el) el.textContent = text; }
  /* The status line lives on Display, so a failure from another pane also uses the window's strip. */
  function failed(text) { status(text); if (window.strip) window.strip(text); }
  function controls() {
    return [...Object.keys(SWITCHES), ...Object.keys(SELECTS)].map(byId).filter(Boolean).concat(orderButtons,byId('custom-scale')).filter(Boolean);
  }
  function sync() {
    for (const [id, key] of Object.entries(SWITCHES)) {
      const el = byId(id); if (!el) continue;
      el.classList.toggle('on', saved[key] === true);
      el.setAttribute('aria-checked', String(saved[key] === true));
    }
    for (const [id, key] of Object.entries(SELECTS)) { const el = byId(id); if (el) el.value = saved[key]; }
    for (const b of orderButtons) {
      const on = b.dataset.v === saved.ring_order;
      b.classList.toggle('on', on);
      b.setAttribute('aria-pressed', String(on));
    }
    const why = byId('cap-ring-order');
    if (why) why.textContent = t(ORDER_WHY[saved.ring_order]);
    const list = byId('tray-providers');
    if (list) {
      list.hidden = !saved.shows_limits_in_tray;
      list.textContent = '';
      for (const source of sources) {
        const b = document.createElement('button');
        const on = saved.tray_providers.includes(source.id);
        b.type = 'button'; b.dataset.id = source.id; b.className = 'matra-chip' + (on ? ' on' : '');
        b.setAttribute('aria-pressed', String(on));
        b.textContent = source.label || source.id;
        b.disabled = !loaded || busy;
        list.append(b);
      }
    }
    for (const el of controls()) el.disabled = !loaded || busy;
    const scale=byId('custom-scale');if(scale){scale.value=saved.custom_scale;byId('custom-scale-row').hidden=!saved.uses_custom_scale;byId('custom-scale-value').textContent=Math.round(saved.custom_scale*100)+'%';}
    const full=byId('sw-fullscreen');if(full)full.disabled=!loaded||busy||!fullscreenAvailable;
    renderAccounts();
    if (window.matraParityChanged) window.matraParityChanged(saved);
  }
  function accountItems(){
    const items=[...accounts,...sources.filter(p=>p.id!=='claude').map(p=>({id:p.id,name:p.label||p.id}))];
    for(const id of [...saved.account_order,...Object.keys(saved.account_nicknames)]) if(!items.some(p=>p.id===id))items.push({id,name:id+' (unavailable)'});
    return P.arrange(items,saved.account_order,p=>p.id);
  }
  function renderAccounts(){
    const host=byId('account-presentation');if(!host||(host.contains(document.activeElement)&&document.activeElement?.tagName==='INPUT'))return;
    host.replaceChildren();const items=accountItems();
    for(const [index,item] of items.entries()){
      const row=document.createElement('div');row.className='acct-sub';
      const input=document.createElement('input');input.value=saved.account_nicknames[item.id]||'';input.placeholder=item.name;input.maxLength=80;
      input.setAttribute('aria-label',item.name+' nickname');input.disabled=!loaded||busy;input.addEventListener('blur',()=>queueMicrotask(renderAccounts));
      input.addEventListener('change',()=>{const names={...saved.account_nicknames};if(input.value.trim())names[item.id]=input.value.trim();else delete names[item.id];save({account_nicknames:names});});row.append(input);
      for(const [step,label] of [[-1,'↑'],[1,'↓']]){const b=document.createElement('button');b.type='button';b.className='btn';b.textContent=label;b.setAttribute('aria-label','Move '+item.name+(step<0?' up':' down'));
        b.disabled=!loaded||busy||saved.ring_order!=='manual'||index+step<0||index+step>=items.length;
        b.addEventListener('click',()=>save({account_order:P.moved(items.map(p=>p.id),item.id,step)}));row.append(b);}
      host.append(row);
    }
  }
  byId('custom-scale')?.addEventListener('input',e=>{byId('custom-scale-value').textContent=Math.round(Number(e.target.value)*100)+'%';});
  byId('custom-scale')?.addEventListener('change',e=>save({custom_scale:Number(e.target.value),uses_custom_scale:true}));
  function apply(value) { saved = P.normalize(value); sync(); }
  async function save(patch) {
    if (!loaded || busy) return;
    busy = true; sync();
    try {
      saved = P.normalize(await api.core.invoke('set_parity_settings', { settings: P.merge(saved, patch) }));
      status(t('Saved.'));
    } catch (e) {
      failed(t('Could not save this setting. Your previous choice is unchanged.'));
    } finally { busy = false; sync(); }
  }
  for (const [id, key] of Object.entries(SWITCHES)) {
    const el = byId(id);
    if (el) el.addEventListener('click', () => save({ [key]: !saved[key] }));
  }
  for (const [id, key] of Object.entries(SELECTS)) {
    const el = byId(id);
    if (el) el.addEventListener('change', () => save({ [key]: el.value }));
  }
  for (const b of orderButtons) b.addEventListener('click', () => { if (b.dataset.v !== saved.ring_order) save({ ring_order: b.dataset.v }); });
  const trayList = byId('tray-providers');
  if (trayList) trayList.addEventListener('click', e => {
    const b = e.target.closest && e.target.closest('button[data-id]');
    if (!b) return;
    const id = b.dataset.id;
    save({ tray_providers: saved.tray_providers.includes(id) ? saved.tray_providers.filter(x => x !== id) : saved.tray_providers.concat(id) });
  });
  window.matraParitySources = list => { sources = Array.isArray(list) ? list : []; sync(); };
  document.addEventListener('matra-ui-language', sync);
  sync();
  // Register before loading so an update from another window cannot be missed.
  if (api) {
    let revision = 0;
    api.event.listen('parity_settings', e => { revision++; loaded = true; apply(e.payload); }).catch(() => {});
    const load = () => {
      const before = revision;
      return api.core.invoke('get_parity_settings').then(value => {
        if (before === revision) { loaded = true; apply(value); }
      }).catch(() => { sync(); status(t('These settings could not be loaded. Reopen Settings to retry.')); });
    };
    load();
    const receiveUsage=value=>{accounts=window.MatraAccounts.claude(value).map(p=>({id:p.id,name:p.name}));renderAccounts();};
    api.event.listen('usage',e=>receiveUsage(e.payload)).catch(()=>{});
    api.core.invoke('get_usage').then(receiveUsage).catch(()=>{});
    api.core.invoke('get_platform_info').then(info=>{fullscreenAvailable=info?.capabilities?.fullscreen_detection===true;const cap=byId('fullscreen-cap');if(!fullscreenAvailable&&cap)cap.textContent='Fullscreen detection is unavailable in this session. Wayland keeps other apps’ window state private; hover folding remains available.';sync();}).catch(()=>{});
  } else { status(t('These settings could not be loaded. Reopen Settings to retry.')); }
})();
