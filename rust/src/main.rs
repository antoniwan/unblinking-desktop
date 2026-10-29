//! Unblinking, in Rust: the eye in a small transparent window that stays on
//! top of everything. This file owns the window and the frame loop.
//!
//! Flags:
//!   --snap=MS        after MS milliseconds, save the frame to snap.png and quit
//!   --backend=NAME   force `dx12` or `vulkan` instead of trying both
//!   --camera         open "Where's my camera?" at start (also in the tray)
//!   --senses         open Blinki's Senses at start (also Ctrl+Alt+B, or the tray)
//!   --mood=NAME      hold one of the studio's moods (for looking at it, or captures)
//!   --demo           a scripted 14 s performance: moods, whispers, a flinch (no camera or mic)
//!   --record=F.gif   save the frames it draws as an animated GIF, then quit when it closes
//!   --whisper=TEXT   show TEXT as a whisper once the eye opens (for captures)
//!   --voice          turn all sound (music, voice, effects) on for this run, without saving it
//!   --blind          start blinded this run (camera and mic off), without saving it
//!
//! Left-drag moves the eye; right-click it (or the tray icon) for the menu.

// In release builds, don't open a console window next to the eye.
// `cfg_attr(condition, attribute)` applies the attribute only when the
// condition holds; `debug_assertions` is on in debug builds only.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Each `mod` line pulls in a file of the same name from src/.
mod camera_picker;
mod demo;
mod desktop;
mod egui_window;
mod eye;
mod feeling;
mod fonts;
mod fullscreen;
mod mind;
mod moods;
mod motion;
mod pacing;
mod placement;
mod prefs;
mod record;
mod remembrance;
mod senses;
mod senses_panel;
mod session;
mod sound;
mod themes;
mod tray;
mod voice;
mod wellbeing;
mod whisper;

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId, WindowLevel};

use desktop::{Point, Rect};
use eye::{Eye, EyeState};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use feeling::Feeling;
use mind::{Line, Mind, Mode, Senses, Vec2, World};
use moods::Moods;
use prefs::{Position, Prefs};
use remembrance::Remembrance;
use senses::Detector;
use senses::camera::Eyes;
use senses::expression::Expression;
use senses::face::YuNet;
use senses::mic::Ears;
use senses::wake::{Wake, WakeEvent};
use tray::{Action, Tray};
use voice::{Effect, Voice};
use wellbeing::Watch;
use whisper::{School, Whispers};

use camera_picker::{CameraPicker, Picked};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use placement::Monitor;
use senses_panel::{LEVEL_HISTORY, SensesPanel, Snapshot};

/// Window size in logical pixels (scaled by the monitor's DPI), as in main.js.
const WIN_SIZE: f64 = 320.0;
/// Gap from the work area's top right corner, in logical pixels.
const MARGIN: f64 = 24.0;
/// The eye sits a little above center so whispers fit below it.
const CENTER: [f32; 2] = [0.0, 0.06];
/// Wait this long after the last move before saving where the eye was dropped.
const SAVE_MOVE_AFTER: Duration = Duration::from_millis(400);
/// How long the eye takes to open when it appears.
const OPEN_SECS: f32 = 0.9;
/// How long it takes to close before it hides or quits.
const CLOSE_SECS: f32 = 0.6;
/// Behind the eye in snap.png, so the flames are judged as on a desktop.
const SNAP_BACKDROP: [u8; 3] = [0x1d, 0x1f, 0x24];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Backend {
    Dx12,
    Vulkan,
}

/// Command line options.
struct Args {
    snap: Option<Duration>,
    backend: Option<Backend>,
    blind: bool,
    mood: Option<String>,
    senses: bool,
    camera: bool,
    voice: bool,
    whisper: Option<String>,
    demo: bool,
    record: Option<String>,
}

impl Args {
    fn parse() -> Self {
        let mut args = Args {
            snap: None,
            backend: None,
            blind: false,
            mood: None,
            senses: false,
            camera: false,
            voice: false,
            whisper: None,
            demo: false,
            record: None,
        };
        for arg in std::env::args().skip(1) {
            // `if let` runs the block only when the pattern matches.
            if let Some(ms) = arg.strip_prefix("--snap=") {
                args.snap = Some(Duration::from_millis(ms.parse().unwrap_or(6000)));
            } else if arg == "--snap" {
                args.snap = Some(Duration::from_millis(6000));
            } else if let Some(name) = arg.strip_prefix("--backend=") {
                args.backend = match name {
                    "dx12" => Some(Backend::Dx12),
                    "vulkan" => Some(Backend::Vulkan),
                    other => {
                        eprintln!("unknown backend {other:?}; trying both");
                        None
                    }
                };
            } else if let Some(name) = arg.strip_prefix("--mood=") {
                args.mood = Some(name.to_string());
            } else if let Some(text) = arg.strip_prefix("--whisper=") {
                args.whisper = Some(text.to_string());
            } else if arg == "--demo" {
                args.demo = true;
            } else if let Some(path) = arg.strip_prefix("--record=") {
                args.record = Some(path.to_string());
            } else if arg == "--voice" {
                args.voice = true;
            } else if arg == "--camera" {
                args.camera = true;
            } else if arg == "--senses" {
                args.senses = true;
            } else if arg == "--blind" {
                args.blind = true;
            } else if arg == "--bench-face" {
                // Handled before the app starts.
            } else {
                eprintln!("ignoring unknown argument {arg:?}");
            }
        }
        args
    }
}

/// What to do once the eye has finished closing.
#[derive(Clone, Copy, Debug, PartialEq)]
enum AfterClose {
    Hide,
    Quit,
}

/// The window's own life, apart from the mind's moods. An enum variant can
/// carry data: `Opening` remembers when it started, `Closing` also what
/// comes after.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Presence {
    Opening(Instant),
    Open,
    Closing(Instant, AfterClose),
    Hidden,
}

impl Presence {
    /// How present the eye is, 0 (gone) to 1 (fully there), eased so it
    /// starts quickly and settles softly.
    fn amount(self) -> f32 {
        match self {
            Presence::Opening(since) => {
                let x = (since.elapsed().as_secs_f32() / OPEN_SECS).min(1.0);
                1.0 - (1.0 - x).powi(3) // ease out
            }
            Presence::Open => 1.0,
            Presence::Closing(since, _) => {
                let x = (since.elapsed().as_secs_f32() / CLOSE_SECS).min(1.0);
                1.0 - x * x // ease in: slow to start, quick at the end
            }
            Presence::Hidden => 0.0,
        }
    }

    fn shown(self) -> bool {
        matches!(self, Presence::Opening(_) | Presence::Open)
    }
}

/// The window and everything on the GPU that draws into it.
struct Gpu {
    // `Arc` is a shared, reference-counted pointer. The surface needs the
    // window to live as long as it does, and so do we; an Arc lets both
    // hold it, and the window is freed only when the last Arc goes away.
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    eye: Eye,
    whispers: Whispers,
    /// Debugging aid: with UNBLINKING_SURFACE_SNAP=MS set, the frame actually
    /// presented at MS is copied to surface.png.
    surface_snap: Option<Instant>,
    // Kept so another window (Blinki's Senses) can draw with the same GPU.
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
}

