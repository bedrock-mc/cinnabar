//! GPU percentile luminance metering without CPU readback.
use super::{EXPOSURE_SHADER, frame::EnhancedFrameGpu};
use bevy::{
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
    },
};
use std::num::NonZeroU64;

pub(crate) fn layout() -> BindGroupLayoutDescriptor {
    let uniform = |size| BindingType::Buffer {
        ty: BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: NonZeroU64::new(size),
    };
    let storage = |size| BindingType::Buffer {
        ty: BufferBindingType::Storage { read_only: false },
        has_dynamic_offset: false,
        min_binding_size: NonZeroU64::new(size),
    };
    let types = [
        uniform(std::mem::size_of::<EnhancedFrameGpu>() as u64),
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: false },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Depth,
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        storage(256),
        storage(16),
    ];
    let entries: Vec<_> = types
        .into_iter()
        .enumerate()
        .map(|(binding, ty)| BindGroupLayoutEntry {
            binding: binding as u32,
            visibility: ShaderStages::COMPUTE,
            ty,
            count: None,
        })
        .collect();
    BindGroupLayoutDescriptor::new("enhanced exposure layout", &entries)
}

pub(crate) struct ExposureBuffers {
    pub histogram: Buffer,
    pub value: Buffer,
}
impl ExposureBuffers {
    pub fn new(device: &RenderDevice) -> Self {
        let create = |label, size| {
            device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            histogram: create("enhanced luminance histogram", 256),
            value: create("enhanced adapted exposure", 16),
        }
    }
}

#[derive(Resource)]
pub(crate) struct ExposurePipeline {
    histogram: CachedComputePipelineId,
    adapt: CachedComputePipelineId,
}
impl FromWorld for ExposurePipeline {
    fn from_world(world: &mut World) -> Self {
        let cache = world.resource::<PipelineCache>();
        let pipeline = |name: &'static str| {
            cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some(name.into()),
                layout: vec![layout()],
                shader: EXPOSURE_SHADER,
                entry_point: Some(name.into()),
                ..default()
            })
        };
        Self {
            histogram: pipeline("build_histogram"),
            adapt: pipeline("adapt_exposure"),
        }
    }
}

pub(crate) fn meter(
    context: &mut RenderContext,
    world: &World,
    frame: &Buffer,
    buffers: &ExposureBuffers,
    scene: &TextureView,
    depth: &TextureView,
    size: [u32; 2],
) -> bool {
    let pipeline = world.resource::<ExposurePipeline>();
    let cache = world.resource::<PipelineCache>();
    let (Some(histogram), Some(adapt)) = (
        cache.get_compute_pipeline(pipeline.histogram),
        cache.get_compute_pipeline(pipeline.adapt),
    ) else {
        return false;
    };
    let group = context.render_device().create_bind_group(
        "enhanced exposure",
        &cache.get_bind_group_layout(&layout()),
        &[
            BindGroupEntry {
                binding: 0,
                resource: frame.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(scene),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::TextureView(depth),
            },
            BindGroupEntry {
                binding: 3,
                resource: buffers.histogram.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: buffers.value.as_entire_binding(),
            },
        ],
    );
    context
        .command_encoder()
        .clear_buffer(&buffers.histogram, 0, None);
    let mut pass = context
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("enhanced auto exposure"),
            ..default()
        });
    pass.set_bind_group(0, &group, &[]);
    pass.set_pipeline(histogram);
    pass.dispatch_workgroups(size[0].div_ceil(32), size[1].div_ceil(32), 1);
    pass.set_pipeline(adapt);
    pass.dispatch_workgroups(1, 1, 1);
    true
}
