//! The wgpu half of video rendering: one render pipeline plus, per on-screen
//! tile, the three `R8Unorm` plane textures its I420 frame is uploaded into.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use iced::wgpu;
use iced::widget::shader::Pipeline;
use livekit::webrtc::video_frame::{I420Buffer, VideoBuffer};

use super::video_sink::Upload;

/// Matches `Uniforms` in `yuv.wgsl`. Exactly 16 bytes, which is the alignment
/// a WGSL uniform block needs, so no explicit padding is required.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    scale: [f32; 2],
    srgb: u32,
    rotation: u32,
}

const UNIFORM_SIZE: u64 = 16;
const _: () = assert!(size_of::<Uniforms>() == 16);

/// The three plane textures for one frame size, plus the bind group that
/// references their views.
///
/// The bind group holds the views, so a resolution change has to rebuild both
/// together — recreating only the textures would leave the bind group pointing
/// at the old ones.
struct Planes {
    size: (u32, u32),
    bind_group: wgpu::BindGroup,
    y: wgpu::Texture,
    u: wgpu::Texture,
    v: wgpu::Texture,
}

/// GPU state for one video widget on screen.
///
/// iced keeps a single pipeline per primitive *type* and runs every
/// primitive's `prepare` before any `draw`, so state shared across widgets
/// would leave every tile showing whichever frame was uploaded last. Each
/// widget therefore keys its own textures and uniforms.
struct Tile {
    planes: Planes,
    /// Letterbox scale and rotation differ per tile, and the bind group
    /// references the buffer, so it cannot be shared.
    uniforms: wgpu::Buffer,
    /// Id of the frame currently resident in the textures, so repeated redraws
    /// of the same frame skip the upload.
    uploaded: Option<u64>,
    /// Touched by `prepare`; cleared by `trim`. A tile nobody prepared this
    /// frame is gone from the screen and its textures can go.
    used: bool,
}

pub struct VideoPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    tiles: HashMap<String, Tile>,
    srgb: bool,
}

impl Pipeline for VideoPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video.yuv.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("yuv.wgsl"))),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video.yuv.bind_group_layout"),
            entries: &[
                plane_entry(0),
                plane_entry(1),
                plane_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    // The vertex stage reads scale/rotation, the fragment
                    // stage reads the srgb flag.
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video.yuv.pipeline_layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video.yuv.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // The shader premultiplies, and this matches what the
                    // rest of iced draws with.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // No culling: the fullscreen triangle's winding is
                // whatever the vertex index maths produces.
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video.yuv.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            pipeline,
            layout,
            sampler,
            tiles: HashMap::new(),
            srgb: format.is_srgb(),
        }
    }

    /// Runs once after every rendered frame. Anything not prepared since the
    /// last trim is off screen — a participant who left, or every tile when
    /// the mosaic is switched off — so its textures are released here.
    fn trim(&mut self) {
        self.tiles
            .retain(|_, tile| std::mem::replace(&mut tile.used, false));
    }
}

impl VideoPipeline {
    /// Uploads the frame into its tile and refreshes that tile's uniforms.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, upload: Upload<'_>) {
        let Upload {
            key,
            buffer,
            frame_id,
            rotation,
            bounds,
        } = upload;
        let size = (buffer.width(), buffer.height());

        if !self.tiles.contains_key(key) {
            let uniforms = create_uniforms(device);
            let planes = allocate(device, &self.layout, &self.sampler, &uniforms, size, buffer);

            self.tiles.insert(
                key.to_owned(),
                Tile {
                    planes,
                    uniforms,
                    uploaded: None,
                    used: false,
                },
            );
        }

        let Some(tile) = self.tiles.get_mut(key) else {
            return;
        };

        tile.used = true;

        if tile.planes.size != size {
            tile.planes = allocate(
                device,
                &self.layout,
                &self.sampler,
                &tile.uniforms,
                size,
                buffer,
            );
            // Sizes changed, so whatever was resident is gone.
            tile.uploaded = None;
        }