impl Gpu {
    /// Opens the window and sets up `backend` to draw into it.
    ///
    /// Returns `Result`: either `Ok(Gpu)` or `Err(reason)`. The `?` after a
    /// call means "if this failed, return its error from here right away".
    /// `Box<dyn Error>` is "any kind of error", handy at the top of an app.
    fn new(
        event_loop: &ActiveEventLoop,
        backend: Backend,
        allow_opaque: bool,
        prefs: &Prefs,
    ) -> Result<Gpu, Box<dyn Error>> {
        let window =
            Arc::new(event_loop.create_window(window_attributes(event_loop, backend, prefs))?);

        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = match backend {
            Backend::Dx12 => wgpu::Backends::DX12,
            Backend::Vulkan => wgpu::Backends::VULKAN,
        };
        // A plain HWND swapchain can't be see-through. A DirectComposition
        // visual can, so ask DX12 for that.
        desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
        let instance = wgpu::Instance::new(desc);

        // `window.clone()` clones the Arc (one more owner), not the window.
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))?;
        let (device, queue) =
            // Ask for small GPU memory blocks: the eye needs little, and the
            // default (`Performance`) reserves big ones, about 180 MB more here.
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            }))?;

        let caps = surface.get_capabilities(&adapter);
        // Prefer a compositor that understands our premultiplied pixels.
        let alpha_mode = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
        ]
        .into_iter()
        .find(|m| caps.alpha_modes.contains(m));
        let alpha_mode = match alpha_mode {
            Some(mode) => mode,
            None if allow_opaque => caps.alpha_modes[0],
            None => {
                return Err(
                    format!("no transparent alpha mode, only {:?}", caps.alpha_modes).into(),
                );
            }
        };
        // WebGL wrote the shader's numbers straight to the screen; a
        // non-sRGB format does the same (sRGB would brighten them again).
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | (caps.usages & wgpu::TextureUsages::COPY_SRC),
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            color_space: Default::default(),
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let eye = Eye::new(&device, format);
        let whispers = Whispers::new(&device, format);
        whispers.set_fonts(fonts::blinki());

        let info = adapter.get_info();
        eprintln!(
            "unblinking: {backend:?} on {} ({:?}), alpha {alpha_mode:?} of {:?}, format {format:?}, {}x{}",
            info.name, info.backend, caps.alpha_modes, size.width, size.height
        );
        Ok(Gpu {
            instance,
            adapter,
            window,
            surface,
            device,
            queue,
            config,
            eye,
            whispers,
            surface_snap: std::env::var("UNBLINKING_SURFACE_SNAP")
                .ok()
                .and_then(|ms| ms.parse().ok())
                .map(|ms| Instant::now() + Duration::from_millis(ms)),
        })
    }

    fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Presents one fully transparent frame. Done before the window is shown
    /// and before it hides, so it never flashes an empty (black) surface.
    fn clear(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        // A pass that only clears to (0, 0, 0, 0): premultiplied nothing.
        // The braces end the pass (dropping it) before `finish` needs the encoder.
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.queue.present(frame);
    }

    /// Draws one frame to the window.
    fn render(&mut self, state: &EyeState) {
        // `match` must handle every variant of the enum; `_` catches the rest.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return, // timed out or hidden: skip this frame
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.eye
            .draw(&self.queue, &mut encoder, &view, self.size(), state);
        let scale = self.window.scale_factor() as f32;
        self.whispers.draw(
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            self.size(),
            scale,
            state.colors,
            state.fade,
        );
        let readback = self.surface_snap.filter(|t| Instant::now() >= *t).map(|_| {
            self.surface_snap = None;
            let (w, h) = self.size();
            let row = (w * 4).div_ceil(256) * 256;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("surface snap"),
                size: (row * h) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                frame.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            (buffer, row)
        });
        self.queue.submit([encoder.finish()]);
        if let Some((buffer, row)) = readback {
            let slice = buffer.slice(..);
            slice.map_async(wgpu::MapMode::Read, |r| r.expect("map"));
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            let data = slice.get_mapped_range().expect("read");
            let (w, h) = self.size();
            let mut rgba = Vec::new();
            for y in 0..h as usize {
                for px in data[y * row as usize..y * row as usize + w as usize * 4]
                    .as_chunks::<4>()
                    .0
                {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            }
            match write_snap("surface.png", (w, h), &rgba) {
                Ok(()) => eprintln!("surface.png written"),
                Err(e) => eprintln!("surface snap failed: {e}"),
            }
        }
        self.window.pre_present_notify();
        self.queue.present(frame);
    }
}

/// The eye's window: frameless, see-through, always on top, off the taskbar.
/// It goes where it was last dragged, if that spot is still on a screen,
/// else in the top right corner of the primary monitor's work area.
fn window_attributes(
    event_loop: &ActiveEventLoop,
    backend: Backend,
    prefs: &Prefs,
) -> winit::window::WindowAttributes {
    let scale = event_loop
        .primary_monitor()
        .map_or(1.0, |m| m.scale_factor());
    let side = (WIN_SIZE * scale).round();

    let mut attrs = Window::default_attributes()
        .with_title("Unblinking")
        .with_inner_size(LogicalSize::new(WIN_SIZE, WIN_SIZE))
        .with_resizable(false)
        .with_decorations(false)
        .with_transparent(true)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_active(false)
        .with_visible(false);
    if let Some(pos) = prefs.position.filter(|&p| on_some_screen(event_loop, p)) {
        attrs = attrs.with_position(LogicalPosition::new(pos.x, pos.y));
    } else if let Some(area) = desktop::work_area() {
        let x = area.x + area.width - side - MARGIN * scale;
        let y = area.y + MARGIN * scale;
        attrs = attrs.with_position(PhysicalPosition::new(x, y));
    }

    #[cfg(windows)]
    {
        // Extension traits add platform-only methods to winit's types;
        // bringing the trait into scope with `use` makes them callable.
        use winit::platform::windows::WindowAttributesExtWindows;
        attrs = attrs
            .with_skip_taskbar(true)
            .with_undecorated_shadow(false)
            // DirectComposition draws the window itself; the GDI bitmap
            // Windows would otherwise keep behind it is just an opaque layer.
            .with_no_redirection_bitmap(backend == Backend::Dx12);
    }
    attrs
}

/// Whether a saved spot (logical pixels) still lands on a monitor, with the
/// same 100 pixel slack main.js allowed. Monitors come and go.
fn on_some_screen(event_loop: &ActiveEventLoop, p: Position) -> bool {
    event_loop.available_monitors().any(|m| {
        let k = m.scale_factor();
        let (x, y) = (m.position().x as f64 / k, m.position().y as f64 / k);
        let (w, h) = (m.size().width as f64 / k, m.size().height as f64 / k);
        p.x >= x - 100.0 && p.x < x + w - 100.0 && p.y >= y - 100.0 && p.y < y + h - 100.0
    })
}

/// The whole app. winit calls into it through `ApplicationHandler`.
/// Hours after midnight before this count as the small hours.
const LATE_UNTIL_HOUR: u32 = 5;

