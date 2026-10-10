//! Persistent unit-irradiance atmospheric transfer, independent of camera and weather updates.

use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicU32, Ordering},
};

use bevy::{
    asset::uuid_handle,
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
    },
};

pub(crate) const MULTIPLE_SCATTER_SIZE: [u32; 3] = [32, 32, 8];
const DIRECTIONS: u32 = 32;
const STEPS: u32 = 20;
const BATCH_ROWS: u32 = 4;
const WORKGROUP_SIDE: u32 = 4;
pub(crate) const MULTIPLE_SCATTER_SHADER: Handle<Shader> =
    uuid_handle!("7e11c460-1b18-4c72-bc54-c5c6460c37b8");

pub(crate) fn shader_source(source: &str) -> String {
    source
        .replace("MULTIPLE_SCATTER_DIRECTIONS", &format!("{DIRECTIONS}u"))
        .replace("MULTIPLE_SCATTER_STEPS", &format!("{STEPS}u"))
        .replace("MULTIPLE_SCATTER_WORKGROUP", &WORKGROUP_SIDE.to_string())
}

pub(crate) fn shader(source: &str, path: impl Into<String>) -> Shader {
    crate::shader_safety::from_wgsl(shader_source(source), path)
}

pub(crate) fn texture_descriptor() -> TextureDescriptor<'static> {
    TextureDescriptor {
        label: Some("Enhanced atmospheric multiple scattering"),
        size: Extent3d {
            width: MULTIPLE_SCATTER_SIZE[0],
            height: MULTIPLE_SCATTER_SIZE[1],
            depth_or_array_layers: MULTIPLE_SCATTER_SIZE[2],
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D3,
        format: TextureFormat::Rgba16Float,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    }
}

pub(crate) fn sampler_descriptor() -> SamplerDescriptor<'static> {
    SamplerDescriptor {
        label: Some("Enhanced atmospheric transfer interpolation"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    }
}

pub(crate) fn generation_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "Enhanced multiple scattering generation",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::StorageTexture {
                    access: StorageTextureAccess::WriteOnly,
                    format: TextureFormat::Rgba16Float,
                    view_dimension: TextureViewDimension::D3,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(16),
                },
                count: None,
            },
        ],
    )
}

#[derive(Resource)]
pub(crate) struct MultipleScattering {
    _texture: Texture,
    _batch_uniforms: Buffer,
    pub view: TextureView,
    pub sampler: Sampler,
    pipeline: CachedComputePipelineId,
    group: BindGroup,
    uniform_slot_bytes: u32,
    submitted: AtomicU32,
}

impl FromWorld for MultipleScattering {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        let cache = world.resource::<PipelineCache>();
        let texture = device.create_texture(&texture_descriptor());
        let view = texture.create_view(&TextureViewDescriptor::default());
        let sampler = device.create_sampler(&sampler_descriptor());
        let rows = MULTIPLE_SCATTER_SIZE[1].div_ceil(BATCH_ROWS);
        let uniform_slot_bytes = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let mut slots = vec![0u32; (Self::batch_count() * uniform_slot_bytes / 4) as usize];
        for batch in 0..Self::batch_count() {
            let index = (batch * uniform_slot_bytes / 4) as usize;
            slots[index..index + 4].copy_from_slice(&[
                (batch % rows) * BATCH_ROWS,
                batch / rows,
                BATCH_ROWS,
                0,
            ]);
        }
        let uniforms = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("Enhanced atmospheric generation batches"),
            contents: bytemuck::cast_slice(&slots),
            usage: BufferUsages::UNIFORM,
        });
        let layout = generation_layout();
        let group = device.create_bind_group(
            "Enhanced atmospheric generation",
            &cache.get_bind_group_layout(&layout),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer: &uniforms,
                        offset: 0,
                        size: NonZeroU64::new(16),
                    }),
                },
            ],
        );
        let pipeline = cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("Enhanced bounded atmospheric transfer".into()),
            layout: vec![layout],
            shader: MULTIPLE_SCATTER_SHADER,
            entry_point: Some("generate_multiple_scattering".into()),
            ..default()
        });
        Self {
            _texture: texture,
            _batch_uniforms: uniforms,
            view,
            sampler,
            pipeline,
            group,
            uniform_slot_bytes,
            submitted: AtomicU32::new(0),
        }
    }
}

impl MultipleScattering {
    fn batch_count() -> u32 {
        MULTIPLE_SCATTER_SIZE[1].div_ceil(BATCH_ROWS) * MULTIPLE_SCATTER_SIZE[2]
    }

    /// Later consumers must retain queue order after the final generation submission.
    pub(crate) fn ready(&self) -> bool {
        self.submitted.load(Ordering::Acquire) == Self::batch_count()
    }

    fn encode_batch(&self, cache: &PipelineCache, encoder: &mut CommandEncoder) -> bool {
        let Some(pipeline) = cache.get_compute_pipeline(self.pipeline) else {
            return false;
        };
        let batch = self.submitted.load(Ordering::Acquire);
        if batch >= Self::batch_count() {
            return false;
        }
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("Enhanced atmospheric transfer batch"),
            ..default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.group, &[batch * self.uniform_slot_bytes]);
        pass.dispatch_workgroups(
            MULTIPLE_SCATTER_SIZE[0].div_ceil(WORKGROUP_SIDE),
            BATCH_ROWS.div_ceil(WORKGROUP_SIDE),
            1,
        );
        drop(pass);
        true
    }
}

pub(crate) fn prepare(
    transfer: Res<MultipleScattering>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    views: Query<(), With<super::EnhancedRendering>>,
) {
    if transfer.ready()
        || views.is_empty()
        || cache.get_compute_pipeline(transfer.pipeline).is_none()
    {
        return;
    }
    let mut encoder = device.create_command_encoder(&Default::default());
    if transfer.encode_batch(&cache, &mut encoder) {
        queue.submit([encoder.finish()]);
        transfer.submitted.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
pub(super) fn fixture(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
) -> (wgpu::TextureView, wgpu::Sampler) {
    use wgpu::util::DeviceExt as _;
    let texture = device.create_texture(&texture_descriptor());
    let view = texture.create_view(&Default::default());
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Enhanced atmospheric transfer fixture"),
        source: wgpu::ShaderSource::Wgsl(
            crate::shader_source::composed(
                &shader_source(include_str!("multiple_scattering.wgsl")),
                &[],
            )
            .into(),
        ),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Enhanced atmospheric transfer fixture"),
        layout: None,
        module: &module,
        entry_point: Some("generate_multiple_scattering"),
        compilation_options: Default::default(),
        cache: None,
    });
    let rows = MULTIPLE_SCATTER_SIZE[1].div_ceil(BATCH_ROWS);
    for batch in 0..MultipleScattering::batch_count() {
        let words = [(batch % rows) * BATCH_ROWS, batch / rows, BATCH_ROWS, 0];
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Enhanced atmospheric fixture batch"),
            contents: bytemuck::cast_slice(&words),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Enhanced atmospheric fixture batch"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(
            MULTIPLE_SCATTER_SIZE[0].div_ceil(WORKGROUP_SIDE),
            BATCH_ROWS.div_ceil(WORKGROUP_SIDE),
            1,
        );
    }
    (view, device.create_sampler(&sampler_descriptor()))
}
