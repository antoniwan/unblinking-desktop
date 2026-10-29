//! A normal desktop window drawn with egui on the eye's GPU device: the
//! plumbing shared by Blinki's Senses and the camera picker.

use std::error::Error;
use std::sync::Arc;

use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

pub struct EguiWindow {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pub ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    /// Debugging and README aid: with UNBLINKING_WINDOW_SNAP=MS set, the
    /// frame this window presents at MS is saved as "<title>.png". It reads
    /// back the window's own pixels, so nothing else on screen can get in.
    snap: Option<(std::time::Instant, String)>,
}

impl EguiWindow {
    pub fn new(
        event_loop: &ActiveEventLoop,
        gpu: (&wgpu::Instance, &wgpu::Adapter, &wgpu::Device),
        title: &str,
        size: (f64, f64),
        min: (f64, f64),
    ) -> Result<EguiWindow, Box<dyn Error>> {
        let (instance, adapter, device) = gpu;
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(size.0, size.1))
            .with_min_inner_size(winit::dpi::LogicalSize::new(min.0, min.1));
        let window = Arc::new(event_loop.create_window(attrs)?);
        let surface = instance.create_surface(window.clone())?;
        let caps = surface.get_capabilities(adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let inner = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | (caps.usages & wgpu::TextureUsages::COPY_SRC),
            format,
            width: inner.width.max(1),
            height: inner.height.max(1),
            // No vsync wait here: the main loop paces frames (pacing.rs),
            // and waiting on vsync twice per frame would halve the rate.
            present_mode: wgpu::PresentMode::AutoNoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            color_space: Default::default(),
            view_formats: vec![],
        };
        surface.configure(device, &config);

        let ctx = egui::Context::default();
        ctx.set_visuals(egui::Visuals::dark());
        ctx.set_fonts(crate::fonts::blinki());
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let renderer = egui_wgpu::Renderer::new(device, format, Default::default());
        let snap = std::env::var("UNBLINKING_WINDOW_SNAP")
            .ok()
            .and_then(|ms| ms.parse().ok())
            .map(|ms| {
                let at = std::time::Instant::now() + std::time::Duration::from_millis(ms);
                let name: String = title
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == ' ')
                    .collect();
                (
                    at,
                    format!("{}.png", name.trim().replace(' ', "-").to_lowercase()),
                )
            });
        Ok(EguiWindow {
            snap,
            window,
            surface,
            config,
            ctx,
            state,
            renderer,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Passes a window event to egui; resizes the surface when needed.
    pub fn on_event(&mut self, device: &wgpu::Device, event: &winit::event::WindowEvent) {
        if let winit::event::WindowEvent::Resized(size) = event {
            self.config.width = size.width.max(1);
            self.config.height = size.height.max(1);
            self.surface.configure(device, &self.config);
        }
        let _ = self.state.on_window_event(&self.window, event);
    }

    /// Runs `ui` for one frame and presents it.
    ///
    /// `impl FnMut(&mut egui::Ui)` takes any closure with that shape, so each
    /// window brings its own drawing code and this handles the rest.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ui: impl FnMut(&mut egui::Ui),
    ) {
        let raw = self.state.take_egui_input(&self.window);
        let out = self.ctx.run_ui(raw, ui);
        self.state
            .handle_platform_output(&self.window, out.platform_output);
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        // Texture changes must all be used: upload the new ones now, and free
        // the old ones once this frame is out (or skipped).
        let mut textures = out.textures_delta;
        for (id, deltas) in textures.set.drain() {
            for delta in &deltas {
                self.renderer.update_texture(device, queue, id, delta);
            }
        }
        let free = std::mem::take(&mut textures.free);
        self.present(device, queue, &prims, out.pixels_per_point);
        for id in &free {
            self.renderer.free_texture(id);
        }
    }

    fn present(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prims: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(device, &self.config);
                return;
            }
            _ => return, // minimised or timed out: skip this frame
        };
        let view = frame.texture.create_view(&Default::default());
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        let extra = self
            .renderer
            .update_buffers(device, queue, &mut encoder, prims, &screen);
        {
            // egui's renderer wants a pass with no borrowed lifetime;
            // `forget_lifetime` trades the compile-time check for a runtime one.
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui window"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, prims, &screen);
        }
        let readback = self
            .snap
            .take_if(|(at, _)| std::time::Instant::now() >= *at)
            .map(|(_, name)| {
                let (w, h) = (self.config.width, self.config.height);
                let row = (w * 4).div_ceil(256) * 256;
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("window snap"),
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
                (buffer, row, name)
            });
        queue.submit(extra.into_iter().chain([encoder.finish()]));
        if let Some((buffer, row, name)) = readback
            && let Err(err) = save_png(
                device,
                &buffer,
                row,
                (self.config.width, self.config.height),
                self.config.format,
                &name,
            )
        {
            eprintln!("window snap failed: {err}");
        }
        self.window.pre_present_notify();
        queue.present(frame);
    }
}

/// Waits for a read-back frame and writes it as an RGB PNG.
fn save_png(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    row: u32,
    (w, h): (u32, u32),
    format: wgpu::TextureFormat,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map window snap"));
    device.poll(wgpu::PollType::wait_indefinitely())?;
    let data = slice.get_mapped_range()?;
    let bgra = matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h as usize {
        // `as_chunks::<4>()` views the bytes as whole 4-byte pixels.
        for px in data[y * row as usize..y * row as usize + w as usize * 4]
            .as_chunks::<4>()
            .0
        {
            rgb.extend_from_slice(&if bgra {
                [px[2], px[1], px[0]]
            } else {
                [px[0], px[1], px[2]]
            });
        }
    }
    let mut png = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(name)?), w, h);
    png.set_color(png::ColorType::Rgb);
    png.write_header()?.write_image_data(&rgb)?;
    eprintln!("{name} written");
    Ok(())
}
