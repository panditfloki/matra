'use strict';
(() => {
  const vscode = acquireVsCodeApi();
  let snapshot = { providers: [] }, mode = 'remaining', refreshing = true;
  let expanded = vscode.getState()?.expanded || [];
  const root = document.getElementById('root');
  function draw() {
    const focused = document.activeElement;
    const focusKey = focused?.dataset?.action ? { action: focused.dataset.action, mode: focused.dataset.mode, provider: focused.dataset.provider } : null;
    root.innerHTML = MatraView.render(snapshot, { mode, expanded, refreshing });
    if (focusKey) Array.from(root.querySelectorAll('button')).find(b => b.dataset.action === focusKey.action && b.dataset.mode === focusKey.mode && b.dataset.provider === focusKey.provider)?.focus();
  }
  document.addEventListener('click', event => {
    const button = event.target.closest('button[data-action]');
    if (!button || button.disabled) return;
    if (button.dataset.action === 'refresh') { refreshing = true; draw(); }
    vscode.postMessage({ type: button.dataset.action, mode: button.dataset.mode, provider: button.dataset.provider });
  });
  document.addEventListener('toggle', event => {
    if (event.target.tagName !== 'DETAILS') return;
    expanded = Array.from(root.querySelectorAll('details[open]')).map(d => d.dataset.provider);
    vscode.setState({ expanded });
  }, true);
  window.addEventListener('message', event => {
    const msg = event.data;
    if (msg?.type !== 'snapshot') return;
    snapshot = msg.snapshot; mode = msg.mode; refreshing = !!msg.refreshing; draw();
  });
  setInterval(draw, 30000);
  vscode.postMessage({ type: 'ready' });
})();
