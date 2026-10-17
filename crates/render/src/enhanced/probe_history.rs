//! Displayed probe radiance follows replacement captures without restarting from old targets.

use super::CUBE_FACE_COUNT;
use bevy::{
    color::LinearRgba,
    core_pipeline::FullscreenShader,
    prelude::*,
    render::{render_resource::*, renderer::RenderContext},
};

const RESPONSE_SECONDS: f32 = 0.06;
const SETTLE_RESIDUAL: f32 = 0.002;

#[derive(Clone, Copy, Debug, Default)]
struct FaceHistory {
    populated: bool,
    active: bool,
    residual: f32,
}

#[derive(Debug, Default)]
pub(super) struct ReflectionHistory {
    faces: [FaceHistory; CUBE_FACE_COUNT as usize],
    last_seconds: Option<f32>,
}

impl ReflectionHistory {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns true when an unpopulated face must publish its first capture immediately.
    pub fn target_changed(&mut self, face: usize) -> bool {
        let state = &mut self.faces[face];
        if !state.populated {
            state.populated = true;
            return true;
        }
        state.active = true;
        state.residual = 1.0;
        false
    }

    /// Weights apply to the current displayed radiance, including interrupted transitions.
    pub fn weights(&mut self, seconds: f32) -> [Option<f32>; CUBE_FACE_COUNT as usize] {
        let mut weights = [None; CUBE_FACE_COUNT as usize];
        if !seconds.is_finite() {
            return weights;
        }
        let elapsed = self
            .last_seconds
            .map_or(0.0, |previous| (seconds - previous).max(0.0));
        self.last_seconds = Some(seconds);
        if elapsed <= 0.0 {
            return weights;
        }
        let retained = (-elapsed / RESPONSE_SECONDS).exp();
        for (state, weight) in self.faces.iter_mut().zip(&mut weights) {
            if !state.active {
                continue;
            }
            state.residual *= retained;
            if state.residual <= SETTLE_RESIDUAL {
                state.active = false;
                *weight = Some(1.0);
            } else {
                *weight = Some(1.0 - retained);
            }
        }
        weights
    }
}

pub(crate) fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "enhanced reflection history resolve",
        &[
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ],
    )
}

pub(crate) fn colour_target() -> ColorTargetState {
    let component = BlendComponent {
        src_factor: BlendFactor::Constant,
        dst_factor: BlendFactor::OneMinusConstant,
        operation: BlendOperation::Add,
    };
    ColorTargetState {
        format: TextureFormat::Rgba16Float,
        blend: Some(BlendState {
            color: component,
            alpha: component,
        }),
        write_mask: ColorWrites::ALL,
    }
}

pub(super) struct ReflectionResolve {
    pipeline: CachedRenderPipelineId,
    specular: Vec<Vec<BindGroup>>,
    diffuse: Vec<BindGroup>,
}

