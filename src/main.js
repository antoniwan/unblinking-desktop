// Unblinking: main process.
// Owns the always-on-top transparent window, the tray / menu bar icon,
// camera and microphone permissions, and feeds the renderer where the
// cursor and the eye are on screen.
const {
  app,
  BrowserWindow,
  Tray,
  Menu,
  nativeImage,
  screen,
  session,
  protocol,
  net,
  ipcMain,
  systemPreferences,
} = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const { pathToFileURL } = require('node:url');

const IS_MAC = process.platform === 'darwin';
const ROOT = path.join(__dirname, '..');
const ICONS = path.join(ROOT, 'assets', 'icons');
const SNAP = process.argv.find((a) => a.startsWith('--snap='));
const WIN_SIZE = IS_MAC ? { width: 300, height: 260 } : { width: 320, height: 320 };

protocol.registerSchemesAsPrivileged([
  {
    scheme: 'app',
    privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: true },
  },
]);

if (!app.requestSingleInstanceLock()) app.quit();

let win = null;
let tray = null;
let feedTimer = null;
const prefs = loadPrefs();

function prefsPath() {
  return path.join(app.getPath('userData'), 'prefs.json');
}

function loadPrefs() {
  const defaults = { whispers: true, blind: false, clickThrough: false, position: null };
  try {
    return { ...defaults, ...JSON.parse(fs.readFileSync(prefsPath(), 'utf8')) };
  } catch {
    return defaults;
  }
}

function savePrefs() {
  try {
    fs.mkdirSync(path.dirname(prefsPath()), { recursive: true });
    fs.writeFileSync(prefsPath(), JSON.stringify(prefs, null, 2));
  } catch (err) {
    console.error('could not save prefs:', err.message);
  }
}

// Serves the project folder as app://unblinking/... so the renderer gets a
// secure origin (camera, mic, WebAssembly) without a web server.
function registerAppProtocol() {
  protocol.handle('app', (request) => {
    const { pathname } = new URL(request.url);
    const filePath = path.normalize(path.join(ROOT, decodeURIComponent(pathname)));
    if (!filePath.startsWith(ROOT)) return new Response('forbidden', { status: 403 });
    return net.fetch(pathToFileURL(filePath).toString());
  });
}

function allowCameraAndMic() {
  const ses = session.defaultSession;
  ses.setPermissionRequestHandler((_wc, permission, callback) => {
    callback(permission === 'media');
  });
  ses.setPermissionCheckHandler((_wc, permission) => permission === 'media');
}

async function askMacMediaAccess() {
  if (!IS_MAC) return;
  for (const kind of ['camera', 'microphone']) {
    if (systemPreferences.getMediaAccessStatus(kind) !== 'granted') {
      await systemPreferences.askForMediaAccess(kind);
    }
  }
}

// Mac: hangs from the top center, right under the menu bar and the notch.
// Windows: top right of the work area, or wherever it was last dragged.
function startPosition() {
  const display = screen.getPrimaryDisplay();
  if (IS_MAC) {
    const { x, width } = display.bounds;
    return { x: Math.round(x + width / 2 - WIN_SIZE.width / 2), y: display.workArea.y };
  }
  if (prefs.position) {
    const visible = screen.getAllDisplays().some(({ workArea: a }) =>
      prefs.position.x >= a.x - 100 &&
      prefs.position.x < a.x + a.width - 100 &&
      prefs.position.y >= a.y - 100 &&
      prefs.position.y < a.y + a.height - 100
    );
    if (visible) return prefs.position;
  }
  const a = display.workArea;
  return { x: a.x + a.width - WIN_SIZE.width - 24, y: a.y + 24 };
}

