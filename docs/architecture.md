# Architecture

Unblinking is one Electron app with two processes. The main process owns the operating system side: window, tray, menu, settings, permissions. The renderer is one page that sees, hears, decides, and draws.

## Processes

```
main process (src/main.js)
  window, tray / menu bar, menu, prefs.json, permissions, app:// protocol
      |  'world'  ~30/s: cursor, eye position, display bounds
      |  'prefs'  on start and on every menu change
      v
preload (src/preload.js)  ->  window.unblinking
      ^
      |  'menu'   right-click on the eye opens the menu
renderer (renderer/index.html, app.js)
  senses -> mind -> eye
                 -> voice
```

The preload exposes four things on `window.unblinking`: `onWorld`, `onPrefs`, `openMenu`, and `platform`. The renderer has no Node access (`contextIsolation` on, `nodeIntegration` off). Everything it knows about the desktop comes through those calls.

The page loads from `app://unblinking/...`, a custom protocol that serves the project folder. That gives the renderer a secure origin for the camera, mic, and WebAssembly without a web server. Requests outside the project folder get a 403.

## Main process

- **Window.** Transparent, frameless, always on top, no taskbar entry. On Windows it starts top right, or where it was last dragged; the position is saved 400 ms after a move. On macOS it hangs top center under the menu bar and is on every Space. (The macOS mode is untested.)
- **Tray.** A tray icon on Windows (click to show or hide, right-click for the menu), a template icon in the menu bar on macOS. The Dock icon is hidden on macOS.
- **Menu.** Built fresh each time it opens so the checkboxes are current. Shared by the tray and by right-clicking the eye.
- **World feed.** Every 33 ms, while the window is visible, it sends the cursor position, the eye's spot on screen, and the bounds of the display the eye is on.
- **Permissions.** Only `media` (camera and mic) is granted. On macOS it asks the system for camera and mic access at launch.
- **Single instance.** A second launch shows the existing eye (or reopens it) and exits.
- **Closing.** Closing the window does not quit. Only **Banish** does.

## Prefs

Saved as JSON in `prefs.json` under Electron's `userData` folder. Loaded once at start, key by key: anything missing or of the wrong type falls back to its default.

| Pref | Default | Effect |
| --- | --- | --- |
| `whispers` | on | Whisper text appears under the eye |
| `voice` | off | Drone and spoken whispers |
| `blind` | off | Camera and mic closed; the eye dims to embers |
| `clickThrough` | off | Mouse clicks pass through the window |
| `position` | none | Last dragged spot (Windows only) |

A menu click updates the pref, saves the file, and sends the renderer the prefs it needs (`whispers`, `blind`, `voice`). "Start with the computer" is not in the file; it reads and writes the OS login item directly.

## Renderer

`app.js` builds the parts and runs the loop.

- **Senses** (`senses.js`). Opens the camera and mic with `getUserMedia`. Faces come from the MediaPipe BlazeFace short-range detector, run in WebAssembly (GPU delegate, falling back to CPU). Each face is reduced to `{ x, y, size, score }`. The mic goes through a Web Audio analyser and becomes one loudness number from 0 to 1. Each sense can fail on its own (no camera, mic denied, model missing); the eye keeps running without it. If the model is missing, the camera is closed again rather than left on.
- **Mind** (`mind.js`). A small state machine with three modes: `locked` (a face is in view), `hunting` (no face; it darts, follows the cursor, grows restless), and `blind`. Each frame it takes the faces, the loudness, the world feed, and the prefs, and returns what the eye should look like: gaze, pupil, heat, flare, how wide it is open, dim, and shake. It also picks whisper lines, with cooldowns so it doesn't nag.
- **Eye** (`eye.js`). One full-window quad and one WebGL fragment shader. Everything visible is computed per pixel from the uniforms the mind drives.
- **Voice** (`voice.js`). Silent unless the `voice` pref is on. A low Web Audio drone (detuned oscillators and filtered noise, through a generated cave reverb and a final low-pass) that swells with the mind's heat and flare, and whispers read aloud with `speechSynthesis` in a low, slow voice. `speechSynthesis` can't be routed through Web Audio, so the reverb is on the drone only; while the eye speaks, the dry drone drops away and the reverb tail keeps ringing. A line is skipped if it is still speaking. The mic can hear the speakers, so while `voice.masking` is true (speaking, or the drone loud or swelling) `app.js` hands the mind the last mic level it trusted instead of the live one, and the eye never startles at itself. Turning Voice off fades the drone out and suspends its audio context.

### Start-up

The camera and mic are not opened until the renderer knows whether the eye is blind: on the first `prefs` message, or after 1.5 seconds if none arrives. Turning blind on closes both streams; turning it off opens them again. Changes are queued, so quick toggling can't leave a stream open. The streams are also closed when the page goes away.

### The frame loop

One `requestAnimationFrame` loop in `app.js`:

1. Run face detection, at most about 15 times a second (every 66 ms), and only when the camera has a new frame.
2. Read the mic loudness, every frame.
3. `mind.update(dt, ...)` turns faces, loudness, world, and prefs into an eye state. `dt` is capped at 0.1 s so a stall doesn't make it jump.
4. `eye.render(...)` draws it.

Whispers flow out of the mind through a `say(text, kind)` callback, where `kind` is the line group in `mind.js` (`returned`, `loud`, and so on). `app.js` shows the text under the eye for about 3 seconds and logs it. With Voice on, the same line is spoken. The mind sends nothing when Whispers is off, so Voice then means the drone alone.

Every 3 seconds the renderer logs a heartbeat (mode, face count, the nearest face, mic level). The main process copies renderer console output to the terminal.

## Privacy boundaries

- **No network from the app.** The page's Content Security Policy limits scripts, fetches, and media to the app's own origin. The detector's WebAssembly comes from `node_modules`, and the model from `assets/models/`. The only download is `scripts/fetch-model.mjs`, run by `npm run setup`.
- **Frames stay in memory.** Video frames go to the detector and are dropped. Only face boxes are kept, and only the latest set.
- **Audio becomes one number.** The analyser reads a short buffer each frame and keeps one loudness value. Nothing is recorded.
- **Voice is output only.** The drone is synthesized in Web Audio; no audio files are loaded. Speech uses only voices the system marks as local; if only remote voices exist, it stays silent.
- **Blind means off.** Blinding stops the camera and mic tracks and closes the audio context, so the OS camera light goes out.
- **Only settings are saved.** `prefs.json` holds menu choices and the window position. `snap.png` is written only when you pass `--snap`.
