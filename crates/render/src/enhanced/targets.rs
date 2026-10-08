//! Persistent post targets keep temporal history independent of the texture pool.
use super::{
    EnhancedRendering,
    atmosphere_cache::{AtmosphereCache, SKY_LUT_SIZE},
    quality::budget,
};
use bevy::render::{render_resource::*, renderer::RenderDevice};
pub(crate) struct PostTargets {
    pub size: [u32; 2],
    effects_size: [u32; 2],
    cloud_shadow_size: u32,
    reflection_samples: u32,
    quality: super::EnhancedQuality,
    _history: [Texture; 2],
    pub history_views: [TextureView; 2],
    pub effects: TextureView,
    pub sky: TextureView,
    pub composite: TextureView,
    pub cloud_shadow: TextureView,
    pub atmosphere_cache: AtmosphereCache,
    pub bindings: super::post::PostBindings,
}

pub(crate) struct EffectTarget {
    pub size: [u32; 2],
    pub view: TextureView,
}

fn effect_texture(device: &RenderDevice, label: &'static str, size: [u32; 2]) -> Texture {
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
}

impl EffectTarget {
    pub fn new(device: &RenderDevice, label: &'static str, size: [u32; 2]) -> Self {
        Self {
            size,
            view: effect_texture(device, label, size)
                .create_view(&TextureViewDescriptor::default()),
        }
    }
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
    _receiver_normal: Texture,
    pub receiver_normal_view: TextureView,
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
        let receiver_normal = device.create_texture(&TextureDescriptor {
            label: Some("Enhanced geometric receiver normal"),
            size: depth.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rg16Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let receiver_normal_view = receiver_normal.create_view(&TextureViewDescriptor::default());
        Self {
            size,
            colour,
            colour_view,
            mips,
            depth,
            depth_view,
            _motion: motion,
            motion_view,
            _receiver_normal: receiver_normal,
            receiver_normal_view,
        }
    }
}
impl PostTargets {
    pub fn new(device: &RenderDevice, size: [u32; 2], settings: &EnhancedRendering) -> Self {
        let create = |label, size| effect_texture(device, label, size);
        let (effects_size, cloud_shadow_size) = Self::effect_sizes(size, settings);
        let history = [
            create("enhanced temporal history A", size),
            create("enhanced temporal history B", size),
        ];
        let history_views = history
            .each_ref()
            .map(|texture| texture.create_view(&TextureViewDescriptor::default()));
        Self {
            size,
            effects_size,
            cloud_shadow_size,
            reflection_samples: budget(settings.quality).reflection_samples,
            quality: settings.quality,
            _history: history,
            history_views,
            effects: create("enhanced reduced resolution effects", effects_size)
                .create_view(&TextureViewDescriptor::default()),
            sky: create("enhanced sky view LUT", SKY_LUT_SIZE)
                .create_view(&TextureViewDescriptor::default()),
            composite: create("enhanced linear world composite", size)
                .create_view(&TextureViewDescriptor::default()),
            cloud_shadow: create("enhanced cloud shadow map", [cloud_shadow_size; 2])
                .create_view(&TextureViewDescriptor::default()),
            atmosphere_cache: AtmosphereCache::default(),
            bindings: super::post::PostBindings::default(),
        }
    }

    fn effect_sizes(size: [u32; 2], settings: &EnhancedRendering) -> ([u32; 2], u32) {
        let budget = budget(settings.quality);
        (
            size.map(|value| value.div_ceil(budget.effects_divisor).max(1)),
            if settings.volumetric_clouds {
                budget.cloud_shadow_resolution
            } else {
                1
            },
        )
    }

    pub fn prepare(&mut self, device: &RenderDevice, settings: &EnhancedRendering) {
        let reflection_samples = budget(settings.quality).reflection_samples;
        if self.reflection_samples != reflection_samples {
            self.atmosphere_cache.invalidate_environment();
            self.reflection_samples = reflection_samples;
        }
        self.quality = settings.quality;
        let (effects_size, cloud_shadow_size) = Self::effect_sizes(self.size, settings);
        if self.effects_size != effects_size {
            self.effects =
                effect_texture(device, "enhanced reduced resolution effects", effects_size)
                    .create_view(&TextureViewDescriptor::default());
            self.effects_size = effects_size;
        }
        if self.cloud_shadow_size != cloud_shadow_size {
            self.cloud_shadow =
                effect_texture(device, "enhanced cloud shadow map", [cloud_shadow_size; 2])
                    .create_view(&TextureViewDescriptor::default());
            self.cloud_shadow_size = cloud_shadow_size;
        }
    }

    pub fn prepare_atmosphere(&self, frame: &super::frame::EnhancedFrameGpu) {
        self.atmosphere_cache
            .prepare_with_quality(frame, self.cloud_shadow_size, self.quality);
    }
}
