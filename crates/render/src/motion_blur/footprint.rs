//! Tile velocities bound the actor footprint search without a full-resolution neighbourhood scan.

use super::{SHADER, history::ExposureUniform, shader::TILE_SIZE};
use bevy::{
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
};
use std::mem::size_of;

pub(super) const FORMAT: TextureFormat = TextureFormat::Rg16Float;
// Row winners must not round before the final winner is selected.
pub(super) const ROW_FORMAT: TextureFormat = TextureFormat::Rgba32Float;

/// Retains the reduced velocity texture and its source binding for one enabled view.
pub(super) struct Footprint {
    pub texture: Texture,
    pub view: TextureView,
    pub bindings: [BindGroup; 2],
    pub rows: Texture,
    pub row_view: TextureView,
    motion: TextureViewId,
}

impl Footprint {
    /// Reuses the tile target until either its extent or actor velocity source changes;
    /// `layout` is resolved through `cache` only when the target is rebuilt. Leaves `state`
    /// empty when the device is out of memory.
    pub fn prepare(
        state: &mut Option<Self>,
        device: &RenderDevice,
        cache: &PipelineCache,
        layouts: [&BindGroupLayoutDescriptor; 2],
        uniform: &Buffer,
        motion: &TextureView,
        size: Extent3d,
    ) -> bool {
        let row_size = Extent3d {
            width: size.width.div_ceil(TILE_SIZE),
            ..size
        };
        let size = Extent3d {
            width: size.width.div_ceil(TILE_SIZE),
            height: size.height.div_ceil(TILE_SIZE),
            depth_or_array_layers: 1,
        };
        if state.as_ref().is_some_and(|old| {
            old.texture.size() == size && old.rows.size() == row_size && old.motion == motion.id()
        }) {
            return false;
        }
        *state = None;
        let allocate = |label, size: Extent3d, format: TextureFormat| {
            let bytes = u64::from(size.width)
                * u64::from(size.height)
                * u64::from(format.block_copy_size(None).unwrap_or(16));
            crate::gpu_allocation::checked(device, label, bytes, || {
                device.create_texture(&TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
            })
        };
        let (Some(texture), Some(rows)) = (
            allocate("actor motion tiles", size, FORMAT),
            allocate("actor motion row winners", row_size, ROW_FORMAT),
        ) else {
            return true;
        };
        let view = texture.create_view(&Default::default());
        let row_view = rows.create_view(&Default::default());
        let row_binding = device.create_bind_group(
            "actor motion row source",
            &cache.get_bind_group_layout(layouts[0]),
            &[
                BindGroupEntry {
                    binding: 3,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::TextureView(motion),
                },
            ],
        );
        let tile_binding = device.create_bind_group(
            "actor motion row winners",
            &cache.get_bind_group_layout(layouts[1]),
            &[BindGroupEntry {
                binding: 8,
                resource: BindingResource::TextureView(&row_view),
            }],
        );
        *state = Some(Self {
            texture,
            view,
            bindings: [row_binding, tile_binding],
            rows,
            row_view,
            motion: motion.id(),
        });
        true
    }
}

/// Row reduction shares exact resolved motion; the final reduction reads only row winners.
pub(super) fn layouts() -> [BindGroupLayoutDescriptor; 2] {
    let texture = |binding| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: false },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    [
        BindGroupLayoutDescriptor::new(
            "actor motion row source",
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
                texture(6),
            ],
        ),
        BindGroupLayoutDescriptor::new("actor motion row winners", &[texture(8)]),
    ]
}

/// Queues both short reductions during warmup, before a moving actor reaches the view.
pub(super) fn queue(cache: &PipelineCache) -> [CachedRenderPipelineId; 2] {
    let layouts = layouts();
    let mut layouts = layouts.into_iter();
    [("row_fragment", ROW_FORMAT), ("tile_fragment", FORMAT)].map(|(entry, format)| {
        let layout = layouts.next().unwrap();
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("actor motion reduction".into()),
            layout: vec![layout],
            vertex: VertexState {
                shader: SHADER,
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: SHADER,
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    })
}
