// Browser-only fixtures for the Windows notch review. Never loads native IPC.
(() => {
  const listeners = new Map();
  const calls = [];
  const ids = ['claude', 'codex', 'cursor', 'gemini', 'grok', 'glm'];
  const allIds = [...ids, ...Array.from({ length: 14 }, (_, i) => 'custom:sample-' + (i + 1))];
  const now = Date.now();
  const requestedScale = Number(new URLSearchParams(location.search).get('scale') || 1);
  const scale = [.8, 1, 1.25].includes(requestedScale) ? requestedScale : 1;
  const windowOf = (id, used, hours) => ({ id, label: id === 'weekly' ? 'Weekly' : 'Current session', used, resets_at: now + hours * 3600000 });
  const snap = (windows, plan = 'Sample plan') => ({ status: 'ok', windows, fetched_at: now, plan, note: '' });
  const absent = { status: 'absent', windows: [], fetched_at: 0 };
  const values = {
    get_matra_theme: 'dark', get_matra_accent: 'system', get_matra_system_accent: '36a8eb',
    get_notch_motion: true, get_automatic_updates: false, get_scale: scale,
    get_lang: 'en', get_lang_resolved: 'en', get_system_look: { mica: false },
    get_notch_edge: 'right', get_notch_insets: [0, 0, 0, 0],
    get_notch_slots: ids.slice(0, 3).map(provider => ({ provider })), get_weekly_ring: 'outside', get_move_handle: true,
    get_glyphs: window.__matraSampleGlyphs || {}, get_activity: [], get_extra_usage: {}, get_parity_usage: {},
    get_parity_settings: { show_move_handle: true }, get_quota_colors: { watch: .5, critical: .7, mode: 'step' },
    get_antigravity_prefs: { limit: 'automatic', model: 'gemini' },
    get_state: { sessions: [], agg: 'idle', lang_resolved: 'en' },
    get_ui_flags: { notch_visible: true, notch_on_hover: false, tray_visible: false },
    get_claude_auth: { busy: false }, get_update_state: { phase: 'idle' },
    get_usage: snap([windowOf('session', .17, 3), windowOf('seven_day', .43, 96)]),
    get_codex: snap([windowOf('primary', .32, 4), windowOf('secondary', .58, 72)]),
    get_cursor: snap([windowOf('auto', .69, 240), windowOf('api', .24, 240)]),
    get_antigravity: snap([windowOf('gemini', .27, 5)]),
    get_grok: snap([windowOf('credits', .38, 24)]), get_glm: snap([windowOf('tokens', .46, 5)])
  };
  const emit = (name, payload) => { for (const listener of listeners.get(name) || []) listener({ payload: structuredClone(payload) }); };
  window.__TAURI__ = {
    core: { invoke: async (name, args) => {
      calls.push({ name, args: structuredClone(args || {}), at: Date.now() });
      if (calls.length > 500) calls.splice(0, calls.length - 500);
      if (Object.hasOwn(values, name)) return structuredClone(values[name]);
      if (name === 'refresh_ring') return false;
      if (name === 'begin_move') setTimeout(() => emit('move_end', null), 30);
      if (name === 'drag_begin') setTimeout(() => emit('drag_end', null), 30);
      if (['set_hot', 'report_dpr', 'log_js', 'notch_hidden', 'open_settings', 'refresh_on_look', 'begin_move', 'drag_begin', 'show_notch_menu'].includes(name)) return null;
      throw new Error('Sample bridge has no command: ' + name);
    } },
    event: { listen: async (name, handler) => {
      if (!listeners.has(name)) listeners.set(name, new Set());
      listeners.get(name).add(handler);
      return () => listeners.get(name).delete(handler);
    } },
    app: { getVersion: async () => '0.2.0' },
    window: { getCurrentWindow: () => ({ close() {}, startDragging() {} }) }
  };
  window.__matraSample = {
    calls, emit,
    configure({ edge = 'right', count = 3, theme = 'dark', fold = false, insets = [0, 0, 0, 0], stale = false, readings = true } = {}) {
      const checkedEdge = ['left', 'right', 'top', 'bottom'].includes(edge) ? edge : 'right';
      values.get_notch_edge = checkedEdge;
      const countShown = Math.max(1, Math.min(20, Number(count) || 3));
      values.get_notch_slots = allIds.slice(0, countShown).map(provider => ({ provider }));
      values.get_parity_usage = Object.fromEntries(allIds.slice(6, countShown).map((id, i) => [id, { ...snap([windowOf('monthly', .2 + i * .03, 48)]), display_name: 'Sample provider ' + (i + 7) }]));
      values.get_parity_settings.shows_notch_readings = !!readings;
      values.get_matra_theme = ['dark', 'light', 'darkGlass'].includes(theme) ? theme : 'dark';
      values.get_notch_insets = insets;
      values.get_ui_flags.notch_on_hover = !!fold;
      values.get_usage.status = stale ? 'stale' : 'ok';
      emit('parity_usage', values.get_parity_usage); emit('parity_settings', values.get_parity_settings);
      emit('notch_edge', checkedEdge); emit('notch_slots', values.get_notch_slots);
      emit('matra-theme', values.get_matra_theme); emit('notch_insets', insets);
      emit('ui_flags', values.get_ui_flags); emit('usage', values.get_usage);
    },
    absent
  };
})();
