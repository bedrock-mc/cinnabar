use super::{SHADER, history::ExposureUniform};
use bevy::{
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
};
use std::{collections::HashMap, mem::size_of};

#[derive(Resource)]
pub(super) struct BlurPipeline {
    layout: BindGroupLayoutDescriptor,
    footprint_layouts: [BindGroupLayoutDescriptor; 2],
    resolve_layouts: [BindGroupLayoutDescriptor; 2],
    pub sampler: Sampler,
    variants: HashMap<(TextureFormat, u32), CachedRenderPipelineId>,
    footprint_variants: Option<[CachedRenderPipelineId; 2]>,
    resolve_variants: HashMap<u32, CachedRenderPipelineId>,
    neutral_motion: HashMap<u32, (Texture, TextureView)>,
}

impl BlurPipeline {
    /// Retains both reduction layouts so unchanged preparation allocates nothing.
    pub fn footprint_layouts(&self) -> [&BindGroupLayoutDescriptor; 2] {
        self.footprint_layouts.each_ref()
    }

    /// Returns the row and tile variants shared by every source sample count.
    pub fn footprint(&mut self, cache: &PipelineCache) -> [CachedRenderPipelineId; 2] {
        *self
            .footprint_variants
            .get_or_insert_with(|| super::footprint::queue(cache))
    }

    /// Selects the raw coverage and depth bindings for the once-per-pixel resolve.
    pub fn resolve_layout(&self, samples: u32) -> &BindGroupLayoutDescriptor {
        &self.resolve_layouts[usize::from(samples > 1)]
    }

    /// Prewarms each exact resolve alongside the exposure pipelines.
    pub fn resolve(&mut self, cache: &PipelineCache, samples: u32) -> CachedRenderPipelineId {
        *self
            .resolve_variants
            .entry(samples)
            .or_insert_with(|| super::resolve::queue(cache, samples))
    }

    /// Actorless views bind a tiny texture and skip reading it in the exposure shader.
    pub fn neutral_motion(&mut self, device: &RenderDevice, samples: u32) -> TextureView {
        self.neutral_motion
            .entry(samples)
            .or_insert_with(|| {
                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("empty actor velocity"),
                    size: Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: TextureDimension::D2,
                    format: crate::actor_render::motion::MOTION_FORMAT,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                let view = texture.create_view(&Default::default());
                (texture, view)
            })
            .1
            .clone()
    }
    /// Every gather reads the same single-sample resolved surfaces.
    pub fn layout(&self) -> &BindGroupLayoutDescriptor {
        &self.layout
    }

    pub fn specialize(
        &mut self,
        cache: &PipelineCache,
        format: TextureFormat,
        samples: u32,
    ) -> CachedRenderPipelineId {
        if let Some(&id) = self.variants.get(&(format, samples)) {
            return id;
        }
        let layout = self.layout().clone();
        *self.variants.entry((format, samples)).or_insert_with(|| {
            let shader = SHADER;
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("camera velocity exposure".into()),
                layout: vec![layout],
                vertex: VertexState {
                    shader: shader.clone(),
                    entry_point: Some("vertex".into()),
                    ..default()
                },
                fragment: Some(FragmentState {
                    shader,
                    entry_point: Some("fragment".into()),
                    targets: vec![Some(ColorTargetState {
                        format,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                multisample: MultisampleState {
                    count: samples,
                    ..default()
                },
                ..default()
            })
        })
    }
}

pub(super) fn init(mut commands: Commands, device: Res<RenderDevice>) {
    let layout = {
        BindGroupLayoutDescriptor::new(
            "camera velocity exposure",
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
                    binding: 5,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 6,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 7,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        )
    };
    commands.insert_resource(BlurPipeline {
        layout,
        footprint_layouts: super::footprint::layouts(),
        resolve_layouts: [false, true].map(super::resolve::layout),
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("camera exposure linear clamp"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        variants: HashMap::default(),
        footprint_variants: None,
        resolve_variants: HashMap::default(),
        neutral_motion: HashMap::default(),
    });
}

impl crate::pipeline_warmup::PrewarmPipelines for BlurPipeline {
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
        ids.push(self.specialize(cache, format, view.msaa.samples()));
        for id in self.footprint(cache) {
            ids.push(id);
        }
        ids.push(self.resolve(cache, view.msaa.samples()));
        Ok(())
    }
}
