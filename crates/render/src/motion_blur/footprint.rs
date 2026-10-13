//! Tile velocities bound the actor footprint search without a full-resolution neighbourhood scan.

use super::{MSAA_SHADER, SHADER, history::ExposureUniform};
use bevy::{
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
};
use std::mem::size_of;

pub(super) const TILE_SIZE: u32 = 48;
pub(super) const FORMAT: TextureFormat = TextureFormat::Rg16Float;

/// Retains the reduced velocity texture and its source binding for one enabled view.
pub(super) struct Footprint {
    pub texture: Texture,
    pub view: TextureView,
    pub binding: BindGroup,
    motion: TextureViewId,
}

impl Footprint {
    /// Reuses the tile target until either its extent or actor velocity source changes.
    pub fn prepare(
        state: &mut Option<Self>,
        device: &RenderDevice,
        layout: &BindGroupLayout,
        uniform: &Buffer,
        motion: &TextureView,
        size: Extent3d,
    ) -> bool {
        let size = Extent3d {
            width: size.width.div_ceil(TILE_SIZE),
            height: size.height.div_ceil(TILE_SIZE),
            depth_or_array_layers: 1,
        };
        if state
            .as_ref()
            .is_some_and(|old| old.texture.size() == size && old.motion == motion.id())
        {
            return false;
        }
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("actor motion tiles"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let binding = device.create_bind_group(
            "actor motion tile source",
            layout,
            &[
                BindGroupEntry {
                    binding: 3,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(motion),
                },
            ],
        );
        *state = Some(Self {
            texture,
            view,
            binding,
            motion: motion.id(),
        });
        true
    }
}

/// Tile reduction reads the same exposure and actor samples as the final gather.
pub(super) fn layout(multisampled: bool) -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "actor motion tile source",
        &[
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ExposureUniform>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: false },
                    view_dimension: TextureViewDimension::D2,
                    multisampled,
                },
                count: None,
            },
        ],
    )
}

/// Queues reduction with the ordinary exposure warmup, before a moving actor reaches the view.
pub(super) fn queue(cache: &PipelineCache, samples: u32) -> CachedRenderPipelineId {
    let shader = if samples > 1 { MSAA_SHADER } else { SHADER };
    cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("actor motion tile reduction".into()),
        layout: vec![layout(samples > 1)],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("tile_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    })
}