struct App {
    args: Args,
    gpu: Option<Gpu>,
    started: Instant,
    last_frame: Instant,
    /// When the next frame is due (see pacing.rs), and frames drawn since
    /// the last heartbeat line, to print the frame rate.
    next_frame: Instant,
    frames: u32,
    mind: Mind,
    /// The body that carries out the mind's wishes: weight, breath, flinch.
    motion: motion::Motion,
    mode: Option<Mode>,
    /// The camera thread, while the eye is allowed to see.
    eyes: Option<Eyes>,
    /// The microphone, while the eye is allowed to hear.
    ears: Option<Ears>,
    /// Listening for "Hey, OG Blinki", while that's on and it isn't blind.
    wake: Option<Wake>,
    prefs: Prefs,
    tray: Option<Tray>,
    presence: Presence,
    /// When to save the window's spot, a moment after the last move.
    save_move_at: Option<Instant>,
    last_beat: Instant,
    moods: Moods,
    /// Picks the mood from the moment, your face and its temperament.
    feeling: Feeling,
    /// What it remembers of your days, while "Remember our days" is on.
    book: Option<Remembrance>,
    /// Notices breaks, returns and smiles, for the book.
    watch: Watch,
    /// The latest reading of your face, while "Read my face" is on.
    expression: Option<Expression>,
    /// The flame drift's looping phase, 0..1, advanced at the mood's speed.
    flow: f32,
    /// The latest whisper and when, shared with the mind's `say` closure.
    /// `Rc<Cell<..>>`: both own it (Rc), and a Copy value inside a Cell can
    /// be swapped even through a shared handle.
    last_said: Rc<Cell<Option<(Line, Instant)>>>,
    /// When it last saw a face, and when it last heard anything louder than quiet.
    last_face: Option<Instant>,
    last_noise: Instant,
    /// The screen's pixels per metre, to place the eye in the room.
    px_per_m: f64,
    /// Blinki's Senses, while it's open, and what it shows.
    panel: Option<SensesPanel>,
    /// "Where's my camera?", while it's open.
    picker: Option<CameraPicker>,
    /// The monitors as they're laid out, for placing the camera.
    monitors: Vec<Monitor>,
    snapshot: Option<Snapshot>,
    levels: VecDeque<f32>,
    /// One flag per entry in `levels`: did it flinch on that frame?
    flinch_marks: VecDeque<bool>,
    last_startle: f64,
    /// Every whisper so far, for the Mind lens.
    #[allow(clippy::type_complexity)]
    whisper_log: Rc<RefCell<Vec<(Instant, Line, &'static str)>>>,
    /// The drone and spoken whispers, and how many logged whispers it has seen.
    voice: Voice,
    spoken: usize,
    /// A whisper to put on screen once the frame's GPU work is at hand.
    pending_whisper: Option<String>,
    /// The startle the flinch effect last played for.
    last_flinch_sound: f64,
    /// --demo's script, while it plays.
    demo: Option<demo::Demo>,
    /// --record's frames, written out when the app ends.
    recording: Option<record::Recording>,
    /// Seconds you've been up and here past midnight tonight, and when you
    /// were last here (face or keyboard/mouse), for the Worried mood.
    up_late: f32,
    last_here: Instant,
    /// When it last told you to rest, so it doesn't nag.
    last_worry: Option<Instant>,
    /// Proof this is the only copy running; another launch pokes it awake.
    instance: Option<session::SingleInstance>,
    /// Owns the Ctrl+Alt+B shortcut; dropping it would unregister it.
    _hotkeys: Option<GlobalHotKeyManager>,
    /// Hides the eye during full-screen games, and the pout after.
    fullscreen: fullscreen::Watcher,
}

impl App {
    /// Where the cursor, the eye and its monitor are right now.
    fn world(&self, window: &Window) -> Option<World> {
        let cursor = desktop::cursor()?;
        let pos = window.outer_position().ok()?;
        let size = window.inner_size();
        let monitor = window.current_monitor()?;
        let (mp, ms) = (monitor.position(), monitor.size());
        // Where the webcam is: the spot picked in "Where's my camera?", or
        // the top middle of the main screen.
        let (camera, px_per_m) = placement::locate(self.prefs.camera.as_ref(), &self.monitors)
            .unwrap_or((
                Point {
                    x: mp.x as f64 + ms.width as f64 / 2.0,
                    y: mp.y as f64,
                },
                self.px_per_m,
            ));
        Some(World {
            cursor,
            eye: Point {
                x: pos.x as f64 + size.width as f64 / 2.0,
                y: pos.y as f64 + size.height as f64 * 0.42,
            },
            display: Rect {
                x: mp.x as f64,
                y: mp.y as f64,
                width: ms.width as f64,
                height: ms.height as f64,
            },
            scale: window.scale_factor(),
            camera,
            px_per_m,
            look_offset: self
                .prefs
                .look_offset
                .map_or_else(Vec2::default, |p| Vec2::new(p.x, p.y)),
        })
    }

