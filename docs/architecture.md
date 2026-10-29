# Architecture

Unblinking is one Rust process on Windows. A winit event loop owns the eye's
window and runs the frame loop on the main thread. The camera, the mic, the
sound output and the wake word each run on threads of their own and talk to
the main loop through channels and atomics, so a slow camera frame never
stalls the eye.

Building, running and testing it: [development.md](development.md).

## The layers

Each frame flows one way, from what the eye senses to what it draws:

```
senses   camera faces, expression, mic loudness, cursor and screen
   |
   v
mind     mode (locked, hunting, blind), gaze, pupil, heat, flare, whispers
   |
   +-->  feeling   which mood fits now, coloured by the bond's temperament
   |        |
   |        v
   |     moods / themes   the mood's look: fire colours, glow, embers, flow
   |
   v
motion   a body for the mind's wishes: springy gaze, breath, pupil, wince
   |
   v
render   uniforms for the eye shader, whispers drawn under it with egui
```

- **Senses** report what is out there: the faces the camera thread found,
  an optional expression reading, one mic loudness number, the cursor, the
  work area and where the webcam sits.
- **The mind** turns that into what the eye wants: a mode (Locked when a face
  is in view, Hunting when not, Blind with the camera and mic off), where to
  look, how wide the pupil is, how hot the fire burns, and the whispers. It
  speaks through a `say` callback with cooldowns so it doesn't nag.
- **Feeling** picks which of the eight moods fits the moment: what's
  happening now (a bang, a face, the hour, a crowd), your face if you opted
  in to "Read my face", and the slow temperament from the bond.
- **Moods and themes** say how the fire looks while the mind does its thing.
  A theme is the whole character (today, The Burning Eye) with its own take
  on every mood. Both come from `assets/blinki.json`, exported by the Blinki
  Studio. Moods blend over 1.4 seconds.
- **The bond** is Remembrance, after Lifesworn's: a ledger of good deeds
  (breaks, rest, coming back, smiling, time together) that slowly shapes the
  blinki's temperament. The wellbeing `Watch` turns each frame's signals into
  those deeds. It is glad about what's good for you, never about time spent
  staring at it.
- **Motion** sits between the mind and the shader. The mind's numbers are
  right but move like a machine; motion gives them weight: a gaze spring
  with drift, a restless pupil, breath, a wince on a flinch, a slow roll.
- **Render** fills the shader's uniforms and draws the frame with wgpu. The
  eye is one full-window triangle and one fragment shader. Whispers are drawn
  after it with egui, in the same transparent window.

## Folder map

| Path | What it holds |
| --- | --- |
| `src/main.rs` | Start-up: the flags, `--bench-face`, and the winit event loop |
| `src/cli.rs` | Parsing the command line flags |
| `src/prefs.rs` | Everything the user can toggle, saved as JSON |
| **`src/app/`** | **The whole app: one `App` that winit calls into** |
| `app/mod.rs` | The `App` struct and what it holds |
| `app/events.rs` | winit's callbacks: window events, the frame loop and its pacing |
| `app/frame.rs` | One frame: senses, mind, feeling, motion, uniforms, and what Blinki's Senses shows |
| `app/pacing.rs` | How often to draw (60, 30, or not at all), as a plain testable function |
| `app/presence.rs` | The window's own life: opening, open, closing, hidden |
| `app/actions.rs` | What the tray, the hotkey and the wake word ask for |
| `app/capture.rs` | Reading back the frame as drawn, for `--snap` and `--record` |
| **`src/render/`** | **Drawing** |
| `render/gpu.rs` | The eye's window and everything on the GPU that draws into it |
| `render/eye.rs` | The eye's wgpu pipeline and its uniforms |
| `render/shaders/` | The GLSL shader: `common.glsl`, `body.glsl`, `aura.glsl`, `eye.frag`, `eye.vert` |
| `render/whisper.rs` | Whispers written under the eye, letter by letter, with each school's entrance |
| `render/fonts.rs` | Tiny5, the whisper font, built into the exe |
| **`src/mind/`** | **Behaviour and looks** |
| `mind/mod.rs` | The `Mind`: modes, gaze, flinches and whispers |
| `mind/motion.rs` | The body that carries out what the mind decides |
| `mind/feeling.rs` | Which mood fits the moment, and how temperament colours it |
| `mind/moods.rs` | Each mood's colours and feel, and the blending between them |
| `mind/themes.rs` | Themes: the whole character, with its own version of each mood |
| **`src/bond/`** | **What the blinki remembers of your days together** |
| `bond/remembrance.rs` | The ledger of deeds, and the temperament worked out from it |
| `bond/wellbeing.rs` | The `Watch` that turns each frame's signals into deeds |
| **`src/senses/`** | **What the eye perceives** |
| `senses/camera.rs` | The camera thread and the channel that brings its reports back |
| `senses/face.rs` | YuNet through tract, with box decoding and NMS done by hand |
| `senses/expression.rs` | The opt-in expression reading: landmark geometry plus a small model |
| `senses/mic.rs` | The default mic, reduced to one loudness number |
| `senses/wake.rs` | The wake word, through Windows' own speech recognizer |
| `senses/placement.rs` | Where the webcam sits, so the eye can aim from the right spot |
| **`src/audio/`** | **Sound** |
| `audio/voice.rs` | The drone, the effects and spoken whispers |
| `audio/sound.rs` | The mixer's settings: Music, Voice and Effects under one master |
| **`src/platform/`** | **What the app asks of Windows** |
| `platform/desktop.rs` | The cursor, the work area, idle time and the hour |
| `platform/fullscreen.rs` | Full-screen jealousy: hiding for full-screen apps |
| `platform/session.rs` | One copy at a time, and starting with the computer |
| `platform/tray.rs` | The tray icon and its menu, also the right-click menu on the eye |
| **`src/ui/`** | **The ordinary windows, drawn with egui** |
| `ui/senses_panel.rs` | Blinki's Senses: Sight, Hearing, Mind and World |
| `ui/camera_picker.rs` | "Where's my camera?" |
| `ui/egui_window.rs` | The plumbing both windows share on the eye's GPU device |
| **`src/tools/`** | **Capture tools, not part of the everyday eye** |
| `tools/demo.rs` | `--demo`: a scripted 14 s performance |
| `tools/record.rs` | `--record`: the frames it draws, written as a GIF |
| `tools/snap.rs` | `--snap`: one frame as a PNG, and a transparency check |
| `assets/` | The face and expression models, the font, `blinki.json`, the icon, and licence texts |

