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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
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
        Ok(EguiWindow {
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
        queue.submit(extra.into_iter().chain([encoder.finish()]));
        self.window.pre_present_notify();
        queue.present(frame);
    }
}
