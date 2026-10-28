//! Unblinking, in Rust: the eye in a small transparent window that stays on
//! top of everything. This file owns the window and the frame loop.
//!
//! Flags:
//!   --snap=MS        after MS milliseconds, save the frame to snap.png and quit
//!   --backend=NAME   force `dx12` or `vulkan` instead of trying both
//!   --mood=NAME      hold one of the studio's moods (for looking at it, or captures)
//!   --blind          start blinded this run (camera and mic off), without saving it
//!
//! Left-drag moves the eye; right-click it (or the tray icon) for the menu.

// In release builds, don't open a console window next to the eye.
// `cfg_attr(condition, attribute)` applies the attribute only when the
// condition holds; `debug_assertions` is on in debug builds only.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Each `mod` line pulls in a file of the same name from src/.
mod desktop;
mod eye;
mod mind;
mod moods;
mod prefs;
mod senses;
mod tray;

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId, WindowLevel};

use desktop::{Point, Rect};
use eye::{Eye, EyeState};
use std::cell::Cell;
use std::rc::Rc;

use mind::{Line, Mind, Mode, Senses, World};
use moods::Moods;
use prefs::{Position, Prefs};
use senses::Detector;
use senses::camera::Eyes;
use senses::face::YuNet;
use senses::mic::Ears;
use tray::{Action, Tray};

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
}

