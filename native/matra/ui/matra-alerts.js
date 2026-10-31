/* Notifications pane. Every change sends the complete object, merged from what was last loaded. A failed
   load disables the controls instead of offering defaults that would overwrite saved choices, and a
   failed save restores what is actually saved. The test button is the only thing here that sends. */
(root => {
  const DEFAULTS = Object.freeze({ enabled: false, usage_watch: false, usage_critical: false, usage_limit_session: false,
    usage_limit_weekly: false, usage_reset: false, work_complete: false, attention: false, sound: true, muted_providers: [] });
  const SECTIONS = [
    { title: 'Usage', hint: 'Once per crossing, and again only after usage falls back or the window rolls over.', rows: [
      { key: 'usage_watch', label: 'Reaches Watch' },
      { key: 'usage_critical', label: 'Reaches Critical' },
      { key: 'usage_limit_session', label: 'Session limit reached' },
      { key: 'usage_limit_weekly', label: 'Weekly or monthly limit reached' },
      { key: 'usage_reset', label: 'A limit resets' }
    ] },
    { title: 'Sessions', hint: 'Names the session folder or provider, never the prompt or the question being asked.', rows: [
      { key: 'work_complete', label: 'Work finishes' },
      { key: 'attention', label: 'An agent is waiting on you' }
    ] }
  ];

  function normalize(value) {
    const out = Object.assign({}, DEFAULTS, value || {});
    for (const key of Object.keys(DEFAULTS)) {
      if (key !== 'muted_providers') out[key] = out[key] === true;
    }
    out.muted_providers = Array.isArray(value && value.muted_providers)
      ? [...new Set(value.muted_providers.filter(id => typeof id === 'string' && id))] : [];
    return out;
  }
  function toggleMuted(settings, id) {
    const muted = settings.muted_providers.includes(id)
      ? settings.muted_providers.filter(other => other !== id)
      : settings.muted_providers.concat(id);
    return Object.assign({}, settings, { muted_providers: muted });
  }
  function percent(value) {
    return Number.isFinite(value) ? `${Math.round(value * 100)}%` : '';
  }
  /* Delivered means Windows accepted it. Whether a banner appeared, under Do Not Disturb say, is unknown. */
  function lastDelivery(last, T) {
    if (!last) return '';
    return last.ok ? T('Windows accepted the last notification.') : `${T('The last notification failed.')} ${last.error || ''}`.trim();
  }
  function reason(error) {
    return typeof error === 'string' && error ? error : 'Unavailable in this build.';
  }

  function mount(host, options) {
    const invoke = options.invoke, T = options.ui || (key => key);
    const listen = options.listen || (root.__TAURI__ && root.__TAURI__.event && root.__TAURI__.event.listen);
    let saved = normalize(null), status = null, loaded = false, busy = false, note = '', loadError = '';

    const el = (tag, className, text) => {
      const node = document.createElement(tag);
      if (className) node.className = className;
      if (text !== undefined) node.textContent = text;
      return node;
    };
    const available = () => loaded && !busy;

    function switchRow(label, on, disabled, change) {
      const row = el('div', 'item');
      row.append(el('span', '', T(label)));
      const button = el('button', on ? 'switch on' : 'switch');
      button.type = 'button';
      button.setAttribute('role', 'switch');
      button.setAttribute('aria-checked', String(on));
      button.setAttribute('aria-label', T(label));
      button.disabled = disabled;
      button.addEventListener('click', change);
      row.append(button);
      return row;
    }

    function render() {
      host.textContent = '';
      if (!loaded) {
        const group = el('div', 'group');
        group.append(el('div', 'item cap', loadError ? `${T('Notification preferences are unavailable.')} ${T(loadError)}` : T('Loading')));
        host.append(group);
        return;
      }
      const master = el('div', 'group');
      master.append(switchRow('Windows notifications', saved.enabled, !available(), () => save(Object.assign({}, saved, { enabled: !saved.enabled }))));
      master.append(switchRow('Play a sound', saved.sound, !available() || !saved.enabled,
        () => save(Object.assign({}, saved, { sound: !saved.sound }))));
      master.append(el('div', 'item cap', status ? T(status.message) : T('Windows notification status is unavailable.')));
      if (saved.enabled && status && !status.can_deliver) {
        master.append(el('div', 'item cap', T('Nothing can be delivered until this is fixed. Your choices are kept.')));
      }
      const test = el('div', 'item');
      test.append(el('span', 'hint', T('Sends one real notification, even with the switch above off.')));
      const send = el('button', 'btn', T('Send a test'));
      send.type = 'button';
      send.disabled = busy;
      send.addEventListener('click', sendTest);
      test.append(send);
      master.append(test);
      const report = note || (status ? lastDelivery(status.last, T) : '');
      if (report) {
        const line = el('div', 'item cap', report);
        line.setAttribute('role', 'status');
        master.append(line);
      }
      host.append(master);

      for (const section of SECTIONS) {
        host.append(el('div', 'sec', T(section.title)));
        const group = el('div', 'group');
        for (const row of section.rows) {
          group.append(switchRow(row.label, saved[row.key], !available() || !saved.enabled,
            () => save(Object.assign({}, saved, { [row.key]: !saved[row.key] }))));
        }
        group.append(el('div', 'item cap', T(section.hint)));
        host.append(group);
      }

      const providers = status && Array.isArray(status.providers) ? status.providers : [];
      if (providers.length) {
        host.append(el('div', 'sec', T('Providers')));
        const group = el('div', 'group');
        for (const provider of providers) {
          const on = !saved.muted_providers.includes(provider.id);
          group.append(switchRow(provider.name, on, !available() || !saved.enabled, () => save(toggleMuted(saved, provider.id))));
        }
        group.append(el('div', 'item cap', T('Turn one off to mute its usage notifications.')));
        host.append(group);
      }
      if (status && Number.isFinite(status.watch) && Number.isFinite(status.critical)) {
        const shared = `${T('Watch')} ${percent(status.watch)}, ${T('Critical')} ${percent(status.critical)}.`;
        const group = el('div', 'group');
        group.append(el('div', 'item cap', shared));
        host.append(group);
      }
    }

    async function save(next) {
      const before = saved;
      saved = normalize(next);
      busy = true; note = '';
      render();
      try {
        saved = normalize(await invoke('set_alert_settings', { settings: saved }));
      } catch (error) {
        saved = before;
        note = `${T('Could not save. Your previous choice is kept.')} ${T(reason(error))}`;
      }
      busy = false;
      render();
    }
    async function sendTest() {
      busy = true; note = '';
      render();
      try {
        status = await invoke('send_test_alert');
        note = T('Windows accepted the test. If no banner appeared, check Do Not Disturb.');
      } catch (error) {
        note = `${T('The test was not delivered.')} ${T(reason(error))}`;
        await refreshStatus();
      }
      busy = false;
      render();
    }
    async function refreshStatus() {
      try { status = await invoke('get_alert_status'); } catch (_) { status = null; }
    }
    async function load() {
      try {
        saved = normalize(await invoke('get_alert_settings'));
        loaded = true; loadError = '';
      } catch (error) {
        loaded = false; loadError = reason(error);
      }
      await refreshStatus();
      render();
    }
    if (listen) {
      Promise.resolve(listen('alert_settings', event => { saved = normalize(event.payload); if (!busy) render(); })).catch(() => {});
      Promise.resolve(listen('alert_status', event => { status = event.payload; if (!busy) render(); })).catch(() => {});
    }
    render();
    const ready = load();
    return { refresh: load, ready };
  }

  root.MatraAlerts = { mount, normalize, toggleMuted, lastDelivery, DEFAULTS };
})(typeof window !== 'undefined' ? window : globalThis);
