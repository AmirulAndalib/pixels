use crate::{ScalingMode, SurfaceSize};
use ultraviolet::Mat4;
use wgpu::util::DeviceExt;

/// The default renderer that scales your frame to the screen size.
#[derive(Debug)]
pub struct ScalingRenderer {
    vertex_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group_nearest: wgpu::BindGroup,
    bind_group_linear: wgpu::BindGroup,
    render_pipeline: wgpu::RenderPipeline,
    render_pipeline_fill: wgpu::RenderPipeline,
    pub(crate) clear_color: wgpu::Color,
    width: f32,
    height: f32,
    clip_rect: (u32, u32, u32, u32),
    pub(crate) scaling_mode: ScalingMode,
}

impl ScalingRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        device: &wgpu::Device,
        texture_view: &wgpu::TextureView,
        texture_size: &wgpu::Extent3d,
        surface_size: &SurfaceSize,
        render_texture_format: wgpu::TextureFormat,
        clear_color: wgpu::Color,
        blend_state: wgpu::BlendState,
        scaling_mode: ScalingMode,
    ) -> Self {
        let shader = wgpu::include_wgsl!("../shaders/scale.wgsl");
        let module = device.create_shader_module(shader);

        let shader_fill = wgpu::include_wgsl!("../shaders/scale_fill.wgsl");
        let module_fill = device.create_shader_module(shader_fill);

        // Create a texture sampler with nearest neighbor
        let sampler_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pixels_scaling_renderer_sampler_nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 1.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });

        let sampler_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pixels_scaling_renderer_sampler_linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: 1.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });

        // Create vertex buffer; array-of-array of position and texture coordinates
        let vertex_data: [[f32; 2]; 3] = [
            // One full-screen triangle
            // See: https://github.com/parasyte/pixels/issues/180
            [-1.0, -1.0],
            [3.0, -1.0],
            [-1.0, 3.0],
        ];
        let vertex_data_slice = bytemuck::cast_slice(&vertex_data);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pixels_scaling_renderer_vertex_buffer"),
            contents: vertex_data_slice,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: (vertex_data_slice.len() / vertex_data.len()) as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            }],
        };

        // Create uniform buffer
        let matrix = ScalingMatrix::new(
            (texture_size.width as f32, texture_size.height as f32),
            (surface_size.width as f32, surface_size.height as f32),
            scaling_mode,
        );
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pixels_scaling_renderer_matrix_uniform_buffer"),
            contents: matrix.uniform_buffer.as_slice(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create bind group
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pixels_scaling_renderer_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(matrix.uniform_buffer.len() as u64),
                    },
                    count: None,
                },
            ],
        });
        let bind_group_nearest = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixels_scaling_renderer_bind_group_nearest"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler_nearest),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });
        let bind_group_linear = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixels_scaling_renderer_bind_group_linear"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler_linear),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        // Create pipeline
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pixels_scaling_renderer_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pixels_scaling_renderer_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: "vs_main",
                buffers: &[vertex_buffer_layout.clone()],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_texture_format,
                    blend: Some(blend_state),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });
        let render_pipeline_fill = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pixels_scaling_renderer_pipeline_fill"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module_fill,
                entry_point: "vs_main",
                buffers: &[vertex_buffer_layout],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module_fill,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_texture_format,
                    blend: Some(blend_state),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });

        // Create clipping rectangle
        let clip_rect = matrix.clip_rect();

        Self {
            vertex_buffer,
            uniform_buffer,
            bind_group_nearest,
            bind_group_linear,
            render_pipeline,
            render_pipeline_fill,
            clear_color,
            width: texture_size.width as f32,
            height: texture_size.height as f32,
            clip_rect,
            scaling_mode,
        }
    }

    /// Draw the pixel buffer to the render target.
    pub fn render(&self, encoder: &mut wgpu::CommandEncoder, render_target: &wgpu::TextureView) {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pixels_scaling_renderer_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: render_target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let pipeline = match self.scaling_mode {
            ScalingMode::PixelPerfect => &self.render_pipeline,
            ScalingMode::Fill => &self.render_pipeline_fill,
        };
        rpass.set_pipeline(pipeline);
        let bind_group = match self.scaling_mode {
            ScalingMode::PixelPerfect => &self.bind_group_nearest,
            ScalingMode::Fill => &self.bind_group_linear,
        };
        rpass.set_bind_group(0, bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_scissor_rect(
            self.clip_rect.0,
            self.clip_rect.1,
            self.clip_rect.2,
            self.clip_rect.3,
        );
        rpass.draw(0..3, 0..1);
    }

    /// Get the clipping rectangle for the scaling renderer.
    ///
    /// This rectangle defines the inner bounds of the surface texture, without the border.
    pub fn clip_rect(&self) -> (u32, u32, u32, u32) {
        self.clip_rect
    }

    pub(crate) fn resize(&mut self, queue: &wgpu::Queue, width: u32, height: u32) {
        let matrix = ScalingMatrix::new(
            (self.width, self.height),
            (width as f32, height as f32),
            self.scaling_mode,
        );
        queue.write_buffer(&self.uniform_buffer, 0, &matrix.uniform_buffer);

        self.clip_rect = matrix.clip_rect();
    }
}

