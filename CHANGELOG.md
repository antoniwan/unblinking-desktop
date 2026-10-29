# Changelog

All notable changes to Unblinking. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org).

## [0.2.1] - 2026-10-29

No user-visible changes. The code is tidier underneath.

### Changed

- **The code is grouped into folders** by job: `app/`, `render/`, `mind/`,
  `bond/`, `senses/`, `audio/`, `platform/`, `ui/` and `tools/`, instead of
  one flat `src/`. `main.rs` is split into `app/`, `cli.rs`, `render/gpu.rs`
  and `tools/snap.rs`.
- **The Rust crate moved to the repo root**, from `rust/`. Its README is now
  `docs/development.md`, and CI runs from the root.

### Removed

- The Electron app's leftovers (`renderer/`, the old `src/`, `scripts/`,
  `package.json`). The Electron version is still at the tag `v0.1.0`.

## [0.2.0] - 2026-10-29

The Rust rewrite: one native exe instead of an Electron app, with much more
to it.

### Added

- **A native Rust app**: winit and wgpu, one exe with the face models, the font
  and the moods inside, and a per-user installer (Start menu shortcut,
  uninstaller).
- **A transparent eye** drawn through a DirectComposition swapchain with
  premultiplied alpha. It opens with an ease and fades out, with no black box.
- **The sphere, raymarched**: a signed-distance body with layered fire inside,
  a solid pupil, and a layered aura (rim bloom, halo, corona, light tongues,
  curling embers), with motion (breath, gaze velocity, a wince).
- **Nine moods, each with a school of magic**: Watchful (Pyromancy), Curious
  (Arcane), Lonely (Umbral), Wrathful (Storm), Delighted (Fae), Suspicious
  (Druidic), Drowsy (Astral), Worried and Blinded (Warding). Each sets the
  fire's colours and how its whispers enter. They blend over 1.4 s, and
  Worried grows more insistent the later it gets past midnight.
- **Themes, feelings and Remembrance**: a private ledger of good days (breaks,
  rest, coming back, smiling) that slowly shapes its temperament, after
  Lifesworn's Remembrance. "Forget our days" erases it.
- **On-device face tracking** with YuNet in pure Rust (tract), aimed with real
  geometry at your eyes, and an opt-in expression reader ("Read my face").
- **Where's my camera?**: your monitors as laid out. Drag the camera to where
  it sits, then fine-tune with "Look at me".
- **Blinki's Senses**: a scrying-glass window showing what it sees, hears and
  feels (Ctrl+Alt+B).
- **Whispers** in the Tiny5 pixel font, letter by letter, with per-school
  entrances, and spoken by the system's local voice if you like.
- **A sound mixer**: Music (a live-synthesised drone), Voice and Effects (a
  whoomp at a bang, a chime with each whisper), each with a switch and a
  volume, under a master switch.
- **It gets out of the way**: it hides for full-screen apps and fades back in.
- **An opt-in wake word**, "Hey, OG Blinki", matched on the computer.
- One copy at a time, start with the computer, frame pacing.
- `--demo`, `--record`, `--snap`, `--senses`, `--camera`, `--bench-face`.
- Windows CI: formatting, clippy with warnings as errors, 168 tests, and a
  release build.

### Changed

- **Smaller and lighter.** Measured idle and blinded on the same PC against
  0.1.0: 1 process instead of 5, a 134.9 MB working set instead of 448.7 MB,
  and 147.9 MB private instead of 231.4 MB. It ships as one exe (22.3 MB as
  measured, 27.5 MB with the expression model and the new sphere) instead of
  a 356.8 MB runtime.
- Face detection is YuNet in pure Rust (23.6 ms per 1280x720 frame) instead of
  MediaPipe in WebAssembly, and nothing is downloaded at setup.
- Prefs keep the same file and keys as 0.1.0 (`prefs.json`); the old Voice
  setting carries over to the sound mixer.

## [0.1.0] - 2026-10-28

The original Electron version, tagged `electron-final`.

### Added

- A floating eye in a transparent, always-on-top Electron window, drawn by
  one WebGL fragment shader.
- Face tracking with MediaPipe in WebAssembly, mic loudness, and a mind that
  locks on, hunts, flinches and whispers.
- A tray menu: hide, blind, whispers, voice, click-through, start with the
  computer, and banish.
- A Web Audio drone and spoken whispers (Voice, off by default).

[0.2.1]: https://github.com/antoniwan/unblinking-desktop/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/antoniwan/unblinking-desktop/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/antoniwan/unblinking-desktop/releases/tag/v0.1.0