    /// Advances the mind one frame and turns its mood into the eye's uniforms.
    fn step(&mut self, window: &Window) -> EyeState {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;

        // What the camera saw and the mic heard last.
        let (faces, can_see) = match &mut self.eyes {
            Some(eyes) => {
                let sight = eyes.look();
                let faces: Vec<_> = sight.detections.iter().map(|d| d.face).collect();
                self.expression = sight.expression;
                (faces, sight.can_see)
            }
            None => (vec![], false),
        };
        // The mic hears the speakers, so it goes deaf while the eye speaks.
        let heard = if let Some(demo) = &self.demo {
            demo.level()
        } else if self.voice.speaking() {
            0.0
        } else {
            self.ears.as_ref().map_or(0.0, Ears::level)
        };
        // In --demo, the cursor follows the script.
        let mut world = self.world(window);
        if let (Some(demo), Some(w)) = (&self.demo, &mut world) {
            w.cursor = demo.cursor(w.eye, w.scale);
        }
        let s = self.mind.update(
            dt,
            &Senses {
                faces: &faces,
                can_see,
                level: heard,
                world,
                blind: self.prefs.blind && self.demo.is_none(),
                // In --demo the script does the talking.
                whispers: self.prefs.whispers && self.demo.is_none(),
            },
        );

        // Debug heartbeat in the terminal, as app.js had: what it sees.
        self.frames += 1;
        if self.last_beat.elapsed() >= Duration::from_secs(3) {
            let fps = self.frames as f64 / self.last_beat.elapsed().as_secs_f64();
            self.frames = 0;
            self.last_beat = now;
            for (i, f) in faces.iter().enumerate().skip(1) {
                eprintln!(
                    "  also face {i}: ({:.2},{:.2}) size={:.2} score={:.2}",
                    f.x, f.y, f.size, f.score
                );
            }
            let first = faces
                .first()
                .map(|f| {
                    format!(
                        " face=({:.2},{:.2}) size={:.2} score={:.2}",
                        f.x, f.y, f.size, f.score
                    )
                })
                .unwrap_or_default();
            let level = self.ears.as_ref().map(Ears::level);
            eprintln!(
                "mode={:?} mood={} sight={can_see} faces={}{first} level={level:.2?} gaze=({:.2},{:.2}) fps={fps:.0}",
                s.mode,
                self.moods.current().name,
                faces.len(),
                s.gaze.x,
                s.gaze.y
            );
        }

        // A line in the terminal whenever the mood changes.
        if self.mode != Some(s.mode) {
            eprintln!("mode: {:?}", s.mode);
            self.mode = Some(s.mode);
        }

        // Speak the newest whisper, if the voice is on; follow the mood's heat.
        let latest = {
            let log = self.whisper_log.borrow();
            let fresh = log.len() > self.spoken;
            self.spoken = log.len();
            fresh
                .then(|| log.last().map(|(_, _, text)| *text))
                .flatten()
        };
        if let Some(text) = latest {
            if self.prefs.whispers {
                self.pending_whisper = Some(text.to_string());
            }
            self.voice.say(text);
        }
        self.voice.update(&s, dt as f32);
        // A new startle: the flinch effect.
        let startle = self.mind.insight().last_startle;
        if startle > self.last_flinch_sound {
            self.last_flinch_sound = startle;
            self.voice.effect(Effect::Flinch);
        }

        // The mood: which of the studio's looks fits what's happening.
        let level = heard;
        if !faces.is_empty() {
            self.last_face = Some(now);
        }
        if level > 0.3 {
            self.last_noise = now;
        }
        // Your face, if you let it read it (None otherwise).
        let expression = self.expression;

        // What it remembers: notice good deeds, and whisper any first.
        if let Some(book) = &mut self.book {
            let firsts = self.watch.update(
                &wellbeing::Signals {
                    now: remembrance::unix_now(),
                    dt: dt as f32,
                    face: !faces.is_empty(),
                    idle_secs: desktop::idle_secs(),
                    hour: desktop::local_hour(),
                    expression,
                },
                book,
            );
            if let Some(first) = firsts.into_iter().next() {
                eprintln!("remembered a first: {}", first.line);
                if self.prefs.whispers {
                    self.voice.say(&first.line);
                    self.pending_whisper = Some(first.line);
                }
            }
        }
        // Up late: time here (face, or input in the last 30 s) after
        // midnight. Five minutes away, or morning, starts it over.
        let here = !faces.is_empty() || desktop::idle_secs().is_some_and(|i| i < 30.0);
        if here {
            self.last_here = now;
        }
        let small_hours = desktop::local_hour() < LATE_UNTIL_HOUR;
        if !small_hours || self.last_here.elapsed() > Duration::from_secs(300) {
            self.up_late = 0.0;
        } else if here {
            self.up_late += dt as f32;
        }

        let now_unix = remembrance::unix_now();
        let temperament = self.book.as_ref().map(|b| b.temperament(now_unix));

        let choice = self.feeling.update(
            dt as f32,
            &feeling::Context {
                mode: s.mode,
                last_said: self
                    .last_said
                    .get()
                    .map(|(kind, at)| (kind, at.elapsed().as_secs_f32())),
                no_face_secs: self
                    .last_face
                    .map_or(self.started.elapsed(), |t| t.elapsed())
                    .as_secs_f32(),
                hour: desktop::local_hour(),
                quiet_secs: self.last_noise.elapsed().as_secs_f32(),
                too_close: faces.iter().any(|f| f.score > 0.55 && f.size > 0.5),
                crowd: mind::crowd_size(&faces),
                temperament,
                expression,
                up_late_secs: self.up_late,
            },
        );
        // Worried: tell you to rest, gently, at most twice an hour.
        // It gets more insistent the later it gets (feeling::worry).
        let worry = feeling::worry(self.up_late);
        if choice.mood == "Worried"
            && self
                .last_worry
                .is_none_or(|t| t.elapsed().as_secs_f32() > worry.every_secs)
        {
            self.last_worry = Some(now);
            let line = worry.lines[fastrand::usize(..worry.lines.len())];
            eprintln!("worried: {line}");
            if self.prefs.whispers {
                self.voice.say(line);
                self.pending_whisper = Some(line.to_string());
            }
        }
        // `as_deref` turns `Option<String>` into `Option<&str>` without copying.
        let sulk = self.fullscreen.sulk().filter(|_| !self.prefs.blind);
        let (mood, reason) = match (self.args.mood.as_deref(), &self.demo, sulk) {
            (Some(m), _, _) => (m, "held by --mood"),
            (None, Some(demo), _) => (demo.mood(), "the --demo script"),
            (None, None, Some(m)) => (m, "sulking after a full-screen app"),
            (None, None, None) => (choice.mood, choice.reason),
        };
        // The script's whispers, unless a mood is held (stills for the README).
        let script_talks = self.args.mood.is_none();
        if let Some(text) = self
            .demo
            .as_mut()
            .filter(|_| script_talks)
            .and_then(|d| d.whisper())
        {
            match text.1 {
                // Logged like the mind's own, so Blinki's Senses lists it;
                // the log puts it on screen (and speaks it) next frame.
                Some(kind) => self
                    .whisper_log
                    .borrow_mut()
                    .push((Instant::now(), kind, text.0)),
                None => self.pending_whisper = Some(text.0.to_string()),
            }
        }
        self.moods.set(mood);
        let mut look = self.moods.now();
        if let (Some(t), Some(book)) = (temperament, &self.book) {
            // Remembrance's day runs 0.45 (left alone) to 1 (well cared for);
            // tint wants 0..1 with 0.5 an ordinary day.
            let day = (book.day_brightness(now_unix) - 0.45) / 0.55 * 0.5 + 0.25;
            look = feeling::tint(look, &t, day);
        }
        if mood == "Worried" {
            look.shake += worry.tremble;
        }
        self.flow = (self.flow + dt as f32 * look.flow) % 1.0;

        // The mind's frame, given a body: a gaze with weight and drift, a
        // restless pupil, breath, and a flinch (see motion.rs).
        let m = self.motion.update(dt, &s);

        // Opening, the eye grows and fades in with its pupil wide, then the
        // pupil settles to what the mind wants. Closing, it narrows, cools,
        // shrinks and fades to nothing.
        let k = self.presence.amount();
        let closing = matches!(self.presence, Presence::Closing(..));
        let pupil = m.pupil;
        let pupil = if closing {
            pupil * k
        } else {
            1.0 + (pupil - 1.0) * k
        };
        let cool = if closing { k } else { 1.0 };

        // The mind's numbers carry the moment; the mood scales them. Heat is
        // relative to Watchful's 0.55, so Watchful leaves it as the mind set it.
        let jitter = (look.shake + s.flare as f32 * 0.5) * 0.006;
        let shake = [
            s.shake.x as f32 + (fastrand::f32() - 0.5) * jitter,
            s.shake.y as f32 + (fastrand::f32() - 0.5) * jitter,
        ];
        let state = EyeState {
            flow: self.flow,
            gaze: m.gaze,
            pupil,
            heat: m.heat * look.heat / 0.55 * cool,
            flare: s.flare as f32,
            wide: s.wide as f32 * look.wake * cool,
            dim: (s.dim as f32).max(look.dim),
            center: [CENTER[0] + shake[0], CENTER[1] + shake[1]],
            fade: k,
            size: 0.75 + 0.25 * k,
            colors: look.display_colors(),
            swirl: look.swirl,
            glow: look.glow,
            embers: look.embers,
            time: (self.started.elapsed().as_secs_f32()) % 3600.0,
            breath: m.breath,
            morph: m.morph,
            gaze_vel: m.gaze_vel,
            pupil_vel: m.pupil_vel,
            quality: m.quality,
            wince: m.wince,
            roll: m.roll,
        };
        if self.panel.is_some() {
            self.observe(&s, &state, reason, level, window);
        }
        state
    }

