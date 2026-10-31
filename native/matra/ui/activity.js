const invoke = window.__TAURI__.core.invoke;
const view = window.MatraActivity.mount(document.querySelector('#activity-root'), {invoke});
view.show();
document.querySelector('#activity-settings').addEventListener('click', () => invoke('open_settings'));
