# Unblinking

A living desktop companion. Think tamagotchi, if your tamagotchi could see you.

Each one is a **blinki**: a small entity that lives on your screen, sees you through your camera, hears you through your mic, and reacts. It notices when you arrive, gets curious when you lean in, gets lonely when you leave, and has opinions about loud noises. Some day you'll be able to name yours.

The very first blinki was inspired by the Eye of Sauron. It has grown into its own thing since.

## Status

What ships today, on `main`, is an Electron app with one blinki: **The Burning Eye**. A perfect sphere with a fiery iris and a slit pupil that slides across its curved surface to track your face. Outside the sphere there is only a glow and rising embers. It whispers, it can speak and hum if you let it, and it lives in your tray.

Everything else on this page marked **planned** is not built yet.

- **Windows:** a floating blinki, always on top. Drag it anywhere; it remembers the spot. It lives in the system tray.
- **macOS:** it hangs from the menu bar, right under the notch, with an icon in the menu bar and no Dock icon. This mode is written but untested; expect rough edges.

A pure Rust rewrite (winit + wgpu) is in progress on the [`rust`](https://github.com/antoniwan/unblinking-desktop/tree/rust) branch. It will replace the Electron version once it can do everything this one does.

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

## What the Burning Eye does

| It notices | It does |
| --- | --- |
| Your face | The pupil narrows to a slit and follows you, aimed from wherever it sits on screen |
| You lean in close | It burns hotter. "Too close." |
| Two faces | It looks from one to the other. "Who is with you?" |
| You leave | It hunts: darts around, follows your cursor, grows restless. "Where are you?" |
| You come back | It flares. "There you are." |
| A sudden bang | It flinches, shakes, flares. "Silence." |
| You keep talking | Your voice feeds its fire. "I hear you." |

Right-click the blinki or its tray icon for the menu: hide it, **blind it** (camera and mic fully off), turn whispers off, turn its voice on, let clicks pass through, start with the computer, or banish it.

### Voice

Off by default. Turn on **Voice** in the menu and the blinki makes sound: a low drone in a cavernous room that swells when it flares, and its whispers spoken aloud by your system's speech voice. All of it is made on your machine with Web Audio and the built-in speech synthesis. With Whispers off, Voice is just the drone. It ignores the mic while it makes its own sound, so it won't startle at itself.

## How a blinki is described

Three layers, from slowest to fastest:

- **Theme**: who the blinki is. Its body, material, palette, and font.
- **Mood**: how it feels right now. Each mood draws on a school of magic. *(planned)*
- **Mode**: what it is doing, internally. `locked` on a face, `hunting` for one, or `blind`. *(built)*

The planned moods:

| Mood | School |
| --- | --- |
| Watchful | Pyromancy |
| Curious | Arcane |
| Lonely | Umbral |
| Wrathful | Storm |
| Delighted | Fae |
| Suspicious | Astral |
| Drowsy | Druidic |
| Blinded | Warding |

## Roadmap

All planned, none of it built yet:

- **More themes:** an energy sphere, a sphere array, a wisp, a third eye, a planetary entity, a techno-entity, and **Lucero**, a floating magical cat.
- **Moods** as described above, each with its own look and voice.
- **Naming** your blinki.
- **A magical pixel font** for its whispers.
- **The Rust version** reaching parity and replacing Electron.

## Privacy

Everything runs on your machine. Camera frames go to a face detector running locally in WebAssembly; the mic becomes a single loudness number. Nothing is recorded, saved, or sent anywhere. The only network call is the one-time model download in `npm run setup`.

Blind it from the menu and the camera and mic are released, not just ignored. Your settings live in a small `prefs.json` in Electron's user data folder.

## Layout

- `src/main.js`: window, tray / menu bar, menu, saved settings, permissions, cursor feed
- `src/preload.js`: the bridge between page and Electron
- `renderer/eye.js`: the Burning Eye, a single WebGL fragment shader
- `renderer/senses.js`: camera, face detection (MediaPipe), mic loudness
- `renderer/mind.js`: behavior, modes, and whispers
- `renderer/voice.js`: the drone and spoken whispers
- `renderer/app.js`: wires it together and runs the frame loop
- `scripts/`: setup (model download, icon drawing)

How the pieces talk: [docs/architecture.md](docs/architecture.md). Working on it: [CONTRIBUTING.md](CONTRIBUTING.md).

## Debug

`npx electron . --snap=8000` saves `snap.png` after 8 seconds. The terminal prints what it sees and hears every 3 seconds.

## License

MIT. See [LICENSE](LICENSE).
