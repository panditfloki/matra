/* Runtime platform capabilities. Controls and commands share the same native decision. */
(() => {
  'use strict';
  const core = window.__TAURI__?.core;
  if (!core?.invoke || !window.__TAURI_INTERNALS__) return;
  const invoke = core.invoke.bind(core);
  const reasons = {
    updates: 'Updates are disabled in Development 0.2.0. Install a verified development package.',
    notch_drag: 'Perimeter carry requires Windows or an X11 desktop session.',
    global_pointer: 'Global pointer tracking requires Windows or an X11 desktop session.',
    absolute_window_placement: 'This desktop does not allow absolute notch placement. Use an X11 login session.',
    terminal_focus: 'This desktop cannot identify and focus the originating terminal.',
    autostart: 'Login startup is unavailable in this isolated fixture.',
    hooks: 'Provider hooks are unavailable in this isolated fixture.',
    notifications: 'The desktop notification service is unavailable.',
    secure_storage: 'Secure storage is unavailable; no plaintext credential will be saved.'
  };
  const commands = {
    begin_move: 'notch_drag', drag_begin: 'notch_drag', set_move_handle: 'notch_drag',
    set_notch_edge: 'absolute_window_placement', set_notch_monitor: 'absolute_window_placement',
    reset_notch_position: 'absolute_window_placement', focus_session: 'terminal_focus',
    set_autostart: 'autostart', set_hooks_installed: 'hooks',
    install_opencode_bridge: 'hooks', remove_opencode_bridge: 'hooks',
    check_for_update: 'updates', check_update: 'updates', download_update: 'updates',
    install_update: 'updates', set_automatic_updates: 'updates', set_provider_secret: 'secure_storage'
  };
  const ready = invoke('get_platform_info');
  window.matraPlatformReady = ready;
  const guardedInvoke = (command, args, options) => {
    const capability = commands[command];
    if (!capability) return invoke(command, args, options);
    return ready.then(info => {
      if (!info.capabilities?.[capability]) throw new Error(reasons[capability]);
      return invoke(command, args, options);
    });
  };
  // Tauri freezes its core module namespace; replace the outer module slot, never mutate it.
  window.__TAURI__.core = { ...core, invoke: guardedInvoke };
  ready.then(info => {
    const selectors = {
      updates: '#sw-auto-install,#btn-update,#update-confirm',
      notch_drag: '#sw-move,#move-handle',
      global_pointer: '#seg-show [data-v="hover"]',
      absolute_window_placement: '#seg-edge button,#screen,#btn-recentre',
      autostart: '#sw-autostart', hooks: '[data-remote="hooks"]',
      terminal_focus: '[data-action="focus-session"],[data-cmd="focus_session"]'
    };
    function apply() {
      for (const [key, selector] of Object.entries(selectors)) {
        if (info.capabilities?.[key]) continue;
        document.querySelectorAll(selector).forEach(control => {
          if (control.getAttribute('aria-disabled') !== 'true') control.setAttribute('aria-disabled', 'true');
          if ('disabled' in control && !control.disabled) control.disabled = true;
          if (control.title !== reasons[key]) control.title = reasons[key];
          if (control.id === 'move-handle') control.style.visibility = 'hidden';
        });
      }
      if (!info.capabilities?.updates) {
        const line = document.getElementById('auto-install-status');
        if (line && line.textContent !== reasons.updates) line.textContent = reasons.updates;
        const row = document.getElementById('btn-update')?.closest('.item');
        const note = row?.nextElementSibling;
        if (note?.classList.contains('cap') && note.textContent !== reasons.updates) note.textContent = reasons.updates;
      }
    }
    const start = () => {
      apply();
      const about = document.getElementById('pane-general');
      if (about && !document.getElementById('platform-status')) {
        const status = document.createElement('p');
        status.id = 'platform-status'; status.className = 'item cap'; status.setAttribute('role', 'status');
        status.textContent = `${info.label} · Development 0.2.0. ${info.limitations.join(' ')}`;
        about.prepend(status);
      }
      const observer = new MutationObserver(apply);
      observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["disabled"] });
    };
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true }); else start();
  }).catch(() => { /* Native commands still fail closed; never invent a supported capability. */ });
})();
