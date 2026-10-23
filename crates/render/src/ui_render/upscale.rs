//! Bilinear world reconstruction with optional, bounded sharpening before native UI.

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::*,
    render::{RenderApp, render_resource::*, renderer::RenderDevice},
};

const SHADER: Handle<Shader> = uuid_handle!("3dd2d5df-3d2c-44af-bbba-f0ff45fc60ed");

/// Loads the original reconstruction shader and its per-device pipeline resources.
pub(super) fn install(app: &mut App) {
    load_internal_asset!(
        app,
        SHADER,
        "../world_upscale.wgsl",
        crate::shader_safety::from_wgsl
    );
    crate::pipeline_warmup::register::<UpscalePipeline>(app);
    app.sub_app_mut(RenderApp)
        .init_resource::<UpscalePipeline>();
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(super) struct UpscaleKey {
    pub format: TextureFormat,
    pub sharpen: bool,
}

struct UpscaleSpecializer;

impl Specializer<RenderPipeline> for UpscaleSpecializer {
    type Key = UpscaleKey;

    /// Specializes colour storage and removes the extra samples when sharpening is disabled.
    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.targets[0].as_mut().unwrap().format = key.format;
        fragment.entry_point = Some(
            if key.sharpen {
                "sharpen_fragment"
            } else {
                "bilinear_fragment"
            }
            .into(),
        );
        Ok(key)
    }
}

/// Shares the input layout and cached variants across scaled views.
#[derive(Resource)]
pub(super) struct UpscalePipeline {
    layout: BindGroupLayoutDescriptor,
    variants: Variants<RenderPipeline, UpscaleSpecializer>,
}

impl FromWorld for UpscalePipeline {
    /// Declares pipeline state before the render device becomes available.
    fn from_world(_world: &mut World) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "world upscale inputs",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("world upscale before native UI".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: SHADER,
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: SHADER,
                entry_point: Some("bilinear_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        };
        Self {
            layout,
            variants: Variants::new(UpscaleSpecializer, descriptor),
        }
    }
}

impl UpscalePipeline {
    /// Queues one reusable variant for the scene colour format and selected filter.
    pub(super) fn specialize(
        &mut self,
        cache: &PipelineCache,
        key: UpscaleKey,
    ) -> CachedRenderPipelineId {
        self.variants
            .specialize(cache, key)
            .expect("valid world upscale specialization")
    }

    /// Binds a completed world image without sampling the presentation attachment.
    pub(super) fn binding(
        &self,
        device: &RenderDevice,
        cache: &PipelineCache,
        source: &TextureView,
        sampler: &Sampler,
    ) -> BindGroup {
        device.create_bind_group(
            "world upscale",
            &cache.get_bind_group_layout(&self.layout),
            &BindGroupEntries::sequential((source, sampler)),
        )
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for UpscalePipeline {
    /// Warms both filter choices before entering a world, including HDR camera variants.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        let format = if view.hdr {
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
        };
        ids.extend(
            [false, true].map(|sharpen| self.specialize(cache, UpscaleKey { format, sharpen })),
        );
        Ok(())
    }
}
