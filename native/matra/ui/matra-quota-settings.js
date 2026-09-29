/* Persist once, then broadcast to both windows. Failed writes keep saved state. */
(() => {
  const api = window.__TAURI__;
  const watch = document.getElementById('quota-watch');
  const critical = document.getElementById('quota-critical');
  if (!watch || !critical) return;
  const mode = document.querySelectorAll('#quota-mode button');
  const reset = document.getElementById('quota-reset');
  const status = document.getElementById('quota-status');
  const controls = [watch, critical, reset, ...mode];
  let saved = { ...MatraQuota.defaults };
  function apply(value) {
    saved = MatraQuota.normalize(value);
    watch.value = Math.round(saved.watch * 100);
    critical.value = Math.round(saved.critical * 100);
    watch.max = Math.round(saved.critical * 100) - 1;
    critical.min = Math.round(saved.watch * 100) + 1;
    document.getElementById('quota-watch-value').textContent = `${watch.value}%`;
    document.getElementById('quota-critical-value').textContent = `${critical.value}%`;
    for (const button of mode) {
      button.classList.toggle('on', button.dataset.mode === saved.mode);
      button.setAttribute('aria-pressed', String(button.dataset.mode === saved.mode));
    }
  }
  async function save(value) {
    controls.forEach(control => control.disabled = true);
    try {
      if (!api) throw new Error('Native settings unavailable');
      apply(await api.core.invoke('set_quota_colors', { colors: value }));
      status.textContent = 'Saved. Usage rings and bars now use these thresholds. App accent is unchanged.';
    } catch {
      apply(saved);
      status.textContent = 'Could not save quota colours. Your previous settings are unchanged.';
    } finally { controls.forEach(control => control.disabled = false); }
  }
  apply(saved);
  // Register before loading so an update in the other window cannot be missed.
  if (api) {
    let revision = 0;
    api.event.listen('matra-quota-colors', event => { revision++; apply(event.payload); }).then(() => {
      const before = revision;
      return api.core.invoke('get_quota_colors').then(value => { if (before === revision) apply(value); });
    }).catch(() => { status.textContent = 'Could not load saved quota colours. Reopen Settings to retry.'; });
  }
  for (const [input, output] of [[watch, 'quota-watch-value'], [critical, 'quota-critical-value']]) {
    input.addEventListener('input', () => { document.getElementById(output).textContent = `${input.value}%`; });
  }
  watch.addEventListener('change', () => save({ ...saved, watch: Number(watch.value) / 100 }));
  critical.addEventListener('change', () => save({ ...saved, critical: Number(critical.value) / 100 }));
  mode.forEach(button => button.addEventListener('click', () => save({ ...saved, mode: button.dataset.mode })));
  reset.addEventListener('click', () => save({ ...MatraQuota.defaults }));
})();
