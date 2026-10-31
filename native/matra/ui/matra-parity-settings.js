/* Display and AI sources controls for the parity settings. Every change sends the complete object,
   merged from what was last loaded. A failed load disables the controls instead of offering defaults
   that would overwrite saved choices, and a failed save restores what is actually saved. */
(() => {
  const api = window.__TAURI__;
  const byId = id => document.getElementById(id);
  const P = window.MatraParity;
  const t = (key) => (window.matraUi ? window.matraUi(key, key) : key);
  const SWITCHES = { 'sw-show-remaining': 'shows_remaining', 'sw-hide-depleted': 'hide_depleted',
    'sw-shut-rings': 'shut_rings_when_spent', 'sw-readings': 'shows_notch_readings', 'sw-tray-limits': 'shows_limits_in_tray' };
  const SELECTS = { 'menu-reading': 'menu_bar_reading', 'menu-click': 'menu_bar_click_action', 'menu-activity': 'menu_bar_activity_style' };
  const ORDER_WHY = {
    manual: 'The notch draws rings in the order below. Use the arrows on a source to move it.',
    remaining: 'Most remaining first, by the window that leads each ring. A high number that cannot be spent yet sinks below usable accounts but stays above empty ones.',
    burnDeficit: 'Quota expiring soonest first: remaining quota minus remaining time. 70% left with 23h of a week left (about 56 to burn) outranks 100% with 6 days left (about 14 to burn).'
  };
  const orderButtons = [...document.querySelectorAll('#seg-ring-order button')];
  let saved = P.normalize(null), loaded = false, busy = false, sources = [];

  function status(text) { const el = byId('parity-status'); if (el) el.textContent = text; }
  /* The status line lives on Display, so a failure from another pane also uses the window's strip. */
  function failed(text) { status(text); if (window.strip) window.strip(text); }
  function controls() {
    return [...Object.keys(SWITCHES), ...Object.keys(SELECTS)].map(byId).filter(Boolean).concat(orderButtons);
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
    if (window.matraParityChanged) window.matraParityChanged(saved);
  }
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
  } else { status(t('These settings could not be loaded. Reopen Settings to retry.')); }
})();