#[derive(Debug)]
pub(crate) struct ScalingMatrix {
    pub(crate) transform: Mat4,
    clip_rect: (u32, u32, u32, u32),
    uniform_buffer: Vec<u8>,
}

impl ScalingMatrix {
    // texture_size is the dimensions of the drawing texture
    // screen_size is the dimensions of the surface being drawn to
    pub(crate) fn new(
        texture_size: (f32, f32),
        screen_size: (f32, f32),
        scaling_mode: ScalingMode,
    ) -> Self {
        let (texture_width, texture_height) = texture_size;
        let (screen_width, screen_height) = screen_size;

        let (scaled_width, scaled_height) = match scaling_mode {
            ScalingMode::PixelPerfect => {
                // Scale up to nearest integer multiple of screen size
                let width_ratio = (screen_width / texture_width).max(1.0);
                let height_ratio = (screen_height / texture_height).max(1.0);
                let scale = width_ratio.min(height_ratio).floor().max(1.0);
                (texture_width * scale, texture_height * scale)
            }
            ScalingMode::Fill => {
                // Scale up or down while preserving aspect ratio
                let width_ratio = screen_width / texture_width;
                let height_ratio = screen_height / texture_height;
                let scale = width_ratio.min(height_ratio);
                (texture_width * scale, texture_height * scale)
            }
        };

        // Create a transformation matrix
        let sw = scaled_width / screen_width;
        let sh = scaled_height / screen_height;
        let tx = (screen_width / 2.0).fract() / screen_width;
        let ty = (screen_height / 2.0).fract() / screen_height;
        #[rustfmt::skip]
        let transform: [f32; 16] = [
            sw,  0.0, 0.0, 0.0,
            0.0, sh,  0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            tx,  ty,  0.0, 1.0,
        ];

        // Create a clipping rectangle
        let clip_rect = {
            let scaled_width = scaled_width.min(screen_width);
            let scaled_height = scaled_height.min(screen_height);
            let x = ((screen_width - scaled_width) / 2.0) as u32;
            let y = ((screen_height - scaled_height) / 2.0) as u32;

            (x, y, scaled_width as u32, scaled_height as u32)
        };

        let mat = Mat4::from(transform);

        // Compute the constant buffer
        let mut uniform_buffer = Vec::new();
        uniform_buffer.extend_from_slice(mat.as_byte_slice());
        uniform_buffer.extend_from_slice(&texture_width.to_le_bytes());
        uniform_buffer.extend_from_slice(&texture_height.to_le_bytes());
        uniform_buffer.extend_from_slice(&(1.0 / texture_width).to_le_bytes());
        uniform_buffer.extend_from_slice(&(1.0 / texture_height).to_le_bytes());

        Self {
            transform: mat,
            clip_rect,
            uniform_buffer,
        }
    }

    pub(crate) fn clip_rect(&self) -> (u32, u32, u32, u32) {
        self.clip_rect
    }
}
