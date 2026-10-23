//! Exact per-pixel coverage and depth resolve shared by every exposure tap.

use super::{MSAA_SHADER, SHADER, history::ExposureUniform};
use bevy::{
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
};

// Full precision retains averaged velocities, covered depth and close depth comparisons.
pub(super) const MOTION_FORMAT: TextureFormat = TextureFormat::Rgba32Float;
pub(super) const DEPTH_FORMAT: TextureFormat = TextureFormat::Rg32Float;

/// Enabled views retain the resolve targets until either source attachment changes.
pub(super) struct ResolvedInputs {
    _textures: [Texture; 2],
    pub motion: TextureView,
    pub depth: TextureView,
    pub binding: BindGroup,
    sources: (TextureViewId, TextureViewId),
}

impl ResolvedInputs {
    /// Rebuilds both exact resolves together when depth, coverage or extent changes. Leaves
    /// `state` empty when the device is out of memory.
    pub fn prepare(
        state: &mut Option<Self>,
        device: &RenderDevice,
        cache: &PipelineCache,
        layout: &BindGroupLayoutDescriptor,
        uniform: &Buffer,
        sources: (&TextureView, &TextureView),
        size: Extent3d,
    ) -> bool {
        let ids = (sources.0.id(), sources.1.id());
        if state
            .as_ref()
            .is_some_and(|old| old.sources == ids && old._textures[0].size() == size)
        {
            return false;
        }
        *state = None;
        let texels = u64::from(size.width) * u64::from(size.height);
        let allocate = |format: TextureFormat| {
            let bytes = texels * u64::from(format.block_copy_size(None).unwrap_or(16));
            crate::gpu_allocation::checked(device, "resolved exposure surface", bytes, || {
                device.create_texture(&TextureDescriptor {
                    label: Some("resolved exposure surface"),
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
        let (Some(motion), Some(depth)) = (allocate(MOTION_FORMAT), allocate(DEPTH_FORMAT)) else {
            return true;
        };
        let textures = [motion, depth];
        let motion = textures[0].create_view(&Default::default());
        let depth = textures[1].create_view(&Default::default());
        let binding = device.create_bind_group(
            "exposure surface resolve",
            &cache.get_bind_group_layout(layout),
            &[
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(sources.0),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(sources.1),
                },
            ],
        );
        *state = Some(Self {
            _textures: textures,
            motion,
            depth,
            binding,
            sources: ids,
        });
        true
    }
}

/// Raw depth and actor coverage are read only by the once-per-pixel resolve.
pub(super) fn layout(multisampled: bool) -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "exposure surface resolve",
        &[
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Depth,
                    view_dimension: TextureViewDimension::D2,
                    multisampled,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(std::mem::size_of::<ExposureUniform>() as u64),
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

/// Prewarms exact coverage/depth resolution for the source sample count.
pub(super) fn queue(cache: &PipelineCache, samples: u32) -> CachedRenderPipelineId {
    let shader = if samples > 1 { MSAA_SHADER } else { SHADER };
    cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("resolve exposure surface".into()),
        layout: vec![layout(samples > 1)],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("resolve_fragment".into()),
            targets: [MOTION_FORMAT, DEPTH_FORMAT]
                .map(|format| {
                    Some(ColorTargetState {
                        format,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })
                })
                .to_vec(),
            ..default()
        }),
        ..default()
    })
}
