# Contributing

## Set up

```
npm install
npm run setup
```

`npm run setup` fetches the face model into `assets/models/` and draws the icons into `assets/icons/`. Both folders are gitignored, so run it after every fresh clone.

## Run

```
npm start
```

The terminal shows the renderer's console, including a heartbeat every 3 seconds: mode, faces, face position and size, and mic level. Whispers are logged too.

## Debug snapshot

```
npx electron . --snap=8000
```

Saves `snap.png` in the project folder after 8 seconds, on a dark backdrop so the flames read as they would on a desktop. `snap.png` is gitignored.

## Who owns what

Keep each change inside the module that owns it.

| File | Owns |
| --- | --- |
| `src/main.js` | Window, tray / menu bar, the menu, prefs on disk, permissions, the `world` feed |
| `src/preload.js` | The bridge (`window.unblinking`). Keep it small. |
| `renderer/app.js` | Wiring and the frame loop |
| `renderer/senses.js` | Camera, face detection, mic loudness |
| `renderer/mind.js` | Behavior, moods, whisper lines |
| `renderer/eye.js` | The shader. Everything you see. |
| `renderer/voice.js` | Drone and spoken whispers |
| `scripts/` | Setup: model download, icon drawing |

How they connect: [docs/architecture.md](docs/architecture.md).

Rules worth keeping:

- Nothing leaves the machine. Don't add network calls, analytics, or anything that stores frames or audio.
- A new setting is a pref in `main.js` with a menu item, sent to the renderer like the others.
- Test on Windows. If you can test the macOS menu-bar mode, say so in your PR; nobody has yet.

## Commits

Short, imperative subject lines that say what changed: `Add voice toggle to tray menu`, `Fix gaze when the eye is on a second monitor`. One change per commit. Add a body when the why isn't obvious.
