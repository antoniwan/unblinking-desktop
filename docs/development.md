# Development

How to build, run, test and package Unblinking. All commands run from the
repo root. How the code fits together, with a map of every module, is in
[architecture.md](architecture.md).

## Set up

Needs [Rust](https://rustup.rs) (stable, MSVC) on Windows. There's nothing
else to fetch: the face models, the font and the moods are in `assets/` and
are built into the exe.

## Run it

```bash
cargo run --release
```

Drag the eye with the left mouse button. Right-click it, or the tray icon, for
the menu. A left click on the tray icon shows or hides it.

The terminal shows a heartbeat every 3 seconds (mode, mood, faces, mic level,
gaze, and `fps=`), and a line whenever the mode changes or a whisper is said.

## Flags

| Flag | What it does |
| --- | --- |
| `--snap=MS` | After MS milliseconds, saves the frame over a dark backdrop to `snap.png` in the current folder, then quits. |
| `--backend=dx12` or `--backend=vulkan` | Uses only that backend. By default it tries DX12 first, then Vulkan. |
| `--camera` | Opens "Where's my camera?" at start. |
| `--senses` | Opens Blinki's Senses at start. Ctrl+Alt+B or the tray also open it. |
| `--mood=NAME` | Holds one of the studio's moods. |
| `--whisper=TEXT` | Shows TEXT as a whisper once the eye opens. |
| `--voice` | Turns all sound (music, voice, effects) on for this run only. |
| `--demo` | Plays a scripted 14 s performance (moods, whispers, a flinch) with no camera or mic, then quits. |
| `--record=FILE.gif` | Saves the frames it draws (eye and whispers) as an animated GIF. Sound is off while recording. |
| `--bench-face` | Times the face model on a blank 1280x720 frame and quits. |
| `--blind` | Starts blinded for this run only (camera and mic off), without saving it. |

The flags are parsed in `src/cli.rs`. `--demo`, `--record` and `--snap` live
in `src/tools/`.

## Tuning the shader

The eye's shader is four layers in `src/render/shaders`: `common.glsl`
(uniforms, noise, the mood's fire ramp), `body.glsl` (the sphere, raymarched,
with the fire inside it), `aura.glsl` (only light outside it: glows and
embers) and `eye.frag` (lays the body over the aura). They're built into the
exe, but with `UNBLINKING_SHADERS` set to a folder they're read from there,
so a shader can be tuned by rerunning with `--snap` instead of rebuilding:

```bash
UNBLINKING_SHADERS=src/render/shaders cargo run --release -- --snap=4000
```

## Test

```bash
cargo test
```

Three tests only run when asked: two need the webcam (one of them also
needs a face in front of it), and one times the expression model, which only
means something in a release build:

```bash
cargo test --release -- --ignored --nocapture
```

## Format and clippy

CI (`.github/workflows/ci.yml`) runs on Windows with Rust pinned to 1.99.0,
so a new Rust release can't add clippy lints and turn it red overnight. To
match it locally:

```bash
rustup toolchain install 1.99.0 --component rustfmt clippy
cargo +1.99.0 fmt --check
cargo +1.99.0 clippy --all-targets -- -D warnings
```

CI then runs `cargo test` and `cargo build --release`, and uploads
`unblinking.exe` as an artifact named for the version in `Cargo.toml`. To
move to a newer Rust, bump the pin in `ci.yml` on purpose and fix the new
lints in the same commit.

## Build the installer

The installer is an NSIS setup .exe built by
[cargo-packager](https://github.com/crabnebula-dev/cargo-packager). It
installs for the current user, with a Start menu shortcut and an
uninstaller. Its settings are under `[package.metadata.packager]` in
`Cargo.toml`. Install the tool once, then build:

```bash
cargo install cargo-packager --locked
```

```bash
cargo packager --release
```

The setup .exe lands in `target/packager/`. The first run downloads NSIS
itself.

## The wake word on your machine

The wake word uses Windows' own speech recognizer. To try it, it needs a
speech language installed (Settings > Time & language > Speech, and
Language & region) and the mic allowed for desktop apps (Settings > Privacy
& security > Microphone). If it can't start, it unticks itself and says "I
can't hear names on this computer yet."

## Frame rate and idle cost

The eye doesn't draw as fast as the monitor allows. `src/app/pacing.rs` picks
a frame interval and the loop sleeps until then (`ControlFlow::WaitUntil`):

| State | Eye frames | Face model |
| --- | --- | --- |
| Shown, opening or closing, or a Senses/camera window open | 60 a second | 15 a second |
| Shown but blinded, or calm (only looking around, no face and no noise for a minute) | 30 a second | off when blinded, else 15 |
| Hidden | none; wakes 4 times a second to poll the tray, Ctrl+Alt+B and relaunches | about 3 a second, camera kept open |

All motion is timed by the clock, so fewer frames never slow it down. The
heartbeat line ends in `fps=` to check the rate. To measure on Windows, watch
the CPU and GPU columns in Task Manager (Details tab, with the GPU column
added) for a minute each while shown with a face in view, shown and calm,
blinded, and hidden.

## How the window is transparent

wgpu's DX12 backend is set to `Dx12SwapchainKind::DxgiFromVisual`, which is a
DirectComposition swapchain, with `CompositeAlphaMode::PreMultiplied`. The
window is created without a redirection bitmap. A plain HWND swapchain can't
be transparent. On an RTX 3060, Vulkan also offers `PreMultiplied` and works
as a fallback. The window only shows after a transparent first frame, so
there's no black box.

## Size

Measured on Windows 11 with an RTX 3060 on 2026-10-28, idle and blinded. The
exe was 36.5 MB with full optimisation everywhere. It's 22.3 MB with
size-optimised code except in the face model's runtime, which still looks at
a 1280x720 frame in 23.6 ms (`--bench-face`). Asking wgpu for small GPU
memory blocks (`MemoryHints::MemoryUsage`) saved about 180 MB of private
memory. The comparison with the Electron version is in the
[README](../README.md#size-and-memory).

## The Electron version

The original Electron app was removed from the tree in 0.2.1. It's kept at
the tag `v0.1.0` (also `electron-final`).
