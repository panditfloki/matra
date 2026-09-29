/* Mātrā appearance, shared by the meter and settings windows. */
(() => {
  const media = matchMedia('(prefers-color-scheme: dark)');
  function applyMotion(on) {
    const enabled = on !== false;
    document.documentElement.dataset.matraMotion = enabled ? 'on' : 'off';
    const button = document.getElementById('sw-notch-motion');
    if (button) {
      button.classList.toggle('on', enabled);
      button.setAttribute('aria-checked', String(enabled));
    }
  }
  applyMotion(true);
  let choice = localStorage.getItem('matra-theme') || 'darkGlass';
  function apply(value) {
    choice = ['light', 'dark', 'glass', 'darkGlass', 'system'].includes(value) ? value : 'darkGlass';
    const glass = choice === 'glass' || choice === 'darkGlass';
    document.documentElement.dataset.matraTheme = glass ? 'dark' : choice === 'system' ? (media.matches ? 'dark' : 'light') : choice;
    document.documentElement.dataset.matraMaterial = glass ? 'glass' : 'solid';
    document.documentElement.dataset.matraGlass = choice;
    localStorage.setItem('matra-theme', choice);
    document.querySelectorAll('#matra-theme button').forEach(b => {
      b.classList.toggle('on', b.dataset.theme === choice);
      b.setAttribute('aria-pressed', String(b.dataset.theme === choice));
    });
  }
  apply(choice); media.addEventListener('change', () => apply(choice));
  const accents = ['system','ff33e1','eb4236','eb8436','ffd400','00ff88','00e5cc','36a8eb','6c5ce7','b026ff','f7f6f5'];
  let systemAccent = '36a8eb';
  let currentAccent = 'system';
  function applyAccent(value) {
    const selected = accents.includes(value) ? value : 'system';
    currentAccent = selected;
    const hex = selected === 'system' ? systemAccent : selected;
    const style = document.documentElement.style;
    style.setProperty('--matra-user-accent', '#' + hex);
    const channels = hex.match(/../g).map(v => parseInt(v,16) / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
    const ink = channels[0] * .2126 + channels[1] * .7152 + channels[2] * .0722 > .179 ? '#14110f' : '#faf8f6';
    style.setProperty('--matra-user-ink', ink);
    document.querySelectorAll('#matra-accent button').forEach(b => {
      b.classList.toggle('on', b.dataset.accent === selected);
      b.setAttribute('aria-pressed', String(b.dataset.accent === selected));
    });
  }
  applyAccent('system');
  const api = window.__TAURI__;
  function applyAutomatic(on) {
    const button = document.getElementById('sw-auto-install');
    if (!button) return;
    button.classList.toggle('on', on === true);
    button.setAttribute('aria-checked', String(on === true));
  }
  if (api) {
    api.event.listen('matra-theme', e => apply(e.payload));
    api.core.invoke('get_matra_theme').then(apply).catch(console.error);
    api.event.listen('matra-accent', e => applyAccent(e.payload));
    api.core.invoke('get_matra_accent').then(applyAccent).catch(console.error);
    api.core.invoke('get_matra_system_accent').then(value => {
      if (/^[a-f0-9]{6}$/i.test(value)) systemAccent = value;
      if (currentAccent === 'system') applyAccent('system');
    }).catch(console.error);
    api.event.listen('matra-automatic-updates', e => applyAutomatic(e.payload));
    api.core.invoke('get_automatic_updates').then(applyAutomatic).catch(console.error);
    api.event.listen('matra-notch-motion', e => applyMotion(e.payload));
    api.core.invoke('get_notch_motion').then(applyMotion).catch(() => {});
  }
  document.getElementById('sw-auto-install')?.addEventListener('click', async () => {
    const button = document.getElementById('sw-auto-install');
    const next = button.getAttribute('aria-checked') !== 'true';
    button.disabled = true;
    try {
      applyAutomatic(api ? await api.core.invoke('set_automatic_updates', { on: next }) : next);
      document.getElementById('auto-install-status').textContent = next
        ? 'Automatic installation enabled. Verified updates can restart Mātrā.'
        : 'Automatic installation off. You choose when updates install.';
    } catch {
      document.getElementById('auto-install-status').textContent = 'Could not save the update preference. Your previous choice is unchanged.';
    } finally { button.disabled = false; }
  });
  document.getElementById('sw-notch-motion')?.addEventListener('click', async () => {
    const button = document.getElementById('sw-notch-motion');
    const next = button.getAttribute('aria-checked') !== 'true';
    button.disabled = true;
    try { applyMotion(api ? await api.core.invoke('set_notch_motion', { on: next }) : next); }
    catch { /* Keep the saved state when the bridge rejects the change. */ }
    finally { button.disabled = false; }
  });
  document.querySelectorAll('#matra-theme button').forEach(button => {
    button.addEventListener('click', async () => {
      const buttons = document.querySelectorAll('#matra-theme button');
      buttons.forEach(b => b.disabled = true);
      try {
        const value = api ? await api.core.invoke('set_matra_theme', { theme: button.dataset.theme }) : button.dataset.theme;
        apply(value);
        document.getElementById('matra-theme-status').textContent = 'Saved. Notch and settings use this theme.';
      } catch {
        document.getElementById('matra-theme-status').textContent = 'Theme could not be saved. Try again.';
      } finally { buttons.forEach(b => b.disabled = false); }
    });
  });
  document.querySelectorAll('#matra-accent button').forEach(button => {
    button.addEventListener('click', async () => {
      const buttons = document.querySelectorAll('#matra-accent button');
      buttons.forEach(b => b.disabled = true);
      try {
        applyAccent(api ? await api.core.invoke('set_matra_accent', { accent: button.dataset.accent }) : button.dataset.accent);
        document.getElementById('matra-accent-status').textContent = 'Accent saved for app controls. Usage rings and bars keep their status colours.';
      } catch {
        document.getElementById('matra-accent-status').textContent = 'Accent could not be saved. Your previous colour is unchanged.';
      } finally { buttons.forEach(b => b.disabled = false); }
    });
  });
  const side = document.getElementById('side');
  if (side) {
    const note = document.createElement('small'); note.className = 'matra-credit';
    note.textContent = 'Mātrā · Developed by Pandit Floki'; side.append(note);
  }
})();
