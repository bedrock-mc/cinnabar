use super::{MSAA_SHADER, SHADER, history::ExposureUniform};
use bevy::{
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
};
use std::{collections::HashMap, mem::size_of};

#[derive(Resource)]
pub(super) struct BlurPipeline {
    layouts: [BindGroupLayoutDescriptor; 2],
    pub sampler: Sampler,
    variants: HashMap<(TextureFormat, u32), CachedRenderPipelineId>,
}

impl BlurPipeline {
    pub fn layout(&self, samples: u32) -> &BindGroupLayoutDescriptor {
        &self.layouts[usize::from(samples > 1)]
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
        let layout = self.layout(samples).clone();
        *self.variants.entry((format, samples)).or_insert_with(|| {
            let shader = if samples > 1 { MSAA_SHADER } else { SHADER };
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
    let layouts = [false, true].map(|multisampled| {
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
                        min_binding_size: BufferSize::new(size_of::<ExposureUniform>() as u64),
                    },
                    count: None,
                },
            ],
        )
    });
    commands.insert_resource(BlurPipeline {
        layouts,
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("camera exposure linear clamp"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        variants: HashMap::default(),
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
            crate::SCENE_HDR_FORMAT
        } else {
            crate::SCENE_COLOR_FORMAT
        };
        ids.push(self.specialize(cache, format, view.msaa.samples()));
        Ok(())
    }
}