impl Args {
    fn parse() -> Self {
        let mut args = Args {
            snap: None,
            backend: None,
            blind: false,
            mood: None,
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
            } else if arg == "--blind" {
                args.blind = true;
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
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
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

        let info = adapter.get_info();
        eprintln!(
            "unblinking: {backend:?} on {} ({:?}), alpha {alpha_mode:?} of {:?}, format {format:?}, {}x{}",
            info.name, info.backend, caps.alpha_modes, size.width, size.height
        );
        Ok(Gpu {
            window,
            surface,
            device,
            queue,
            config,
            eye,
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
        self.queue.submit([encoder.finish()]);
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
struct App {
    args: Args,
    gpu: Option<Gpu>,
    started: Instant,
    last_frame: Instant,
    mind: Mind,
    mode: Option<Mode>,
    /// The camera thread, while the eye is allowed to see.
    eyes: Option<Eyes>,
    /// The microphone, while the eye is allowed to hear.
    ears: Option<Ears>,
    prefs: Prefs,
    tray: Option<Tray>,
    presence: Presence,
    /// When to save the window's spot, a moment after the last move.
    save_move_at: Option<Instant>,
    last_beat: Instant,
    moods: Moods,
    /// The flame drift's looping phase, 0..1, advanced at the mood's speed.
    flow: f32,
    /// The latest whisper and when, shared with the mind's `say` closure.
    /// `Rc<Cell<..>>`: both own it (Rc), and a Copy value inside a Cell can
    /// be swapped even through a shared handle.
    last_said: Rc<Cell<Option<(Line, Instant)>>>,
    /// When it last saw a face, and when it last heard anything louder than quiet.
    last_face: Option<Instant>,
    last_noise: Instant,
}

impl App {
    /// Where the cursor, the eye and its monitor are right now.
    fn world(window: &Window) -> Option<World> {
        let cursor = desktop::cursor()?;
        let pos = window.outer_position().ok()?;
        let size = window.inner_size();
        let monitor = window.current_monitor()?;
        let (mp, ms) = (monitor.position(), monitor.size());
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
                (sight.faces.clone(), sight.can_see)
            }
            None => (vec![], false),
        };
        let s = self.mind.update(
            dt,
            &Senses {
                faces: &faces,
                can_see,
                level: self.ears.as_ref().map_or(0.0, Ears::level),
                world: Self::world(window),
                blind: self.prefs.blind,
                whispers: self.prefs.whispers,
            },
        );

        // Debug heartbeat in the terminal, as app.js had: what it sees.
        if self.last_beat.elapsed() >= Duration::from_secs(3) {
            self.last_beat = now;
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
                "mode={:?} mood={} sight={can_see} faces={}{first} level={level:.2?}",
                s.mode,
                self.moods.current().name,
                faces.len()
            );
        }

        // A line in the terminal whenever the mood changes.
        if self.mode != Some(s.mode) {
            eprintln!("mode: {:?}", s.mode);
            self.mode = Some(s.mode);
        }

        // The mood: which of the studio's looks fits what's happening.
        let level = self.ears.as_ref().map_or(0.0, Ears::level);
        if !faces.is_empty() {
            self.last_face = Some(now);
        }
        if level > 0.3 {
            self.last_noise = now;
        }
        // `as_deref` turns `Option<String>` into `Option<&str>` without copying.
        let mood = self.args.mood.as_deref().unwrap_or_else(|| self.choose_mood(&s, &faces));
        self.moods.set(mood);
        let look = self.moods.now();
        self.flow = (self.flow + dt as f32 * look.flow) % 1.0;

        // Opening, the eye grows and fades in with its pupil wide, then the
        // pupil settles to what the mind wants. Closing, it narrows, cools,
        // shrinks and fades to nothing.
        let k = self.presence.amount();
        let closing = matches!(self.presence, Presence::Closing(..));
        let pupil = s.pupil as f32;
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
        EyeState {
            flow: self.flow,
            gaze: [s.gaze.x as f32, s.gaze.y as f32],
            pupil,
            heat: s.heat as f32 * look.heat / 0.55 * cool,
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
        }
    }

    /// Picks the mood from the studio's triggers, most urgent first.
    fn choose_mood(&self, s: &mind::Frame, faces: &[mind::Face]) -> &'static str {
        let said_lately = |kinds: &[Line], secs: f32| {
            self.last_said.get().is_some_and(|(kind, at)| {
                kinds.contains(&kind) && at.elapsed().as_secs_f32() < secs
            })
        };
        let no_face_for = self
            .last_face
            .map_or(self.started.elapsed(), |t| t.elapsed());
        let hour = desktop::local_hour();
        let late = !(6..23).contains(&hour);
        let quiet = self.last_noise.elapsed() > Duration::from_secs(120);
        let too_close = faces.iter().any(|f| f.score > 0.55 && f.size > 0.5);

        if s.mode == Mode::Blind {
            "Blinded"
        } else if said_lately(&[Line::Loud], 3.0) || too_close {
            "Wrathful"
        } else if said_lately(&[Line::Returned, Line::First], 4.0) {
            "Delighted"
        } else if faces.iter().filter(|f| f.score > 0.55).count() > 1 {
            "Suspicious"
        } else if late && quiet {
            "Drowsy"
        } else if s.mode == Mode::Locked {
            "Watchful"
        } else if no_face_for > Duration::from_secs(60) {
            "Lonely"
        } else {
            "Curious"
        }
    }

    /// Opens or closes the camera and mic together. Closing really stops the
    /// devices: setting the Options to `None` drops the Eyes and Ears, and
    /// their `Drop` code lets go of the hardware.
    fn set_senses(&mut self, open: bool) {
        if !open {
            self.eyes = None;
            self.ears = None;
            eprintln!("senses closed");
            return;
        }
        if self.eyes.is_none() {
            self.eyes = Some(Eyes::open(|| {
                let net = YuNet::new().map_err(|e| e.to_string())?;
                Ok(Box::new(net) as Box<dyn Detector>)
            }));
        }
        if self.ears.is_none() {
            // `.map_err(..).ok()` logs a failure and carries on deaf:
            // a missing sense never stops the eye.
            self.ears = Ears::open()
                .map_err(|err| eprintln!("no hearing: {err}"))
                .ok();
        }
    }

    /// Carries out one menu or tray click.
    fn act(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        let Some(gpu) = &self.gpu else { return };
        let window = gpu.window.clone();
        // A match arm can have a guard (`if ...`); arms are tried in order.
        match action {
            Action::Quit => self.close(AfterClose::Quit, event_loop),
            Action::ToggleShown if self.presence.shown() => {
                self.close(AfterClose::Hide, event_loop)
            }
            Action::ToggleShown => self.appear(),
            Action::Blind(on) => {
                self.prefs.blind = on;
                self.set_senses(!on);
            }
            Action::Whispers(on) => self.prefs.whispers = on,
            // Saved now; the voice itself arrives in the next milestone.
            Action::Voice(on) => self.prefs.voice = on,
            Action::ClickThrough(on) => {
                self.prefs.click_through = on;
                let _ = window.set_cursor_hittest(!on);
            }
        }
        if action != Action::ToggleShown && action != Action::Quit {
            self.prefs.save();
        }
        if let Some(tray) = &self.tray {
            tray.refresh(&self.prefs, self.presence.shown());
        }
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

    fn snap(&self, gpu: &Gpu, state: &EyeState) -> Result<(), Box<dyn Error>> {
        let size = gpu.size();
        let rgba = eye::read_frame(
            &gpu.device,
            &gpu.queue,
            &gpu.eye,
            gpu.config.format,
            size,
            state,
        );
        write_snap("snap.png", size, &rgba)?;
        let (clear, partial, solid) = alpha_census(&rgba);
        eprintln!(
            "snap.png written: {}x{}, alpha clear {clear} / partial {partial} / solid {solid} px",
            size.0, size.1
        );
        Ok(())
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
                        self.set_senses(!self.prefs.blind);
                        return;
                    }
                    Err(err) => eprintln!("unblinking: {backend:?} failed: {err}"),
                }
            }
        }
        eprintln!("unblinking: no GPU backend worked");
        event_loop.exit();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
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
                gpu.render(&state);
                if snap_due {
                    if let Err(err) = self.snap(&gpu, &state) {
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

    /// After each batch of events: ask for the next frame. With vsync on,
    /// presenting blocks until the screen is ready, which paces the loop.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // `map_or_else(Vec::new, ..)`: no tray, no clicks.
        let actions = self.tray.as_ref().map_or_else(Vec::new, Tray::poll);
        for action in actions {
            self.act(action, event_loop);
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
        if let Some(gpu) = &self.gpu {
            gpu.window.request_redraw();
        }
    }
}

/// Saves the frame as a PNG over a dark backdrop. The pixels arrive
/// premultiplied, so "over" is just `colour + backdrop * (1 - alpha)`.
fn write_snap(path: &str, (w, h): (u32, u32), rgba: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for px in rgba.chunks_exact(4) {
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
    for px in rgba.chunks_exact(4) {
        match px[3] {
            0 => counts.0 += 1,
            255 => counts.2 += 1,
            _ => counts.1 += 1,
        }
    }
    counts
}

fn main() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    // For now the mind's whispers go to the terminal; the app also notes the
    // latest one, because some moods follow what was just said.
    let last_said = Rc::new(Cell::new(None));
    let heard = Rc::clone(&last_said);
    let say: mind::Say = Box::new(move |text, kind| {
        eprintln!("whisper ({kind:?}): {text}");
        heard.set(Some((kind, Instant::now())));
    });
    let now = Instant::now();
    let args = Args::parse();
    let mut prefs = Prefs::load();
    // --blind blinds this run only; the saved pref is untouched unless the
    // menu changes something.
    prefs.blind |= args.blind;
    let mut app = App {
        args,
        gpu: None,
        started: now,
        last_frame: now,
        mind: Mind::new(say),
        mode: None,
        eyes: None,
        ears: None,
        prefs,
        tray: None,
        presence: Presence::Hidden,
        save_move_at: None,
        last_beat: now,
        moods: Moods::load(),
        flow: 0.0,
        last_said,
        last_face: None,
        last_noise: now,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