    /// Gathers what Blinki's Senses shows this frame.
    fn observe(
        &mut self,
        s: &mind::Frame,
        state: &EyeState,
        reason: &'static str,
        level: f64,
        window: &Window,
    ) {
        let insight = self.mind.insight();
        // A new startle time means it flinched this frame.
        let flinched = insight.last_startle > self.last_startle;
        self.last_startle = insight.last_startle;
        self.levels.push_back(level as f32);
        self.flinch_marks.push_back(flinched);
        while self.levels.len() > LEVEL_HISTORY {
            self.levels.pop_front();
            self.flinch_marks.pop_front();
        }
        let (detections, can_see, picture) = match &mut self.eyes {
            Some(eyes) => {
                let sight = eyes.look();
                (
                    sight.detections.clone(),
                    sight.can_see,
                    sight.preview.clone(),
                )
            }
            None => (vec![], false, None),
        };
        // Where "sitting normally" lands in the picture, at the tracked face's distance.
        let rest_in_picture = insight.tracked.map(|f| {
            let off = self.prefs.look_offset.map_or((0.0, 0.0), |p| (p.x, p.y));
            let [_, _, dist] = mind::face_in_room(&f);
            let tan_h = (70.0f64.to_radians() / 2.0).tan();
            let tan_v = tan_h / f.aspect;
            let x = 0.5 - (mind::REST_SEAT.x + off.0) / (2.0 * tan_h * dist);
            let y = 0.5 + (mind::REST_SEAT.y + off.1) / (2.0 * tan_v * dist);
            [x as f32, y as f32]
        });
        let world = self.world(window);
        let whispers = self
            .whisper_log
            .borrow()
            .iter()
            .map(|(at, kind, text)| (insight.t - at.elapsed().as_secs_f64(), *kind, *text))
            .collect();
        self.snapshot = Some(Snapshot {
            frame: *s,
            drawn_gaze: state.gaze,
            mood: self.moods.current().name.clone(),
            mood_reason: reason,
            mood_blend: self.moods.blend(),
            colors: state.colors,
            detections,
            can_see,
            blind: self.prefs.blind && self.demo.is_none(),
            demo: self.demo.is_some(),
            picture,
            rest_in_picture,
            levels: self.levels.clone(),
            flinches: self
                .flinch_marks
                .iter()
                .enumerate()
                .filter(|(_, f)| **f)
                .map(|(i, _)| i)
                .collect(),
            heard: match &self.demo {
                Some(demo) => Some(demo.level()),
                None => self.ears.as_ref().map(Ears::level),
            },
            whispers,
            cursor: world.map(|w| w.cursor),
            eye: world.map(|w| w.eye),
            display: world.map(|w| w.display),
            idle: desktop::idle_secs(),
            clock: desktop::local_time(),
            sound: self.prefs.sound,
            camera: self.prefs.camera.as_ref().map_or_else(
                || "top middle of the main screen (default)".to_string(),
                |c| c.describe(),
            ),
            remembers: self.book.as_ref().map(|b| {
                let traits = b.traits().into_iter().map(|t| t.name()).collect();
                (traits, b.journal().last().map(|e| e.line.clone()))
            }),
            expression: self.expression,
            insight,
        });
    }

    /// Reads the monitors' layout and sizes, for placing the camera.
    fn refresh_monitors(&mut self, window: &Window) {
        let primary = window.primary_monitor();
        self.monitors = window
            .available_monitors()
            .map(|m| {
                let (p, s) = (m.position(), m.size());
                let name = m.name().unwrap_or_default();
                Monitor {
                    px_per_m: desktop::monitor_px_per_m(&name, s.width as f64),
                    primary: primary.as_ref() == Some(&m),
                    rect: Rect {
                        x: p.x as f64,
                        y: p.y as f64,
                        width: s.width as f64,
                        height: s.height as f64,
                    },
                    name,
                }
            })
            .collect();
    }

    /// Opens "Where's my camera?", or closes it.
    fn toggle_picker(&mut self, event_loop: &ActiveEventLoop) {
        if self.picker.take().is_some() {
            return;
        }
        let Some(gpu) = &self.gpu else { return };
        let window = gpu.window.clone();
        match CameraPicker::new(event_loop, (&gpu.instance, &gpu.adapter, &gpu.device)) {
            Ok(picker) => self.picker = Some(picker),
            Err(err) => eprintln!("could not open Where's my camera?: {err}"),
        }
        self.refresh_monitors(&window);
    }

    /// Opens Blinki's Senses, or closes it (which also stops the camera preview).
    fn toggle_senses(&mut self, event_loop: &ActiveEventLoop) {
        if self.panel.take().is_some() {
            self.snapshot = None;
            if let Some(eyes) = &mut self.eyes {
                eyes.set_preview(false);
            }
            return;
        }
        let Some(gpu) = &self.gpu else { return };
        match SensesPanel::new(event_loop, (&gpu.instance, &gpu.adapter, &gpu.device)) {
            Ok(panel) => {
                self.panel = Some(panel);
                if let Some(eyes) = &mut self.eyes {
                    eyes.set_preview(true);
                }
            }
            Err(err) => eprintln!("could not open Blinki's Senses: {err}"),
        }
    }

    /// Opens or closes the camera and mic together. Closing really stops the
    /// devices: setting the Options to `None` drops the Eyes and Ears, and
    /// their `Drop` code lets go of the hardware.
    fn set_senses(&mut self, open: bool) {
        if !open {
            self.eyes = None;
            self.ears = None;
            self.expression = None;
            self.sync_wake();
            eprintln!("senses closed");
            return;
        }
        if self.eyes.is_none() {
            self.eyes = Some(Eyes::open(|| {
                let net = YuNet::new().map_err(|e| e.to_string())?;
                Ok(Box::new(net) as Box<dyn Detector>)
            }));
            if let Some(eyes) = &mut self.eyes {
                eyes.set_feel(self.prefs.feel);
            }
        }
        if self.ears.is_none() {
            // `.map_err(..).ok()` logs a failure and carries on deaf:
            // a missing sense never stops the eye.
            self.ears = Ears::open()
                .map_err(|err| eprintln!("no hearing: {err}"))
                .ok();
        }
        self.sync_wake();
    }

    /// Starts or stops the wake word to match its toggle. Blind wins: a
    /// blinded eye hears nothing, names included, and dropping the Wake
    /// frees the mic.
    fn sync_wake(&mut self) {
        if !self.prefs.wake_word || self.prefs.blind {
            self.wake = None;
        } else if self.wake.is_none() {
            // No name pref yet, so only "Hey, OG Blinki" and "Hey Blinki".
            self.wake = Some(Wake::start(self.prefs.name.clone()));
        }
    }