function createWindow() {
  const pos = startPosition();
  win = new BrowserWindow({
    ...WIN_SIZE,
    ...pos,
    transparent: true,
    frame: false,
    resizable: false,
    maximizable: false,
    fullscreenable: false,
    hasShadow: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    show: false,
    backgroundColor: '#00000000',
    title: 'Unblinking',
    icon: path.join(ICONS, 'icon.png'),
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      backgroundThrottling: false,
    },
  });

  win.setAlwaysOnTop(true, 'screen-saver');
  if (IS_MAC) win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  applyClickThrough();

  win.loadURL('app://unblinking/renderer/index.html');
  win.once('ready-to-show', () => {
    win.showInactive();
    sendPrefs();
  });

  // Right-click on the eye (a drag region on Windows) opens our menu.
  win.on('system-context-menu', (event) => {
    event.preventDefault();
    buildMenu().popup({ window: win });
  });

  let moveTimer = null;
  win.on('moved', () => {
    clearTimeout(moveTimer);
    moveTimer = setTimeout(() => {
      if (!win || IS_MAC) return;
      const [x, y] = win.getPosition();
      prefs.position = { x, y };
      savePrefs();
    }, 400);
  });

  win.webContents.on('console-message', (event) => {
    const { level, message } = event;
    console.log(`[renderer:${level}] ${message}`);
  });

  win.on('closed', () => {
    win = null;
    clearInterval(feedTimer);
  });

  // Tell the renderer where the eye and the cursor are, about 30 times a second.
  feedTimer = setInterval(() => {
    if (!win || win.isDestroyed() || !win.isVisible()) return;
    const b = win.getBounds();
    const eye = { x: b.x + b.width / 2, y: b.y + b.height * 0.42 };
    const display = screen.getDisplayNearestPoint({ x: Math.round(eye.x), y: Math.round(eye.y) });
    win.webContents.send('world', {
      cursor: screen.getCursorScreenPoint(),
      eye,
      display: display.bounds,
    });
  }, 33);

  if (SNAP) {
    const delay = Number(SNAP.split('=')[1]) || 6000;
    setTimeout(async () => {
      // A dark backdrop, only for the snapshot, so the flames are judged as on a desktop.
      await win.webContents.executeJavaScript("document.body.style.background = '#1d1f24'");
      await new Promise((r) => setTimeout(r, 100));
      const image = await win.webContents.capturePage();
      const out = path.join(ROOT, 'snap.png');
      fs.writeFileSync(out, image.toPNG());
      console.log('snapshot saved:', out);
    }, delay);
  }
}

function applyClickThrough() {
  if (!win) return;
  win.setIgnoreMouseEvents(prefs.clickThrough, { forward: true });
}

function sendPrefs() {
  if (win && !win.isDestroyed()) {
    win.webContents.send('prefs', { whispers: prefs.whispers, blind: prefs.blind });
  }
}

function toggleWindow() {
  if (!win) return createWindow();
  if (win.isVisible()) win.hide();
  else win.showInactive();
}

function buildMenu() {
  return Menu.buildFromTemplate([
    { label: prefs.blind ? 'Unblinking (blinded)' : 'Unblinking is watching', enabled: false },
    { type: 'separator' },
    { label: win && win.isVisible() ? 'Hide the eye' : 'Show the eye', click: toggleWindow },
    {
      label: 'Blind it (camera and mic off)',
      type: 'checkbox',
      checked: prefs.blind,
      click: (item) => {
        prefs.blind = item.checked;
        savePrefs();
        sendPrefs();
        refreshTray();
      },
    },
    {
      label: 'Whispers',
      type: 'checkbox',
      checked: prefs.whispers,
      click: (item) => {
        prefs.whispers = item.checked;
        savePrefs();
        sendPrefs();
      },
    },
    {
      label: 'Let clicks pass through',
      type: 'checkbox',
      checked: prefs.clickThrough,
      click: (item) => {
        prefs.clickThrough = item.checked;
        savePrefs();
        applyClickThrough();
      },
    },
    {
      label: 'Start with the computer',
      type: 'checkbox',
      checked: app.getLoginItemSettings().openAtLogin,
      click: (item) => app.setLoginItemSettings({ openAtLogin: item.checked }),
    },
    { type: 'separator' },
    { label: 'Banish (quit)', role: 'quit' },
  ]);
}

function refreshTray() {
  if (!tray) return;
  tray.setToolTip(prefs.blind ? 'Unblinking: blinded' : 'Unblinking: watching');
  if (IS_MAC) tray.setContextMenu(buildMenu());
}

function createTray() {
  const image = IS_MAC
    ? nativeImage.createFromPath(path.join(ICONS, 'trayTemplate.png'))
    : nativeImage.createFromPath(path.join(ICONS, 'tray.png')).resize({ width: 16, height: 16 });
  if (IS_MAC) image.setTemplateImage(true);
  tray = new Tray(image);
  refreshTray();
  if (IS_MAC) {
    // Rebuild on open so the checkboxes are current.
    tray.on('mouse-down', () => tray.setContextMenu(buildMenu()));
  } else {
    tray.on('click', toggleWindow);
    tray.on('right-click', () => tray.popUpContextMenu(buildMenu()));
  }
}

ipcMain.on('menu', () => {
  if (win) buildMenu().popup({ window: win });
});

app.on('second-instance', () => {
  if (win) win.showInactive();
});

app.whenReady().then(async () => {
  if (IS_MAC && app.dock) app.dock.hide();
  registerAppProtocol();
  allowCameraAndMic();
  await askMacMediaAccess();
  createTray();
  createWindow();
});

// The eye lives in the tray; closing the window does not end it.
app.on('window-all-closed', (event) => event.preventDefault());
