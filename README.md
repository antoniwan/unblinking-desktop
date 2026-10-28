# Unblinking

A burning eye that lives on your desktop and watches you through your camera and microphone.

- **Windows:** a floating eye, always on top. Drag it anywhere; it remembers the spot. It lives in the system tray.
- **macOS:** it hangs from the menu bar, right under the notch, with an eye icon in the menu bar. No Dock icon.

## Run it

```
npm install
npm run setup   # downloads the face model (~230 KB, once) and draws the icons
npm start
```

`npm run setup` has already been run in this folder.

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

Right-click the eye or the tray icon for the menu: hide it, **blind it** (camera and mic fully off), turn whispers off, let clicks pass through, start with the computer, or banish it.

## Privacy

Everything runs on this machine. Camera frames go to a face detector running locally in WebAssembly; the mic becomes a single loudness number. Nothing is recorded, saved, or sent anywhere. The only network call is the one-time model download in `npm run setup`.

## Layout

- `src/main.js`: window, tray / menu bar, permissions, cursor feed
- `src/preload.js`: the bridge between page and Electron
- `renderer/eye.js`: the eye, a single WebGL fragment shader
- `renderer/senses.js`: camera, face detection (MediaPipe), mic loudness
- `renderer/mind.js`: behavior and whispers
- `renderer/app.js`: wires it together

## Debug

`npx electron . --snap=8000` saves `snap.png` after 8 seconds. The terminal prints what it sees and hears every 3 seconds.