## Threads

- **Main thread.** winit's event loop, the mind, motion, rendering, the
  tray and the egui windows. Everything that decides or draws happens here.
- **Camera thread.** Opens the webcam with nokhwa, runs YuNet through tract
  at most 15 times a second (about 3 while the eye is hidden), and the
  expression reader when it's on. It sends what it saw to the main loop over
  a channel. Frames are dropped once looked at. While Blinki's Senses is
  open, it also sends a small picture for the Sight lens.
- **Mic.** cpal calls a closure on its own real-time audio thread. It
  measures each buffer and stores one loudness number in an atomic, which
  the main loop reads.
- **Sound output.** Another cpal stream. The drone and the effects are
  synthesised in the audio callback, sample by sample, without allocating.
  The main thread writes targets into atomics and the audio thread glides
  toward them, so nobody waits on a lock. Spoken whispers use the system's
  local voice through the `tts` crate.
- **Wake word.** When it's on, Windows' speech recognizer runs on its own
  thread and reports over a channel. Dropping it stops the recognizer and
  frees the mic.

## How a frame flows

`app/events.rs` decides when the next frame is due (see `app/pacing.rs`) and
sleeps until then with `ControlFlow::WaitUntil`. On each redraw:

1. `App::step` in `app/frame.rs` takes the time since the last frame,
   capped at 0.1 s so a stall doesn't make it jump.
2. It reads the latest report from the camera thread and the mic level.
   While the eye is speaking, the mic counts as silent, so it never startles
   at itself.
3. `Mind::update` turns faces, loudness, the cursor and the prefs into the
   mind's state, and may say a whisper.
4. New whispers are queued for the screen and handed to the voice; the drone
   follows the mind's heat and flare, and a new startle plays the flinch.
5. The wellbeing `Watch` notes any good deeds in Remembrance.
6. `Feeling` picks the mood, unless `--mood`, `--demo` or a full-screen sulk
   holds one. The mood's look is tinted by the temperament.
7. `Motion` gives the mind's numbers a body, and opening or closing scales
   the result. Together they become the shader's uniforms.
8. `Gpu::render` draws the eye and any whisper. Then come `--record` and
   `--snap` captures, the camera picker and Blinki's Senses, if open.

Between frames, `about_to_wait` polls the tray, the wake word, a second
launch, the full-screen watcher and the Ctrl+Alt+B hotkey, and saves the
window's spot after a drag. When the eye is hidden it wakes 4 times a second
for those and draws nothing.

## Prefs

Saved as JSON in `%APPDATA%\Unblinking\prefs.json`. It's the same file, keys
and defaults the Electron version used, so settings carried over. Loaded
once at start, key by key: anything missing or of the wrong type falls back
to its default. A menu click changes the pref, saves the file, and applies
it. Remembrance keeps its ledger in `remembrance.json` next to it.

## Privacy boundaries

- **No network.** The models, the font and the moods are built into the exe.
  Nothing is downloaded or sent.
- **Frames stay in memory.** Camera frames go to the face detector and are
  dropped. Blinki's Senses shows a picture live only while it's open.
- **Audio becomes one number.** Each mic buffer is measured and gone.
- **Reading your face is opt-in.** Turning it off drops everything learned
  about your face.
- **The wake word stays local.** It only ever learns "I heard one of your
  phrases", never what else was said.
- **Blind means off.** Blinding releases the camera and mic, and stops the
  wake word.
- **Only settings and deeds are saved.** `prefs.json` and `remembrance.json`.
  "Forget our days" erases the ledger.