impl ReflectionResolve {
    pub fn new(world: &World) -> Self {
        let cache = world.resource::<PipelineCache>();
        let device = world.resource::<bevy::render::renderer::RenderDevice>();
        let gpu = world.resource::<super::ProbeGpu>();
        let sampler = &world
            .resource::<super::super::gpu::EnhancedGpu>()
            .linear_sampler;
        let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("resolve_reflections".into()),
            layout: vec![layout()],
            vertex: world.resource::<FullscreenShader>().to_vertex_state(),
            fragment: Some(FragmentState {
                shader: super::super::PROBE_SHADER,
                entry_point: Some("resolve_reflections".into()),
                targets: vec![Some(colour_target())],
                ..default()
            }),
            ..default()
        });
        let group = |source: &TextureView| {
            device.create_bind_group(
                "cached reflection history target",
                &cache.get_bind_group_layout(&layout()),
                &[
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::TextureView(source),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: BindingResource::Sampler(sampler),
                    },
                ],
            )
        };
        Self {
            pipeline,
            specular: gpu
                .target_faces
                .iter()
                .map(|face| face.iter().map(group).collect())
                .collect(),
            diffuse: gpu.target_diffuse_faces.iter().map(group).collect(),
        }
    }

    pub fn ready(&self, world: &World) -> bool {
        world
            .resource::<PipelineCache>()
            .get_render_pipeline(self.pipeline)
            .is_some()
    }

    pub fn resolve(&self, context: &mut RenderContext, world: &World, face: usize, weight: f32) {
        let gpu = world.resource::<super::ProbeGpu>();
        let cache = world.resource::<PipelineCache>();
        let Some(pipeline) = cache.get_render_pipeline(self.pipeline) else {
            return;
        };
        let groups = self.specular[face]
            .iter()
            .zip(&gpu.faces[face])
            .chain(std::iter::once((
                &self.diffuse[face],
                &gpu.diffuse_faces[face],
            )));
        for (group, target) in groups {
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("enhanced continuous reflection resolve"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                ..default()
            });
            pass.set_render_pipeline(pipeline);
            pass.set_blend_constant(LinearRgba::new(weight, weight, weight, weight));
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

pub(super) fn reset(context: &mut RenderContext, gpu: &super::ProbeGpu) {
    gpu.history.lock().unwrap().reset();
    let targets = gpu
        .faces
        .iter()
        .flat_map(|face| face.iter())
        .chain(&gpu.diffuse_faces)
        .chain(gpu.target_faces.iter().map(|face| &face[0]));
    for target in targets {
        let _pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("enhanced reflection history reset"),
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
    }
}

pub(super) fn publish(context: &mut RenderContext, gpu: &super::ProbeGpu, face: usize) {
    if !gpu.history.lock().unwrap().target_changed(face) {
        return;
    }
    publish_initial(context, gpu, face);
}

pub(super) fn publish_initial(context: &mut RenderContext, gpu: &super::ProbeGpu, face: usize) {
    let mut copy = |layer: u32, mip: u32| {
        let origin = Origin3d {
            x: 0,
            y: 0,
            z: layer,
        };
        context.command_encoder().copy_texture_to_texture(
            TexelCopyTextureInfo {
                mip_level: mip,
                origin,
                ..gpu.target_texture.as_image_copy()
            },
            TexelCopyTextureInfo {
                mip_level: mip,
                origin,
                ..gpu._texture.as_image_copy()
            },
            Extent3d {
                width: super::SIZE >> mip,
                height: super::SIZE >> mip,
                depth_or_array_layers: 1,
            },
        );
    };
    for mip in 0..super::MIPS {
        copy(face as u32, mip);
    }
    let mip = super::probe_filter::DIFFUSE_MIP;
    context.command_encoder().copy_texture_to_texture(
        TexelCopyTextureInfo {
            origin: Origin3d {
                x: 0,
                y: 0,
                z: face as u32,
            },
            ..gpu.diffuse_target_texture.as_image_copy()
        },
        TexelCopyTextureInfo {
            mip_level: mip,
            origin: Origin3d {
                x: 0,
                y: 0,
                z: CUBE_FACE_COUNT + face as u32,
            },
            ..gpu._texture.as_image_copy()
        },
        Extent3d {
            width: super::SIZE >> mip,
            height: super::SIZE >> mip,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refreshed() -> ReflectionHistory {
        let mut history = ReflectionHistory::default();
        assert!(history.target_changed(0));
        assert_eq!(history.weights(0.0), [None; CUBE_FACE_COUNT as usize]);
        assert!(!history.target_changed(0));
        history
    }

    #[test]
    fn replacement_is_monotonic_and_stops_work_after_finite_settle() {
        let mut history = refreshed();
        let mut displayed = 0.0;
        for step in 1..=100 {
            if let Some(weight) = history.weights(step as f32 * 0.01)[0] {
                let next = displayed + (1.0 - displayed) * weight;
                assert!(next >= displayed && next <= 1.0);
                displayed = next;
            }
        }
        assert_eq!(displayed, 1.0);
        for step in 101..200 {
            assert_eq!(
                history.weights(step as f32 * 0.01),
                [None; CUBE_FACE_COUNT as usize]
            );
        }
    }

    #[test]
    fn physical_response_is_independent_of_render_frequency() {
        let sample = |rate: u32| {
            let mut history = refreshed();
            let mut displayed = 0.0;
            for step in 1..=rate / 5 {
                let weight = history.weights(step as f32 / rate as f32)[0].unwrap();
                displayed += (1.0 - displayed) * weight;
            }
            displayed
        };
        let expected = 1.0 - (-0.2 / RESPONSE_SECONDS).exp();
        for rate in [30, 60, 120, 240] {
            assert!((sample(rate) - expected).abs() < 1.0e-6);
        }
    }

    #[test]
    fn rapid_replacements_continue_from_displayed_radiance() {
        let mut history = refreshed();
        let mut displayed = 0.0;
        let mut target = 1.0;
        for step in 1..=60 {
            if step % 4 == 0 {
                target = 1.0 - target;
                assert!(!history.target_changed(0));
            }
            let weight = history.weights(step as f32 * 0.01)[0].unwrap();
            let next = displayed + (target - displayed) * weight;
            assert!((0.0..=1.0).contains(&next));
            assert!((next - displayed).abs() <= 1.0 - (-0.01 / RESPONSE_SECONDS).exp() + 1.0e-6);
            displayed = next;
        }
        assert!(displayed > 0.2 && displayed < 0.8);
    }

    #[test]
    fn resets_publish_immediately_and_faces_have_independent_lifetimes() {
        let mut history = refreshed();
        assert!(history.target_changed(1));
        assert!(history.weights(0.01)[0].is_some());
        assert_eq!(history.weights(0.02)[1], None);
        history.reset();
        assert!(history.target_changed(0));
        assert_eq!(history.weights(3.0), [None; CUBE_FACE_COUNT as usize]);
    }

    #[test]
    fn clock_wrap_preserves_display_and_resumes_on_the_next_frame() {
        let mut history = refreshed();
        assert!(history.weights(5.0)[0].is_some());
        assert!(!history.target_changed(0));
        assert_eq!(history.weights(0.0)[0], None);
        assert!(history.weights(0.01)[0].is_some());
        assert_eq!(history.weights(f32::NAN)[0], None);
    }
}

#[cfg(test)]
#[path = "probe_history_tests.rs"]
mod native_tests;
