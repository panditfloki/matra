'use strict';
const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('matra', {
  state: () => ipcRenderer.invoke('state'),
  action: (action, value) => ipcRenderer.invoke('action', action, value),
  onState: callback => ipcRenderer.on('state', (_event, data) => callback(data)),
});
// The existing dashboard uses this narrow bridge, the same messages as VS Code.
contextBridge.exposeInMainWorld('acquireVsCodeApi', () => ({
  postMessage: message => ipcRenderer.send('dashboard-message', message),
}));
ipcRenderer.on('dashboard-data', (_event, data) => window.postMessage(data, '*'));
