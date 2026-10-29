//! The eye itself: one full-window triangle and one fragment shader,
//! drawn with wgpu. This is the Rust side of renderer/eye.js.

use std::borrow::Cow;

/// What the eye looks like this frame. Same fields as `eye.render(s)` in eye.js.
///
/// `#[derive(Clone, Copy)]` lets a value be copied bit for bit instead of
/// *moved*. Rust normally moves values (the old variable can't be used
/// afterwards); small plain-data types opt into copying like this.
#[derive(Clone, Copy, Debug)]
pub struct EyeState {
    pub flow: f32,
    pub gaze: [f32; 2],
    pub pupil: f32,
    pub heat: f32,
    pub flare: f32,
    pub wide: f32,
    pub dim: f32,
    pub center: [f32; 2],
    /// Whole-eye opacity, 0..1: the app's opening and closing fades.
    pub fade: f32,
    /// Whole-eye scale: below 1 while it opens or closes.
    pub size: f32,
    /// The mood's palette: deep fire, flame, hot core, glow.
    pub colors: [[f32; 3]; 4],
    /// How finely the flames swirl.
    pub swirl: f32,
    /// How far the glow reaches.
    pub glow: f32,
    /// How many embers rise.
    pub embers: f32,
    /// Seconds since start, for effects that needn't loop.
    pub time: f32,
    /// A slow breath, -1..1 (inhale above 0).
    pub breath: f32,
    /// 0 is the perfect resting sphere; above 0 the body may deform.
    pub morph: f32,
    /// How fast the gaze and the pupil are moving, per second.
    pub gaze_vel: [f32; 2],
    pub pupil_vel: f32,
    /// 0..1, how much work the shader's raymarches may do.
    pub quality: f32,
    /// 0..1, a flinch: the body tightens and the fire recoils.
    pub wince: f32,
    /// Radians: the sphere's slow roll about the view axis.
    pub roll: f32,
}

/// The uniform block exactly as the GPU sees it (see `Params` in eye.frag).
///
/// `#[repr(C)]` fixes the field order and padding the way C would lay them
/// out, so the bytes line up with the shader. bytemuck's `Pod` ("plain old
/// data") lets us view the struct as a `&[u8]` safely.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    a: [f32; 4],
    b: [f32; 4],
    c: [f32; 4],
    d: [f32; 4],
    e: [f32; 4],
    f: [f32; 4],
    g: [f32; 4],
    h: [f32; 4],
    i: [f32; 4],
    j: [f32; 4],
}

impl Uniforms {
    fn new(size: (u32, u32), s: &EyeState) -> Self {
        Uniforms {
            a: [size.0 as f32, size.1 as f32, s.flow, s.pupil],
            b: [s.gaze[0], s.gaze[1], s.center[0], s.center[1]],
            c: [s.heat, s.flare, s.wide, s.dim],
            d: [s.fade, s.size, s.time, s.breath],
            e: with(s.colors[0], s.swirl),
            f: with(s.colors[1], s.glow),
            g: with(s.colors[2], s.embers),
            h: with(s.colors[3], s.morph),
            i: [s.gaze_vel[0], s.gaze_vel[1], s.pupil_vel, s.quality],
            j: [s.wince, s.roll, 0.0, 0.0],
        }
    }
}

/// A colour plus one more number, packed into a vec4 slot.
fn with([r, g, b]: [f32; 3], w: f32) -> [f32; 4] {
    [r, g, b, w]
}

/// The fragment shader's layers, in the order they're pasted together.
const LAYERS: [(&str, &str); 4] = [
    ("common.glsl", include_str!("shaders/common.glsl")),
    ("body.glsl", include_str!("shaders/body.glsl")),
    ("aura.glsl", include_str!("shaders/aura.glsl")),
    ("eye.frag", include_str!("shaders/eye.frag")),
];

