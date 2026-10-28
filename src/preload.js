// The only bridge between the page and Electron.
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('unblinking', {
  onWorld: (fn) => ipcRenderer.on('world', (_e, data) => fn(data)),
  onPrefs: (fn) => ipcRenderer.on('prefs', (_e, data) => fn(data)),
  openMenu: () => ipcRenderer.send('menu'),
  platform: process.platform,
});
