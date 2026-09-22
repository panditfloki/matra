// Isolated fixtures for documentation screenshots. No native IPC or credentials.
const values = {
  get_matra_theme: 'dark', get_notch_motion: true, get_lang: 'en', get_lang_resolved: 'en',
  get_system_look: { mica: false }, get_scale: 1, get_notch_edge: 'left',
  get_weekly_ring: 'outside', get_move_handle: true, get_hooks_installed: true,
  get_autostart: false, get_notch_slots: [], get_glyphs: {}, get_monitors: [],
  get_ui_flags: { notch_visible: true, notch_on_hover: true, tray_visible: true },
  get_antigravity_prefs: { limit: 'automatic', model: 'gemini' },
  get_update_state: {},
  get_tray_options: ['Claude', 'Codex', 'Cursor', 'Antigravity', 'Grok', 'GLM'].map((label, i) => ({
    id: ['claude', 'codex', 'cursor', 'gemini', 'grok', 'glm'][i], label,
    status: i < 2 ? 'ok' : 'absent', used: i === 0 ? .17 : .35
  }))
};
window.__TAURI__ = {
  core: { invoke: async (name, args) => {
    if (name === 'set_matra_theme') return args.theme;
    if (name === 'set_notch_motion') { values.get_notch_motion = args.on; return args.on; }
    if (Object.hasOwn(values, name)) return values[name];
    throw new Error('Preview does not implement: ' + name);
  } },
  event: { listen: async () => () => {} },
  app: { getVersion: async () => require('../native/codenotch/tauri.conf.json').version },
  window: { getCurrentWindow: () => ({ close() {}, startDragging() {} }) }
};
