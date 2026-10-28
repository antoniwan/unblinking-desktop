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

## Run it

```bash
cargo run --release
```

Drag the eye with the left mouse button. Right-click closes it (there is no
tray menu yet).

| Flag | What it does |
| --- | --- |
| `--snap=MS` | After MS milliseconds, saves the frame over a dark backdrop to `snap.png` in the current folder, then quits. |
| `--backend=dx12` or `--backend=vulkan` | Uses only that backend. By default it tries DX12 first, then Vulkan. |
| `--blind` | Keeps the camera off. The eye dims and drifts. |

```bash
cargo test
```

Two tests need the webcam, and one of them needs a face in front of it, so
they only run when asked:

```bash
cargo test --release -- --ignored --nocapture
```

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
| `src/senses/face.rs` | YuNet through tract, and the box decoding and NMS that MediaPipe used to do |
| `assets/` | The face model and its license note |
