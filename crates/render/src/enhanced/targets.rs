//! Persistent post targets keep temporal history independent of the texture pool.
use super::atmosphere_cache::{AtmosphereCache, CLOUD_SHADOW_SIZE, SKY_LUT_SIZE};
use bevy::render::{render_resource::*, renderer::RenderDevice};
pub(crate) struct PostTargets {
    pub size: [u32; 2],
    _history: [Texture; 2],
    pub history_views: [TextureView; 2],
    pub effects: TextureView,
    pub sky: TextureView,
    pub composite: TextureView,
    pub cloud_shadow: TextureView,
    pub atmosphere_cache: AtmosphereCache,
}

pub(crate) struct SceneTargets {
    pub size: [u32; 2],
    pub colour: Texture,
    pub colour_view: TextureView,
    pub mips: Vec<TextureView>,
    pub depth: Texture,
    pub depth_view: TextureView,
    _motion: Texture,
    pub motion_view: TextureView,
}

impl SceneTargets {
    pub fn new(device: &RenderDevice, size: [u32; 2], format: TextureFormat) -> Self {
        let descriptor = TextureDescriptor {
            label: Some("enhanced opaque colour snapshot"),
            size: Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: size[0].max(size[1]).max(1).ilog2() + 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::COPY_DST
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        };
        let colour = device.create_texture(&descriptor);
        let colour_view = colour.create_view(&TextureViewDescriptor::default());
        let mips = (0..descriptor.mip_level_count)
            .map(|mip| {
                colour.create_view(&TextureViewDescriptor {
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    ..TextureViewDescriptor::default()
                })
            })
            .collect();
        let depth = device.create_texture(&TextureDescriptor {
            label: Some("enhanced opaque depth snapshot"),
            mip_level_count: 1,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::COPY_DST
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::RENDER_ATTACHMENT,
            ..descriptor
        });
        let depth_view = depth.create_view(&TextureViewDescriptor::default());
        let motion = device.create_texture(&TextureDescriptor {
            label: Some("Enhanced surface motion"),
            size: depth.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let motion_view = motion.create_view(&TextureViewDescriptor::default());
        Self {
            size,
            colour,
            colour_view,
            mips,
            depth,
            depth_view,
            _motion: motion,
            motion_view,
        }
    }
}
impl PostTargets {
    pub fn new(device: &RenderDevice, size: [u32; 2]) -> Self {
        let create = |label, size: [u32; 2]| {
            device.create_texture(&TextureDescriptor {
                label: Some(label),
                size: Extent3d {
                    width: size[0].max(1),
                    height: size[1].max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba16Float,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let history = [
            create("enhanced temporal history A", size),
            create("enhanced temporal history B", size),
        ];
        let history_views = history
            .each_ref()
            .map(|texture| texture.create_view(&TextureViewDescriptor::default()));
        Self {
            size,
            _history: history,
            history_views,
            effects: create(
                "enhanced half resolution effects",
                size.map(|v| v.div_ceil(2)),
            )
            .create_view(&TextureViewDescriptor::default()),
            sky: create("enhanced sky view LUT", SKY_LUT_SIZE)
                .create_view(&TextureViewDescriptor::default()),
            composite: create("enhanced linear world composite", size)
                .create_view(&TextureViewDescriptor::default()),
            cloud_shadow: create("enhanced cloud shadow map", [CLOUD_SHADOW_SIZE; 2])
                .create_view(&TextureViewDescriptor::default()),
            atmosphere_cache: AtmosphereCache::default(),
        }
    }
}
