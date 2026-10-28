//! Unblinking, in Rust: the eye in a small transparent window that stays on
//! top of everything. This file owns the window and the frame loop.
//!
//! Flags:
//!   --snap=MS        after MS milliseconds, save the frame to snap.png and quit
//!   --backend=NAME   force `dx12` or `vulkan` instead of trying both
//!   --blind          keep the camera off (the eye dims and drifts)

// In release builds, don't open a console window next to the eye.
// `cfg_attr(condition, attribute)` applies the attribute only when the
// condition holds; `debug_assertions` is on in debug builds only.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Each `mod` line pulls in a file of the same name from src/.
mod desktop;
mod eye;
mod mind;
mod senses;

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId, WindowLevel};

use desktop::{Point, Rect};
use eye::{Eye, EyeState};
use mind::{Mind, Mode, Senses, World};
use senses::Detector;
use senses::camera::Eyes;
use senses::face::YuNet;

/// Window size in logical pixels (scaled by the monitor's DPI), as in main.js.
const WIN_SIZE: f64 = 320.0;
/// Gap from the work area's top right corner, in logical pixels.
const MARGIN: f64 = 24.0;
/// The eye sits a little above center so whispers fit below it.
const CENTER: [f32; 2] = [0.0, 0.06];
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
}

impl Args {
    fn parse() -> Self {
        let mut args = Args {
            snap: None,
            backend: None,
            blind: false,
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
            } else if arg == "--blind" {
                args.blind = true;
            } else {
                eprintln!("ignoring unknown argument {arg:?}");
            }
        }
        args
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
    ) -> Result<Gpu, Box<dyn Error>> {
        let window = Arc::new(event_loop.create_window(window_attributes(event_loop, backend))?);

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

/// The eye's window: frameless, see-through, always on top, off the taskbar,
/// in the top right corner of the primary monitor's work area.
fn window_attributes(
    event_loop: &ActiveEventLoop,
    backend: Backend,
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
    if let Some(area) = desktop::work_area() {
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
    last_beat: Instant,
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

        // What the camera saw last. No mic yet, so it hears silence.
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
                level: 0.0,
                world: Self::world(window),
                blind: self.args.blind,
                whispers: true,
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
            eprintln!(
                "mode={:?} sight={can_see} faces={}{first}",
                s.mode,
                faces.len()
            );
        }

        // A line in the terminal whenever the mood changes.
        if self.mode != Some(s.mode) {
            eprintln!("mode: {:?}", s.mode);
            self.mode = Some(s.mode);
        }

        let secs = self.started.elapsed().as_secs_f32();
        EyeState {
            flow: (secs / 4.0) % 1.0,
            gaze: [s.gaze.x as f32, s.gaze.y as f32],
            pupil: s.pupil as f32,
            heat: s.heat as f32,
            flare: s.flare as f32,
            wide: s.wide as f32,
            dim: s.dim as f32,
            center: [CENTER[0] + s.shake.x as f32, CENTER[1] + s.shake.y as f32],
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
                match Gpu::new(event_loop, backend, allow_opaque) {
                    Ok(gpu) => {
                        // A hidden window gets no redraw events, so show it
                        // now; the surface is set up and starts out clear.
                        gpu.window.set_visible(true);
                        gpu.window.request_redraw();
                        self.gpu = Some(gpu);
                        if !self.args.blind {
                            self.eyes = Some(Eyes::open(|| {
                                let net = YuNet::new().map_err(|e| e.to_string())?;
                                Ok(Box::new(net) as Box<dyn Detector>)
                            }));
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
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
            // No tray menu yet, so a right click closes it.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => event_loop.exit(),
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
                self.gpu = Some(gpu);
            }
            _ => {}
        }
    }

    /// After each batch of events: ask for the next frame. With vsync on,
    /// presenting blocks until the screen is ready, which paces the loop.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
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
    // For now the mind's whispers go to the terminal.
    let say: mind::Say = Box::new(|text, kind| eprintln!("whisper ({kind:?}): {text}"));
    let now = Instant::now();
    let mut app = App {
        args: Args::parse(),
        gpu: None,
        started: now,
        last_frame: now,
        mind: Mind::new(say),
        mode: None,
        eyes: None,
        last_beat: now,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
