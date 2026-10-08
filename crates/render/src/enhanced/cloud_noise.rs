//! Persistent tileable cloud noise with shape deviation in its filtered mip chain.

use bevy::{
    asset::uuid_handle,
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
    shader::Shader,
};
use std::sync::atomic::{AtomicU8, Ordering};

pub(crate) const CLOUD_NOISE_SIZE: u32 = 64;
pub(crate) const CLOUD_NOISE_WORKGROUP: u32 = 4;
pub(crate) const CLOUD_NOISE_SHADER: Handle<Shader> =
    uuid_handle!("b19a7a84-82a0-4a41-8e9e-8a711bfae3e2");

pub(crate) fn shader_source(source: &str) -> String {
    source.replace("CLOUD_NOISE_WORKGROUP", &CLOUD_NOISE_WORKGROUP.to_string())
}

pub(crate) fn shader(source: &str, path: impl Into<String>) -> Shader {
    crate::shader_safety::from_wgsl(shader_source(source), path)
}

pub(crate) fn texture_descriptor() -> TextureDescriptor<'static> {
    TextureDescriptor {
        label: Some("Enhanced tileable Perlin Worley volume"),
        size: Extent3d {
            width: CLOUD_NOISE_SIZE,
            height: CLOUD_NOISE_SIZE,
            depth_or_array_layers: CLOUD_NOISE_SIZE,
        },
        mip_level_count: CLOUD_NOISE_SIZE.ilog2() + 1,
        sample_count: 1,
        dimension: TextureDimension::D3,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    }
}

pub(crate) fn sampler_descriptor() -> SamplerDescriptor<'static> {
    SamplerDescriptor {
        label: Some("Enhanced repeating cloud noise"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: FilterMode::Linear,
        ..default()
    }
}

pub(crate) fn generation_entries(mips: bool) -> Vec<BindGroupLayoutEntry> {
    let mut entries = vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::StorageTexture {
            access: StorageTextureAccess::WriteOnly,
            format: TextureFormat::Rgba8Unorm,
            view_dimension: TextureViewDimension::D3,
        },
        count: None,
    }];
    if mips {
        entries.push(BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStages::COMPUTE,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: false },
                view_dimension: TextureViewDimension::D3,
                multisampled: false,
            },
            count: None,
        });
    }
    entries
}

pub(crate) fn generation_layout(mips: bool) -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new("Enhanced cloud noise generation", &generation_entries(mips))
}

#[derive(Default)]
struct NoiseSubmission(AtomicU8);

impl NoiseSubmission {
    fn claim(&self) -> bool {
        self.0
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn ready(&self) -> bool {
        self.0.load(Ordering::Acquire) == 2
    }

    fn finish(&self) {
        self.0.store(2, Ordering::Release);
    }
}

#[derive(Resource)]
pub(crate) struct CloudNoiseVolume {
    _texture: Texture,
    pub view: TextureView,
    pub sampler: Sampler,
    generate: CachedComputePipelineId,
    filter: CachedComputePipelineId,
    groups: Vec<BindGroup>,
    submission: NoiseSubmission,
}

impl FromWorld for CloudNoiseVolume {
    fn from_world(world: &mut World) -> Self {
        Self::new(
            world.resource::<RenderDevice>(),
            world.resource::<PipelineCache>(),
        )
    }
}

impl CloudNoiseVolume {
    pub(crate) fn new(device: &RenderDevice, cache: &PipelineCache) -> Self {
        let descriptor = texture_descriptor();
        let texture = device.create_texture(&descriptor);
        let view = texture.create_view(&TextureViewDescriptor::default());
        let sampler = device.create_sampler(&sampler_descriptor());
        let mips: Vec<_> = (0..descriptor.mip_level_count)
            .map(|level| {
                texture.create_view(&TextureViewDescriptor {
                    dimension: Some(TextureViewDimension::D3),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..default()
                })
            })
            .collect();
        let queue = |name: &'static str, filtered| {
            cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some(name.into()),
                layout: vec![generation_layout(filtered)],
                shader: CLOUD_NOISE_SHADER,
                entry_point: Some(name.into()),
                ..default()
            })
        };
        let generate = queue("generate_noise", false);
        let filter = queue("filter_noise", true);
        let mut groups = Vec::with_capacity(mips.len());
        groups.push(device.create_bind_group(
            "Enhanced base cloud noise",
            &cache.get_bind_group_layout(&generation_layout(false)),
            &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&mips[0]),
            }],
        ));
        for level in 1..mips.len() {
            groups.push(device.create_bind_group(
                "Enhanced filtered cloud noise",
                &cache.get_bind_group_layout(&generation_layout(true)),
                &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&mips[level]),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::TextureView(&mips[level - 1]),
                    },
                ],
            ));
        }
        Self {
            _texture: texture,
            view,
            sampler,
            generate,
            filter,
            groups,
            submission: NoiseSubmission::default(),
        }
    }

    /// True once generation has been recorded; later work must preserve queue order.
    pub(crate) fn ready(&self) -> bool {
        self.submission.ready()
    }

    /// Returns whether generation was recorded; cached calls enqueue no GPU work.
    pub(crate) fn encode_once(&self, cache: &PipelineCache, encoder: &mut CommandEncoder) -> bool {
        if self.ready() {
            return false;
        }
        let (Some(generate), Some(filter)) = (
            cache.get_compute_pipeline(self.generate),
            cache.get_compute_pipeline(self.filter),
        ) else {
            return false;
        };
        if !self.submission.claim() {
            return false;
        }
        for (level, group) in self.groups.iter().enumerate() {
            let side = (CLOUD_NOISE_SIZE >> level).max(1);
            let dispatch = side.div_ceil(CLOUD_NOISE_WORKGROUP);
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("Enhanced cached cloud noise"),
                ..default()
            });
            pass.set_pipeline(if level == 0 { generate } else { filter });
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(dispatch, dispatch, dispatch);
        }
        self.submission.finish();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::NoiseSubmission;

    #[test]
    fn unchanged_volume_never_submits_generation_twice() {
        let submission = NoiseSubmission::default();
        assert!(!submission.ready());
        assert!(submission.claim());
        assert!(!submission.claim());
        submission.finish();
        for _ in 0..128 {
            assert!(submission.ready());
            assert!(!submission.claim());
        }
    }
}
