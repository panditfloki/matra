/* Mātrā customisation. Original notch behaviour is maintained upstream. */
(() => {
  const media = matchMedia('(prefers-color-scheme: dark)');
  let choice = localStorage.getItem('matra-theme') || 'system';
  function apply(value) {
    choice = ['light', 'dark', 'glass', 'system'].includes(value) ? value : 'system';
    document.documentElement.dataset.matraTheme = choice === 'glass' ? 'light' : choice === 'system' ? (media.matches ? 'dark' : 'light') : choice;
    document.documentElement.dataset.matraMaterial = choice === 'glass' ? 'glass' : 'solid';
    localStorage.setItem('matra-theme', choice);
    document.querySelectorAll('#matra-theme button').forEach(b => {
      b.classList.toggle('on', b.dataset.theme === choice);
      b.setAttribute('aria-pressed', String(b.dataset.theme === choice));
    });
  }
  apply(choice); media.addEventListener('change', () => apply(choice));
  const api = window.__TAURI__;
  if (api) {
    api.event.listen('matra-theme', e => apply(e.payload));
    api.core.invoke('get_matra_theme').then(apply).catch(console.error);
  }
  document.querySelectorAll('#matra-theme button').forEach(button => {
    button.addEventListener('click', async () => {
      try {
        const value = api ? await api.core.invoke('set_matra_theme', { theme: button.dataset.theme }) : button.dataset.theme;
        apply(value);
        document.getElementById('matra-theme-status').textContent = 'Saved. Notch and settings use this theme.';
      } catch {
        document.getElementById('matra-theme-status').textContent = 'Theme could not be saved. Try again.';
      }
    });
  });
  const side = document.getElementById('side');
  if (side) {
    const note = document.createElement('small'); note.className = 'matra-credit';
    note.textContent = 'Mātrā · Developed by Pandit Floki'; side.append(note);
  }
})();
