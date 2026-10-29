# Contributing

## Set up

Needs [Rust](https://rustup.rs) (stable, MSVC) on Windows. Everything else
(the face models, the font, the moods) is in `assets/` and built into the
exe, so there's nothing to fetch after a fresh clone.

## Run

```
cargo run --release
```

The terminal shows a heartbeat every 3 seconds: mode, mood, faces, face
position and size, mic level, gaze, and frame rate. Mode changes and whispers
are logged too. All the flags are in [docs/development.md](docs/development.md#flags).

## Debug snapshot

```
cargo run --release -- --snap=8000
```

Saves `snap.png` in the current folder after 8 seconds, on a dark backdrop so
the flames read as they would on a desktop. `snap.png` is gitignored.

## Before you push

```
cargo +1.99.0 fmt --check
cargo +1.99.0 clippy --all-targets -- -D warnings
cargo test
```

CI pins Rust 1.99.0 for formatting and clippy; see
[docs/development.md](docs/development.md#format-and-clippy).

## Who owns what

Keep each change inside the folder that owns it.

| Folder | Owns |
| --- | --- |
| `src/app/` | The `App`, winit's callbacks, the frame loop and its pacing |
| `src/render/` | The window, the GPU, the eye shader, whispers and their font. Everything you see. |
| `src/mind/` | Behaviour, motion, feeling, moods and themes |
| `src/bond/` | Remembrance and the wellbeing watch |
| `src/senses/` | Camera, face detection, expression, mic loudness, the wake word, camera placement |
| `src/audio/` | The drone, effects, spoken whispers, and the mixer |
| `src/platform/` | What the app asks of Windows: cursor, full screen, single instance, the tray |
| `src/ui/` | Blinki's Senses and "Where's my camera?" |
| `src/tools/` | `--demo`, `--record` and `--snap` |
| `src/prefs.rs` | Prefs on disk |
| `src/cli.rs` | The command line flags |

How they connect, with one line per module: [docs/architecture.md](docs/architecture.md).

Rules worth keeping:

- Nothing leaves the machine. Don't add network calls, analytics, or anything that stores frames or audio.
- A new setting is a field in `src/prefs.rs` with a default and a tray menu item, like the others.
- Test on Windows.

## Commits

Short, imperative subject lines that say what changed: `Add voice toggle to tray menu`, `Fix gaze when the eye is on a second monitor`. One change per commit. Add a body when the why isn't obvious.