/// The whole fragment shader. The layers are built into the exe, but if
/// `UNBLINKING_SHADERS` names a folder, they're read from there instead, so
/// a shader can be tuned by rerunning the exe without rebuilding it.
pub fn shader_source() -> String {
    let dir = std::env::var_os("UNBLINKING_SHADERS").map(std::path::PathBuf::from);
    LAYERS
        .iter()
        .map(|(name, built_in)| match &dir {
            Some(dir) => std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| {
                eprintln!("shader {name}: {e}; using the built-in one");
                built_in.to_string()
            }),
            None => built_in.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Everything needed to draw the eye into a texture of one format.
pub struct Eye {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl Eye {
    /// Builds the pipeline for render targets of `format`.
    ///
    /// `&wgpu::Device` is a *borrow*: we use the device without taking
    /// ownership of it, so the caller still has it when we return.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let glsl = |src: Cow<'static, str>, stage| wgpu::ShaderSource::Glsl {
            shader: src,
            stage,
            defines: &[],
        };
        // include_str! pastes the file into the binary at compile time,
        // so the exe needs no shader files next to it.
        let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("eye.vert"),
            source: glsl(
                Cow::Borrowed(include_str!("shaders/eye.vert")),
                wgpu::naga::ShaderStage::Vertex,
            ),
        });
        let fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("eye.frag"),
            source: glsl(
                Cow::Owned(shader_source()),
                wgpu::naga::ShaderStage::Fragment,
            ),
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("eye uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("eye"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("eye"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("eye"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("eye"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fs,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                // No blending: the shader already writes premultiplied
                // colour and alpha, which is what the compositor wants.
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Eye {
            pipeline,
            uniforms,
            bind_group,
        }
    }

    /// Records one draw of the eye into `target`, cleared to transparent first.
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
        state: &EyeState,
    ) {
        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms::new(size, state)),
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("eye"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
        // `pass` borrows `encoder` mutably; it is dropped here at the end of
        // the function, which ends the render pass and frees the borrow.
    }
}

/// Draws one frame into an offscreen texture and reads its pixels back as
/// tightly packed RGBA8, still premultiplied, top row first.
///
/// It uses the same pipeline and state as the frame on screen, so the
/// picture is what the window shows, minus the desktop behind it.
pub fn read_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    eye: &Eye,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    state: &EyeState,
    // Anything else to draw over the eye (the whispers), into the same frame.
    over: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
) -> Vec<u8> {
    let (w, h) = size;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snap"),
        size: size_3d(w, h),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());

    // GPU copies want each row padded to a multiple of 256 bytes.
    let row = w * 4;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("snap readback"),
        size: (padded * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    eye.draw(queue, &mut encoder, &view, size, state);
    over(&mut encoder, &view);
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        size_3d(w, h),
    );
    queue.submit([encoder.finish()]);

    // Mapping is asynchronous: ask for it, then block until the GPU is done.
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| {
        result.expect("map snapshot buffer")
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("wait for GPU");

    let bgra = matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );
    let data = slice.get_mapped_range().expect("read snapshot buffer");
    let mut out = Vec::with_capacity((row * h) as usize);
    for y in 0..h as usize {
        let start = y * padded as usize;
        for px in data[start..start + row as usize].as_chunks::<4>().0 {
            if bgra {
                out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            } else {
                out.extend_from_slice(px);
            }
        }
    }
    out
}

fn size_3d(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

#[cfg(test)]
mod tests {
    /// The pasted-together shader parses and validates, without a GPU.
    /// With `UNBLINKING_SHADERS` set, it checks the files in that folder.
    #[test]
    fn shader_compiles() {
        use wgpu::naga;
        let mut frontend = naga::front::glsl::Frontend::default();
        let options = naga::front::glsl::Options::from(naga::ShaderStage::Fragment);
        let module = frontend
            .parse(&options, &super::shader_source())
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&super::shader_source())));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    }
}