        if tile.uploaded != Some(frame_id) {
            let (data_y, data_u, data_v) = buffer.data();
            let (stride_y, stride_u, stride_v) = buffer.strides();
            let chroma = (buffer.chroma_width(), buffer.chroma_height());

            write_plane(queue, &tile.planes.y, data_y, stride_y, size);
            write_plane(queue, &tile.planes.u, data_u, stride_u, chroma);
            write_plane(queue, &tile.planes.v, data_v, stride_v, chroma);

            tile.uploaded = Some(frame_id);
        }

        // A quarter turn shows the frame's height across the widget's width.
        let displayed = if rotation.is_multiple_of(2) {
            size
        } else {
            (size.1, size.0)
        };

        queue.write_buffer(
            &tile.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                scale: letterbox(bounds, displayed),
                srgb: u32::from(self.srgb),
                rotation: rotation % 4,
            }),
        );
    }

    /// Draws `key`'s tile. `false` when nothing was uploaded for it, which
    /// `prepare` allows for degenerate bounds or an empty buffer.
    pub fn draw(&self, key: &str, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(tile) = self.tiles.get(key) else {
            return false;
        };

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &tile.planes.bind_group, &[]);
        render_pass.draw(0..3, 0..1);

        true
    }
}

fn create_uniforms(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("video.yuv.uniforms"),
        size: UNIFORM_SIZE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn allocate(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    uniforms: &wgpu::Buffer,
    size: (u32, u32),
    buffer: &I420Buffer,
) -> Planes {
    let chroma = (buffer.chroma_width(), buffer.chroma_height());

    let y = create_plane(device, "video.yuv.plane.y", size);
    let u = create_plane(device, "video.yuv.plane.u", chroma);
    let v = create_plane(device, "video.yuv.plane.v", chroma);

    let view =
        |texture: &wgpu::Texture| texture.create_view(&wgpu::TextureViewDescriptor::default());

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("video.yuv.bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view(&y)),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&view(&u)),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&view(&v)),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: uniforms.as_entire_binding(),
            },
        ],
    });

    Planes {
        size,
        bind_group,
        y,
        u,
        v,
    }
}

const fn plane_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn create_plane(device: &wgpu::Device, label: &str, (width, height): (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Copies one plane straight out of the decoded frame buffer.
///
/// `stride` is passed through as `bytes_per_row`, so a padded plane is uploaded
/// without repacking it first. `write_texture` — unlike `copy_buffer_to_texture`
/// — imposes no 256-byte row alignment.
fn write_plane(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    data: &[u8],
    stride: u32,
    (width, height): (u32, u32),
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(stride),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// "Contain" fit: 1.0 on the axis that fills the widget, < 1.0 on the axis with
/// spare room so the shader's UV runs outside [0, 1] there and leaves a bar.
///
/// Callers guarantee non-degenerate inputs, so the divisions below are safe.
fn letterbox(bounds: iced::Rectangle, (width, height): (u32, u32)) -> [f32; 2] {
    let video = ratio(width, height);
    let widget = bounds.width / bounds.height;

    if video > widget {
        [1.0, widget / video]
    } else {
        [video / widget, 1.0]
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "u32 has no lossless f32 conversion in std, but video extents are \
              far below f32's 2^24 exact-integer limit"
)]
fn ratio(width: u32, height: u32) -> f32 {
    width as f32 / height as f32
}

#[cfg(test)]
mod tests {
    /// The shader module is only built the first time a frame reaches the GPU,
    /// so a WGSL mistake would otherwise show up as a blank widget mid-call
    /// rather than as a build failure.
    #[test]
    fn shader_is_valid_wgsl() {
        let source = include_str!("yuv.wgsl");

        let parsed = naga::front::wgsl::parse_str(source);
        assert!(
            parsed.is_ok(),
            "yuv.wgsl does not parse: {:?}",
            parsed.as_ref().err()
        );

        let Ok(module) = parsed else { return };

        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );

        let validated = validator.validate(&module);
        assert!(
            validated.is_ok(),
            "yuv.wgsl does not validate: {:?}",
            validated.as_ref().err()
        );
    }
}
