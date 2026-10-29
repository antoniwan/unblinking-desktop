//! The eye's window and everything on the GPU that draws into it.

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

use crate::cli::Backend;
use crate::desktop::{self};
use crate::eye::{Eye, EyeState};
use crate::fonts;
use crate::prefs::{Position, Prefs};
use crate::snap::write_snap;
use crate::whisper::Whispers;

/// Window size in logical pixels (scaled by the monitor's DPI), as in main.js.
const WIN_SIZE: f64 = 320.0;
/// Gap from the work area's top right corner, in logical pixels.
const MARGIN: f64 = 24.0;

/// The window and everything on the GPU that draws into it.
pub(crate) struct Gpu {
    // `Arc` is a shared, reference-counted pointer. The surface needs the
    // window to live as long as it does, and so do we; an Arc lets both
    // hold it, and the window is freed only when the last Arc goes away.
    pub(crate) window: Arc<Window>,
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) config: wgpu::SurfaceConfiguration,
    pub(crate) eye: Eye,
    pub(crate) whispers: Whispers,
    /// Debugging aid: with UNBLINKING_SURFACE_SNAP=MS set, the frame actually
    /// presented at MS is copied to surface.png.
    surface_snap: Option<Instant>,
    // Kept so another window (Blinki's Senses) can draw with the same GPU.
    pub(crate) instance: wgpu::Instance,
    pub(crate) adapter: wgpu::Adapter,
}

impl Gpu {
    /// Opens the window and sets up `backend` to draw into it.
    ///
    /// Returns `Result`: either `Ok(Gpu)` or `Err(reason)`. The `?` after a
    /// call means "if this failed, return its error from here right away".
    /// `Box<dyn Error>` is "any kind of error", handy at the top of an app.
    pub(crate) fn new(
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

    pub(crate) fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Presents one fully transparent frame. Done before the window is shown
    /// and before it hides, so it never flashes an empty (black) surface.
    pub(crate) fn clear(&mut self) {
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
    pub(crate) fn render(&mut self, state: &EyeState) {
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