    /// Someone called it, or the wake word could not work here.
    fn on_wake(&mut self, event: WakeEvent) {
        match event {
            WakeEvent::Heard(words) => {
                eprintln!("wake word: heard {words:?}");
                if !self.presence.shown() {
                    self.appear();
                }
                self.mind.called();
            }
            WakeEvent::Failed(why) => {
                // Said once: the toggle turns itself off, so this can't repeat
                // until the user turns it back on.
                eprintln!("wake word off: {why}");
                self.wake = None;
                self.prefs.wake_word = false;
                self.prefs.save();
                if self.prefs.whispers {
                    const DEAF: &str = "I can't hear names on this computer yet.";
                    self.pending_whisper = Some(DEAF.to_string());
                    self.voice.say(DEAF);
                }
            }
        }
        if let Some(tray) = &self.tray {
            tray.refresh(&self.prefs, self.presence.shown());
        }
    }

    /// Carries out one menu or tray click.
    fn act(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        let Some(gpu) = &self.gpu else { return };
        let window = gpu.window.clone();
        // A match arm can have a guard (`if ...`); arms are tried in order.
        match action {
            Action::Quit => self.close(AfterClose::Quit, event_loop),
            Action::Senses => self.toggle_senses(event_loop),
            Action::WhereCamera => self.toggle_picker(event_loop),
            Action::StartWithComputer(on) => {
                if let Err(err) = session::set_starts_with_computer(on) {
                    eprintln!("could not change start with the computer: {err}");
                }
            }
            Action::ToggleShown if self.presence.shown() => {
                self.close(AfterClose::Hide, event_loop)
            }
            Action::ToggleShown => self.appear(),
            Action::Blind(on) => {
                self.prefs.blind = on;
                self.set_senses(!on);
            }
            Action::Whispers(on) => self.prefs.whispers = on,
            Action::HideFullscreen(on) => self.prefs.hide_fullscreen = on,
            Action::WakeWord(on) => {
                self.prefs.wake_word = on;
                self.sync_wake();
            }
            Action::Mute(channel, on) => {
                channel.set(&mut self.prefs.sound, on);
                self.apply_sound();
            }
            Action::LookAtMe => {
                // The face the camera sees now, wherever it is, becomes "sitting normally".
                let face = self
                    .eyes
                    .as_mut()
                    .and_then(|e| e.look().detections.first().map(|d| d.face));
                match face {
                    Some(face) => {
                        let [x, y, _] = mind::face_in_room(&face);
                        let rest = mind::REST_SEAT;
                        self.prefs.look_offset = Some(Position {
                            x: x - rest.x,
                            y: y - rest.y,
                        });
                        eprintln!("look at me: offset {:?}", self.prefs.look_offset);
                    }
                    None => eprintln!("look at me: no face in view"),
                }
            }
            Action::ClickThrough(on) => {
                self.prefs.click_through = on;
                let _ = window.set_cursor_hittest(!on);
            }
            Action::Theme(id) => {
                self.prefs.theme = Some(id.to_string());
                self.moods.set_theme(id);
            }
            Action::ReadFace(on) => {
                self.prefs.feel = on;
                if let Some(eyes) = &mut self.eyes {
                    eyes.set_feel(on);
                }
                if !on {
                    self.expression = None;
                }
            }
            Action::Forget => {
                // Wipes the file. If it's still remembering, it starts afresh.
                let now = remembrance::unix_now();
                match &mut self.book {
                    Some(book) => book.forget(now),
                    None => Remembrance::load(now).forget(now),
                }
                self.watch = Watch::new();
                eprintln!("forgot our days");
            }
            Action::Remember(on) => {
                self.prefs.remember = on;
                // Off: save what it has and stop noticing. The file stays
                // on this computer until "Forget our days".
                if let Some(mut book) = self.book.take() {
                    book.save(remembrance::unix_now());
                }
                if on {
                    self.book = Some(Remembrance::load(remembrance::unix_now()));
                    self.watch = Watch::new();
                }
            }
        }
        if !matches!(
            action,
            Action::ToggleShown
                | Action::Quit
                | Action::Senses
                | Action::WhereCamera
                | Action::StartWithComputer(_)
                | Action::Forget
        ) {
            self.prefs.save();
        }
        if let Some(tray) = &self.tray {
            tray.refresh(&self.prefs, self.presence.shown());
        }
    }

    /// Hands the mixer to the voice, and keeps Electron's "voice" key in step.
    fn apply_sound(&mut self) {
        self.voice.apply(self.prefs.sound.levels());
        self.prefs.voice = self.prefs.sound.audible();
    }

    /// Shows the window (after a clear frame, so no black box) and opens the eye.
    fn appear(&mut self) {
        if let Some(gpu) = &mut self.gpu
            && self.presence == Presence::Hidden
        {
            gpu.clear();
            gpu.window.set_visible(true);
            gpu.window.request_redraw();
        }
        self.presence = Presence::Opening(Instant::now());
    }

    /// Starts closing the eye; the frame loop hides or quits once it's done.
    fn close(&mut self, after: AfterClose, event_loop: &ActiveEventLoop) {
        if after == AfterClose::Quit
            && let Some(book) = &mut self.book
        {
            book.save(remembrance::unix_now());
        }
        if self.presence == Presence::Hidden {
            // Nothing on screen to animate.
            if after == AfterClose::Quit {
                event_loop.exit();
            }
            return;
        }
        self.presence = Presence::Closing(Instant::now(), after);
    }

