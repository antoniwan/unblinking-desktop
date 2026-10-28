// The only bridge between the page and Electron.
const { contextBridge, ipcRenderer } = require('electron');

// Subscribe to a channel; returns a function that unsubscribes.
function listen(channel, fn) {
  const handler = (_e, data) => fn(data);
  ipcRenderer.on(channel, handler);
  return () => ipcRenderer.removeListener(channel, handler);
}

contextBridge.exposeInMainWorld('unblinking', {
  // { cursor, eye, display } in screen pixels, about 30 times a second.
  onWorld: (fn) => listen('world', fn),
  // { whispers, blind, voice } whenever one changes, and once at start.
  onPrefs: (fn) => listen('prefs', fn),
  openMenu: () => ipcRenderer.send('menu'),
  platform: process.platform,
});
