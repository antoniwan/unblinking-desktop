# Unblinking in Rust

A pure Rust rewrite of the Electron app, in progress. The Electron code in
the folder above still works and is untouched.

Done so far:

- The eye in a transparent, frameless, always-on-top window that stays off
  the taskbar. It sits in the top right of the work area. The fragment shader
  is the one from `renderer/eye.js`, compiled from GLSL by naga.
- `src/mind.rs`, a port of `renderer/mind.js`. With nobody in view, it hunts
  and follows the global cursor.
- Sight. A camera thread (nokhwa, 640x480 MJPEG) runs YuNet, a small ONNX
  face detector, through tract (pure Rust, on the CPU) about 15 times a
  second. When it finds you, the eye locks on. The model is built into the
  exe, and frames stay in memory only.
- Hearing. The default mic, reduced to one loudness number (cpal), feeds the
  flames and makes the eye flinch at a bang.
- The wake word (opt-in, off by default). Tick "Wake on 'Hey, OG Blinki'"
  in the tray, then say "Hey, OG Blinki" or "Hey Blinki": the eye appears
  if hidden, looks at you, and answers. It uses Windows' own speech
  recognizer with a fixed list of phrases, which runs on the computer with
  the installed speech language pack: no cloud, no account, no key, nothing
  recorded. Blinding the eye stops it too. It needs a speech language
  installed (Settings > Time & language > Speech, and Language & region)
  and the mic allowed for desktop apps (Settings > Privacy & security >
  Microphone). If it can't start, it unticks itself and says "I can't hear
  names on this computer yet."
- The tray icon and its menu, which also opens when you right-click the eye:
  show or hide, blind (camera and mic really stop), whispers, voice (saved
  only for now), click-through, and banish.
- Prefs saved in Electron's own file and keys (`%APPDATA%\Unblinking\prefs.json`),
  including where you last dropped the eye.
- Voice and whispers. The drone from voice.js is synthesised in Rust inside
  the audio callback (detuned saws, a sub, brown-noise hiss, a breathing
  filter). Whispers are spoken by Windows' deepest local voice. They're also
  written under the eye in Tiny5, letter by letter, with an entrance from the
  mood's school of magic: Pyromancy, Arcane, Umbral, Storm, Fae, Druidic,
  Astral or Warding.
- Blinki's Senses, a window that shows what the eye perceives. Open it from
  the tray or with Ctrl+Alt+B.
- "Where's my camera?" (in the tray): your monitors as they're laid out.
  Drag the camera to where your webcam sits, and the eye aims from there.
  "Look at me" fine-tunes it.
- One copy at a time: launching it again wakes the eye. "Start with the
  computer" in the tray adds it to Windows' per-user startup list.
- Opening and closing. The eye eases open over 0.9 s, and fades out over
  0.6 s before it hides or quits. The window only shows after a transparent
  first frame, so there's no black box.

## Run it

```bash
cargo run --release
```

Drag the eye with the left mouse button. Right-click it, or the tray icon, for
the menu. A left click on the tray icon shows or hides it.

| Flag | What it does |
| --- | --- |
| `--snap=MS` | After MS milliseconds, saves the frame over a dark backdrop to `snap.png` in the current folder, then quits. |
| `--backend=dx12` or `--backend=vulkan` | Uses only that backend. By default it tries DX12 first, then Vulkan. |
| `--camera` | Opens "Where's my camera?" at start. |
| `--senses` | Opens Blinki's Senses at start. Ctrl+Alt+B or the tray also open it. |
| `--mood=NAME` | Holds one of the studio's moods. |
| `--whisper=TEXT` | Shows TEXT as a whisper once the eye opens. |
| `--voice` | Turns the drone and spoken whispers on for this run only. |
| `--bench-face` | Times the face model on a blank 1280x720 frame and quits. |
| `--blind` | Starts blinded for this run only (camera and mic off), without saving it. |

```bash
cargo test
```

Two tests need the webcam, and one of them needs a face in front of it, so
they only run when asked:

```bash
cargo test --release -- --ignored --nocapture
```

## Build the installer

The installer is an NSIS setup .exe built by
[cargo-packager](https://github.com/crabnebula-dev/cargo-packager). It
installs for the current user, with a Start menu shortcut and an
uninstaller. Install the tool once, then build:

```bash
cargo install cargo-packager --locked
```

```bash
cargo packager --release
```

The setup .exe lands in `target/packager/`. The first run downloads NSIS
itself.

## Size and memory, against Electron

Measured on this PC (Windows 11, RTX 3060) on 2026-10-28, both apps idle and
blinded (camera and mic off), 13 to 15 seconds after launch.

| | Electron | Rust |
| --- | --- | --- |
| What ships | 356.8 MB Electron runtime + 0.5 MB of app | one 22.3 MB exe (model, font and moods inside) |
| Processes | 5 | 1 |
| Working set | 448.7 MB | 134.9 MB |
| Private memory | 231.4 MB | 147.9 MB |

The Rust exe was 36.5 MB with full optimisation everywhere. It's 22.3 MB
with size-optimised code except in the face model's runtime, which still
looks at a 1280x720 frame in 23.6 ms (`--bench-face`). Asking wgpu for small
GPU memory blocks (`MemoryHints::MemoryUsage`) saved about 180 MB of
private memory.

## How the window is transparent

wgpu's DX12 backend is set to `Dx12SwapchainKind::DxgiFromVisual`, which is a
DirectComposition swapchain, with `CompositeAlphaMode::PreMultiplied`. The
window is created without a redirection bitmap. A plain HWND swapchain can't
be transparent. On an RTX 3060, Vulkan also offers `PreMultiplied` and works
as a fallback.

## Files

| File | What it holds |
| --- | --- |
| `src/main.rs` | The window, the frame loop, the flags, and the snapshot |
| `src/eye.rs` | The wgpu pipeline and the frame readback |
| `src/shaders/eye.frag` | The eye shader, carried over from `eye.js` |
| `src/mind.rs` | The mind's modes, moods and whispers, plus their tests |
| `src/desktop.rs` | The cursor position and work area, read from Windows |
| `src/senses/camera.rs` | The camera thread and the channel that brings its reports back |
| `src/senses/mic.rs` | The mic stream and its loudness, shared through an atomic |
| `src/senses/wake.rs` | The wake word: Windows' speech recognizer on its own thread, and the phrase matcher |
| `src/prefs.rs` | Loading and saving prefs.json |
| `src/tray.rs` | The tray icon, the menu, and the right-click popup |
| `src/senses/face.rs` | YuNet through tract, and the box decoding and NMS that MediaPipe used to do |
| `assets/` | The face model and its license note |