    /// After a frame: finish an opening, or end a closing with a clear frame.
    fn advance_presence(&mut self, gpu: &mut Gpu, event_loop: &ActiveEventLoop) {
        match self.presence {
            Presence::Opening(since) if since.elapsed().as_secs_f32() >= OPEN_SECS => {
                self.presence = Presence::Open;
            }
            Presence::Closing(since, after) if since.elapsed().as_secs_f32() >= CLOSE_SECS => {
                gpu.clear();
                gpu.window.set_visible(false);
                self.presence = Presence::Hidden;
                if after == AfterClose::Quit {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    /// Draws a frame if one is due, and tells winit when to wake next.
    fn pace(&mut self, event_loop: &ActiveEventLoop) {
        let hidden = self.presence == Presence::Hidden;
        // Look at faces less often while nobody can see the eye react.
        if let Some(eyes) = &self.eyes {
            eyes.set_resting(hidden);
        }
        let no_face_for = self
            .last_face
            .map_or(self.started.elapsed(), |t| t.elapsed());
        let p = pacing::Pacing {
            shown: self.presence.shown(),
            fading: matches!(self.presence, Presence::Opening(_) | Presence::Closing(..)),
            blind: self.prefs.blind,
            calm: self.mode == Some(Mode::Hunting)
                && no_face_for >= pacing::CALM_AFTER
                && self.last_noise.elapsed() >= pacing::CALM_AFTER,
            egui_open: self.panel.is_some() || self.picker.is_some(),
        };
        let now = Instant::now();
        let Some(interval) = pacing::frame_interval(p) else {
            // Nothing to draw. Take in the camera's reports so they don't
            // pile up in the channel, and tick slowly for the tray's sake.
            if let Some(eyes) = &mut self.eyes {
                eyes.look();
            }
            self.next_frame = now;
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + pacing::HIDDEN_TICK));
            return;
        };
        if now >= self.next_frame {
            self.next_frame = pacing::next_due(self.next_frame, now, interval);
            if let Some(gpu) = &self.gpu {
                if hidden {
                    // A hidden window gets no redraw events from Windows,
                    // so hand ourselves one (it draws the egui windows only).
                    let id = gpu.window.id();
                    self.window_event(event_loop, id, WindowEvent::RedrawRequested);
                } else {
                    gpu.window.request_redraw();
                }
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }

    fn snap(&self, gpu: &mut Gpu, state: &EyeState) -> Result<(), Box<dyn Error>> {
        let size = gpu.size();
        let rgba = self.grab(gpu, state);
        write_snap("snap.png", size, &rgba)?;
        let (clear, partial, solid) = alpha_census(&rgba);
        eprintln!(
            "snap.png written: {}x{}, alpha clear {clear} / partial {partial} / solid {solid} px",
            size.0, size.1
        );
        Ok(())
    }

    /// The frame as drawn (eye and whispers), read back as premultiplied RGBA.
    fn grab(&self, gpu: &mut Gpu, state: &EyeState) -> Vec<u8> {
        let size = gpu.size();
        let scale = gpu.window.scale_factor() as f32;
        eye::read_frame(
            &gpu.device,
            &gpu.queue,
            &gpu.eye,
            gpu.config.format,
            size,
            state,
            |encoder, view| {
                gpu.whispers.draw(
                    &gpu.device,
                    &gpu.queue,
                    encoder,
                    view,
                    size,
                    scale,
                    state.colors,
                    state.fade,
                )
            },
        )
    }
}

impl ApplicationHandler for App {
    /// Called once the event loop is ready for windows (on desktop: at start).
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let order = match self.args.backend {
            Some(b) => vec![b],
            None => vec![Backend::Dx12, Backend::Vulkan],
        };
        // First look for a backend that can be see-through; if none can,
        // settle for an opaque eye rather than none at all.
        for allow_opaque in [false, true] {
            for &backend in &order {
                match Gpu::new(event_loop, backend, allow_opaque, &self.prefs) {
                    Ok(gpu) => {
                        let _ = gpu.window.set_cursor_hittest(!self.prefs.click_through);
                        self.gpu = Some(gpu);
                        // Present a transparent frame, then show the window:
                        // a window shown before its first frame flashes black.
                        self.appear();
                        // Without a tray the eye still works; it's just harder to reach.
                        self.tray = Tray::new(&self.prefs)
                            .map_err(|err| eprintln!("no tray icon: {err}"))
                            .ok();
                        if self.args.demo {
                            // The script plays the senses; the camera and mic stay off.
                            self.demo = Some(demo::Demo::new());
                        } else {
                            self.set_senses(!self.prefs.blind);
                        }
                        if let Some(gpu) = &self.gpu {
                            let window = gpu.window.clone();
                            self.refresh_monitors(&window);
                        }
                        if self.args.senses {
                            self.toggle_senses(event_loop);
                        }
                        if self.args.camera {
                            self.toggle_picker(event_loop);
                        }
                        return;
                    }
                    Err(err) => eprintln!("unblinking: {backend:?} failed: {err}"),
                }
            }
        }
        eprintln!("unblinking: no GPU backend worked");
        event_loop.exit();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // Events for "Where's my camera?" go to the picker.
        if let (Some(picker), Some(gpu)) = (&mut self.picker, &self.gpu)
            && picker.id() == id
        {
            match event {
                WindowEvent::CloseRequested => self.picker = None,
                WindowEvent::RedrawRequested => {}
                other => picker.on_event(&gpu.device, &other),
            }
            return;
        }
        // Events for Blinki's Senses go to the panel.
        if let (Some(panel), Some(gpu)) = (&mut self.panel, &self.gpu)
            && panel.id() == id
        {
            match event {
                WindowEvent::CloseRequested => self.toggle_senses(event_loop),
                // It's drawn right after each frame of the eye instead.
                WindowEvent::RedrawRequested => {}
                other => panel.on_event(&gpu.device, &other),
            }
            return;
        }
        match event {
            WindowEvent::CloseRequested => self.close(AfterClose::Quit, event_loop),
            // Drag the eye anywhere with the left button.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(gpu) = &self.gpu {
                    let _ = gpu.window.drag_window();
                }
            }
            // Right-click opens the same menu as the tray icon.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                if let (Some(gpu), Some(tray)) = (&self.gpu, &self.tray) {
                    tray.popup(&gpu.window);
                }
            }
            // Remember where it was dropped, once the dragging stops.
            WindowEvent::Moved(_) => self.save_move_at = Some(Instant::now() + SAVE_MOVE_AFTER),
            WindowEvent::RedrawRequested => {
                let snap_due = self.args.snap.is_some_and(|d| self.started.elapsed() >= d);
                // `take()` moves the Gpu out of the Option for a moment, so we
                // can borrow it and `self` at the same time; we put it back after.
                let Some(mut gpu) = self.gpu.take() else {
                    return;
                };
                let state = self.step(&gpu.window);
                if let Some(text) = self.pending_whisper.take() {
                    self.voice.effect(Effect::Chime);
                    gpu.whispers
                        .show(&text, School::of_mood(&self.moods.current().name));
                }
                if self.presence == Presence::Open
                    && let Some(text) = self.args.whisper.take()
                {
                    gpu.whispers
                        .show(&text, School::of_mood(&self.moods.current().name));
                }
                // Back from a full-screen app: one jealous line, once it's open.
                if self.presence == Presence::Open
                    && let Some(text) = self.fullscreen.take_line()
                    && self.prefs.whispers
                    && !self.prefs.blind
                {
                    gpu.whispers
                        .show(text, School::of_mood(&self.moods.current().name));
                    self.voice.say(text);
                }
                // While hidden (still drawing for the egui windows), skip the eye.
                if self.presence != Presence::Hidden {
                    gpu.render(&state);
                }
                // --record: keep this frame, as drawn.
                if self.recording.as_ref().is_some_and(|r| r.wants_frame()) {
                    let size = gpu.size();
                    let rgba = self.grab(&mut gpu, &state);
                    if let Some(r) = &mut self.recording {
                        r.add(size, rgba);
                    }
                }
                // --demo: when the performance is over, close the eye (and quit).
                if self.demo.as_ref().is_some_and(|d| d.done()) && self.presence == Presence::Open {
                    self.close(AfterClose::Quit, event_loop);
                }
                // The camera picker too, saving each move as it's made.
                if let Some(picker) = &mut self.picker {
                    let colors = self.moods.now().display_colors();
                    match picker.draw(
                        &gpu.device,
                        &gpu.queue,
                        &self.monitors,
                        self.prefs.camera.as_ref(),
                        &colors,
                    ) {
                        Picked::Nothing => {}
                        Picked::Moved(spot) => {
                            self.prefs.camera = spot;
                            self.prefs.save();
                        }
                        Picked::Close => self.picker = None,
                    }
                }
                // Blinki's Senses follows along, one picture per eye frame.
                if let (Some(panel), Some(snap)) = (&mut self.panel, &self.snapshot)
                    && let Some(sound) = panel.draw(&gpu.device, &gpu.queue, snap)
                {
                    self.prefs.sound = sound;
                    self.apply_sound();
                    self.prefs.save();
                    if let Some(tray) = &self.tray {
                        tray.refresh(&self.prefs, self.presence.shown());
                    }
                }
                if snap_due {
                    if let Err(err) = self.snap(&mut gpu, &state) {
                        eprintln!("snapshot failed: {err}");
                    }
                    event_loop.exit();
                }
                self.advance_presence(&mut gpu, event_loop);
                self.gpu = Some(gpu);
            }
            _ => {}
        }
    }

