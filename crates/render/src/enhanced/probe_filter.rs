//! Cached GGX radiance and cosine irradiance in the existing environment array.

use super::{CUBE_FACE_COUNT, MIPS, ProbeGpu};
use bevy::{
    core_pipeline::FullscreenShader,
    prelude::*,
    render::{render_resource::*, renderer::RenderContext},
};
use std::num::NonZeroU64;

pub(super) const DIFFUSE_MIP: u32 = (MIPS - 1) / 2;
pub(super) const ARRAY_LAYERS: u32 = 2 * CUBE_FACE_COUNT + 3;
pub(super) const SKY_DIFFUSE_LAYER: u32 = ARRAY_LAYERS - 3;
pub(super) const RAW_SKY_LAYER: u32 = ARRAY_LAYERS - 2;
pub(super) const SKY_SPECULAR_LAYER: u32 = ARRAY_LAYERS - 1;

pub(crate) fn layout() -> BindGroupLayoutDescriptor {
    let entries = [
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2Array,
            multisampled: false,
        },
        BindingType::Sampler(SamplerBindingType::Filtering),
        BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(std::mem::size_of::<[f32; 4]>() as u64),
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(binding, ty)| BindGroupLayoutEntry {
        binding: binding as u32,
        visibility: ShaderStages::FRAGMENT,
        ty,
        count: None,
    })
    .collect::<Vec<_>>();
    BindGroupLayoutDescriptor::new("enhanced environment convolution", &entries)
}

pub(super) struct EnvironmentFilter {
    pub pipeline: CachedRenderPipelineId,
    specular: Vec<Vec<BindGroup>>,
    diffuse: Vec<BindGroup>,
    sky_specular: Vec<BindGroup>,
    sky_diffuse: BindGroup,
    parameters: Vec<Buffer>,
    samples: u32,
}

impl EnvironmentFilter {
    pub fn new(world: &World) -> Self {
        let cache = world.resource::<PipelineCache>();
        let device = world.resource::<bevy::render::renderer::RenderDevice>();
        let gpu = world.resource::<ProbeGpu>();
        let sampler = &world
            .resource::<super::super::gpu::EnhancedGpu>()
            .linear_sampler;
        let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("prefilter_environment".into()),
            layout: vec![layout()],
            vertex: world.resource::<FullscreenShader>().to_vertex_state(),
            fragment: Some(FragmentState {
                shader: super::super::PROBE_FILTER_SHADER,
                entry_point: Some("prefilter_environment".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        });
        let samples = super::super::quality::budget(super::super::EnhancedQuality::default())
            .reflection_samples;
        let mut parameters = Vec::new();
        let mut group = |face: u32, roughness: f32, mode: u32| {
            let parameter = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("environment convolution parameters"),
                contents: bytemuck::bytes_of(&[
                    face as f32,
                    roughness,
                    mode as f32,
                    samples as f32,
                ]),
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            });
            parameters.push(parameter.clone());
            device.create_bind_group(
                "cached environment convolution",
                &cache.get_bind_group_layout(&layout()),
                &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&gpu.raw_cube),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(sampler),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: parameter.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: BindingResource::TextureView(&gpu.sky_view),
                    },
                ],
            )
        };
        Self {
            pipeline,
            specular: (0..CUBE_FACE_COUNT)
                .map(|face| {
                    (1..MIPS)
                        .map(|mip| group(face, mip as f32 / (MIPS - 1) as f32, 0))
                        .collect()
                })
                .collect(),
            diffuse: (0..CUBE_FACE_COUNT)
                .map(|face| group(face, 1.0, 1))
                .collect(),
            sky_specular: (1..MIPS)
                .map(|mip| group(0, mip as f32 / (MIPS - 1) as f32, 2))
                .collect(),
            sky_diffuse: group(0, 1.0, 3),
            parameters,
            samples,
        }
    }

    pub fn set_quality(
        &mut self,
        queue: &bevy::render::renderer::RenderQueue,
        quality: super::super::EnhancedQuality,
    ) {
        let samples = super::super::quality::budget(quality).reflection_samples;
        if self.samples != samples {
            for parameter in &self.parameters {
                queue.write_buffer(parameter, 12, bytemuck::bytes_of(&(samples as f32)));
            }
            self.samples = samples;
        }
    }

    pub fn ready(&self, world: &World) -> bool {
        world
            .resource::<PipelineCache>()
            .get_render_pipeline(self.pipeline)
            .is_some()
    }

    pub fn capture(&self, context: &mut RenderContext, world: &World, face: usize) -> bool {
        let gpu = world.resource::<ProbeGpu>();
        let mut complete = true;
        for (group, target) in self.specular[face].iter().zip(&gpu.target_faces[face][1..]) {
            complete &= self.draw(context, world, group, target);
        }
        complete &= self.draw(
            context,
            world,
            &self.diffuse[face],
            &gpu.target_diffuse_faces[face],
        );
        complete
    }

    pub fn sky(&self, context: &mut RenderContext, world: &World) -> bool {
        let gpu = world.resource::<ProbeGpu>();
        let mut complete = true;
        for (group, target) in self.sky_specular.iter().zip(&gpu.sky_specular[1..]) {
            complete &= self.draw(context, world, group, target);
        }
        complete &= self.draw(context, world, &self.sky_diffuse, &gpu.sky_diffuse);
        complete
    }

    fn draw(
        &self,
        context: &mut RenderContext,
        world: &World,
        group: &BindGroup,
        target: &TextureView,
    ) -> bool {
        let cache = world.resource::<PipelineCache>();
        let Some(pipeline) = cache.get_render_pipeline(self.pipeline) else {
            return false;
        };
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("cached environment convolution"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            ..default()
        });
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
