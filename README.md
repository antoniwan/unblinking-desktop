# Unblinking

A burning eye that lives on your desktop and watches you through your camera and microphone.

- **Windows:** a floating eye, always on top. Drag it anywhere; it remembers the spot. It lives in the system tray.
- **macOS:** it hangs from the menu bar, right under the notch, with an eye icon in the menu bar. No Dock icon. This mode is written but untested; expect rough edges.

## Run it

Needs Node.js and npm.

```
git clone https://github.com/antoniwan/unblinking-desktop.git
cd unblinking-desktop
npm install
npm run setup   # downloads the face model (~230 KB, once) and draws the icons
npm start
```

Run `npm run setup` after every fresh clone. The model (`assets/models/`) and the icons (`assets/icons/`) are not in the repo.

## What it does

| It notices | It does |
| --- | --- |
| Your face | The pupil narrows to a slit and follows you, aimed from wherever the eye sits on screen |
| You lean in close | It burns hotter. "Too close." |
| Two faces | It looks from one to the other. "Who is with you?" |
| You leave | It hunts: darts around, follows your cursor, grows restless. "Where are you?" |
| You come back | It flares. "There you are." |
| A sudden bang | It flinches, shakes, flares. "Silence." |
| You keep talking | The flames feed on your voice. "I hear you." |

Right-click the eye or the tray icon for the menu: hide it, **blind it** (camera and mic fully off), turn whispers off, turn its voice on, let clicks pass through, start with the computer, or banish it.

## Voice

Off by default. Turn on **Voice** in the menu and the eye makes sound: a low drone that swells when it flares, and its whispers spoken aloud by your system's speech voice. The sound is made on this machine with Web Audio and the built-in speech synthesis. With Whispers off, Voice is just the drone.

## Privacy

Everything runs on this machine. Camera frames go to a face detector running locally in WebAssembly; the mic becomes a single loudness number. Nothing is recorded, saved, or sent anywhere. The only network call is the one-time model download in `npm run setup`.

Blind it from the menu and the camera and mic are released, not just ignored. Your settings live in a small `prefs.json` in Electron's user data folder.

## Layout

- `src/main.js`: window, tray / menu bar, menu, saved settings, permissions, cursor feed
- `src/preload.js`: the bridge between page and Electron
- `renderer/eye.js`: the eye, a single WebGL fragment shader
- `renderer/senses.js`: camera, face detection (MediaPipe), mic loudness
- `renderer/mind.js`: behavior and whispers
- `renderer/voice.js`: the drone and spoken whispers
- `renderer/app.js`: wires it together and runs the frame loop
- `scripts/`: setup (model download, icon drawing)

How the pieces talk: [docs/architecture.md](docs/architecture.md). Working on it: [CONTRIBUTING.md](CONTRIBUTING.md).

## Debug

`npx electron . --snap=8000` saves `snap.png` after 8 seconds. The terminal prints what it sees and hears every 3 seconds.

## License

None chosen yet, so all rights are reserved for now.