    /// After each batch of events: poll the tray and friends, then decide
    /// when the next frame is due and sleep until then (see pacing.rs).
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // `map_or_else(Vec::new, ..)`: no tray, no clicks.
        let actions = self.tray.as_ref().map_or_else(Vec::new, Tray::poll);
        for action in actions {
            self.act(action, event_loop);
        }
        let called = self.wake.as_ref().map_or_else(Vec::new, Wake::poll);
        for event in called {
            self.on_wake(event);
        }
        // Launched again: show the eye (as Electron's 'second-instance').
        if self.instance.as_ref().is_some_and(|i| i.woken()) && !self.presence.shown() {
            self.appear();
        }
        // Full screen: step aside (hiding, not quitting, and no pref saved),
        // then come back once it's over. The watcher itself asks Windows
        // only about once a second.
        match self
            .fullscreen
            .tick(self.prefs.hide_fullscreen, self.presence.shown())
        {
            Some(fullscreen::Do::Hide) => self.close(AfterClose::Hide, event_loop),
            Some(fullscreen::Do::Return) => self.appear(),
            None => {}
        }
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == HotKeyState::Pressed {
                self.toggle_senses(event_loop);
            }
        }
        if self.save_move_at.is_some_and(|t| Instant::now() >= t) {
            self.save_move_at = None;
            if let Some(gpu) = &self.gpu
                && let Ok(pos) = gpu.window.outer_position()
            {
                let logical = pos.to_logical::<f64>(gpu.window.scale_factor());
                self.prefs.position = Some(Position {
                    x: logical.x,
                    y: logical.y,
                });
                self.prefs.save();
            }
        }
        self.pace(event_loop);
    }
}

/// Saves the frame as a PNG over a dark backdrop. The pixels arrive
/// premultiplied, so "over" is just `colour + backdrop * (1 - alpha)`.
fn write_snap(path: &str, (w, h): (u32, u32), rgba: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for px in rgba.as_chunks::<4>().0 {
        let keep = 255 - px[3] as u32;
        for i in 0..3 {
            let v = px[i] as u32 + (SNAP_BACKDROP[i] as u32 * keep + 127) / 255;
            rgb.push(v.min(255) as u8);
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgb)?;
    Ok(())
}

/// How many pixels are fully clear, partly clear and solid. A healthy frame
/// has all three: clear corners, a glowing fringe, a solid sphere.
fn alpha_census(rgba: &[u8]) -> (usize, usize, usize) {
    let mut counts = (0, 0, 0);
    for px in rgba.as_chunks::<4>().0 {
        match px[3] {
            0 => counts.0 += 1,
            255 => counts.2 += 1,
            _ => counts.1 += 1,
        }
    }
    counts
}

fn main() -> Result<(), Box<dyn Error>> {
    // --bench-face: time the face model on a blank 1280x720 frame, then quit.
    if std::env::args().any(|a| a == "--bench-face") {
        let mut net = YuNet::new().map_err(|e| e.to_string())?;
        let frame = image::RgbImage::new(1280, 720);
        net.detect(&frame); // warm up
        let start = Instant::now();
        for _ in 0..50 {
            net.detect(&frame);
        }
        eprintln!(
            "face model: {:.1} ms per frame",
            start.elapsed().as_secs_f64() * 1000.0 / 50.0
        );
        return Ok(());
    }
    let args = Args::parse();
    // One eye at a time. A second launch wakes the first and leaves. Runs
    // with --snap are tools, so they don't take part.
    let instance = if args.snap.is_some() {
        None
    } else {
        match session::SingleInstance::claim() {
            Some(instance) => Some(instance),
            None => {
                eprintln!("unblinking is already running; showing it");
                return Ok(());
            }
        }
    };
    let event_loop = EventLoop::new()?;
    // For now the mind's whispers go to the terminal; the app also notes the
    // latest one, because some moods follow what was just said.
    let last_said = Rc::new(Cell::new(None));
    let heard = Rc::clone(&last_said);
    let whisper_log = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&whisper_log);
    let say: mind::Say = Box::new(move |text, kind| {
        eprintln!("whisper ({kind:?}): {text}");
        heard.set(Some((kind, Instant::now())));
        log.borrow_mut().push((Instant::now(), kind, text));
    });
    // Ctrl+Alt+B opens Blinki's Senses from anywhere. If another app has
    // the shortcut, the tray item still works.
    let hotkeys = GlobalHotKeyManager::new().ok().filter(|m| {
        let key = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyB);
        m.register(key)
            .map_err(|err| eprintln!("no Ctrl+Alt+B: {err}"))
            .is_ok()
    });
    let now = Instant::now();
    let mut prefs = Prefs::load();
    // --blind blinds this run only; the saved pref is untouched unless the
    // menu changes something.
    prefs.blind |= args.blind;
    if args.voice {
        // --voice: everything audible, this run only.
        prefs.sound = sound::Sound {
            master: true,
            music: true,
            voice: true,
            fx: true,
            ..prefs.sound
        };
    }
    let moods = Moods::of_theme(prefs.theme.as_deref());
    let book = prefs
        .remember
        .then(|| Remembrance::load(remembrance::unix_now()));
    let mut app = App {
        args,
        gpu: None,
        started: now,
        last_frame: now,
        next_frame: now,
        frames: 0,
        mind: Mind::new(say),
        motion: motion::Motion::new(),
        mode: None,
        eyes: None,
        ears: None,
        wake: None,
        prefs,
        tray: None,
        presence: Presence::Hidden,
        save_move_at: None,
        last_beat: now,
        moods,
        feeling: Feeling::new(),
        book,
        watch: Watch::new(),
        expression: None,
        flow: 0.0,
        last_said,
        last_face: None,
        last_noise: now,
        px_per_m: {
            let ppm = desktop::pixels_per_metre();
            eprintln!("screen: {ppm:.0} pixels per metre");
            ppm
        },
        panel: None,
        picker: None,
        monitors: Vec::new(),
        snapshot: None,
        levels: VecDeque::new(),
        flinch_marks: VecDeque::new(),
        last_startle: f64::NEG_INFINITY,
        whisper_log,
        voice: Voice::new(),
        spoken: 0,
        pending_whisper: None,
        last_flinch_sound: f64::NEG_INFINITY,
        demo: None,
        recording: None,
        up_late: 0.0,
        last_here: now,
        last_worry: None,
        _hotkeys: hotkeys,
        instance,
        fullscreen: fullscreen::Watcher::new(),
    };
    app.apply_sound();
    app.recording = app.args.record.clone().map(record::Recording::new);
    if app.recording.is_some() {
        // A GIF has no sound, so a recording plays none (and saves nothing).
        app.prefs.sound.master = false;
        app.apply_sound();
    }
    event_loop.run_app(&mut app)?;
    if let Some(recording) = app.recording.take() {
        recording.finish()?;
    }
    Ok(())
}
