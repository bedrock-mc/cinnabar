//! FSR1 reconstruction and sharpening before native-resolution UI composition.

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::*,
    render::{RenderApp, render_resource::*, renderer::RenderDevice},
};

const SHADER: Handle<Shader> = uuid_handle!("3dd2d5df-3d2c-44af-bbba-f0ff45fc60ed");

/// Loads FSR1 and its bilinear fallback with their per-device pipeline resources.
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
    pub pass: UpscalePass,
}

/// Chooses a filter stage without backend-specific storage or compute features.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum UpscalePass {
    Bilinear,
    Easu,
    Rcas,
}

/// Holds the reconstruction stages by role, independent of their compilation order.
#[derive(Clone, Copy)]
pub(super) struct UpscalePipelines {
    pub bilinear: CachedRenderPipelineId,
    pub easu: CachedRenderPipelineId,
    pub rcas: CachedRenderPipelineId,
}

impl UpscalePipelines {
    /// Enumerates all stages for readiness checks; a new field must be handled here.
    pub(super) fn ids(self) -> [CachedRenderPipelineId; 3] {
        let Self {
            bilinear,
            easu,
            rcas,
        } = self;
        [bilinear, easu, rcas]
    }
}

/// FP32 constants from FsrEasuCon and FsrRcasCon, stored without bit reinterpretation.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct FsrConstants {
    con0: [f32; 4],
    con1: [f32; 4],
    con2: [f32; 4],
    con3: [f32; 4],
    rcas: [f32; 4],
}

impl FsrConstants {
    /// Maps a complete input viewport to output pixels; `None` disables RCAS sharpening.
    pub(super) fn new(input: [u32; 2], output: [u32; 2], sharpness: Option<f32>) -> Self {
        let [ix, iy] = input.map(|n| n as f32);
        let [ox, oy] = output.map(|n| n as f32);
        let [rx, ry] = [ix.recip(), iy.recip()];
        let [sx, sy] = [ix * ox.recip(), iy * oy.recip()];
        Self {
            con0: [sx, sy, 0.5 * sx - 0.5, 0.5 * sy - 0.5],
            con1: [rx, ry, rx, -ry],
            con2: [-rx, 2.0 * ry, rx, 2.0 * ry],
            con3: [0.0, 4.0 * ry, 0.0, 0.0],
            rcas: [
                sharpness.map_or(0.0, |stops| (-stops).exp2()),
                0.0,
                0.0,
                0.0,
            ],
        }
    }
}

struct UpscaleSpecializer;

impl Specializer<RenderPipeline> for UpscaleSpecializer {
    type Key = UpscaleKey;

    /// Selects an FSR stage and preserves the scene colour range through its perceptual transform.
    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.targets[0].as_mut().unwrap().format = key.format;
        fragment.entry_point = Some(
            match key.pass {
                UpscalePass::Bilinear => "bilinear_fragment",
                UpscalePass::Easu => "easu_fragment",
                UpscalePass::Rcas => "rcas_fragment",
            }
            .into(),
        );
        fragment.shader_defs.clear();
        if key.format == render_scene::SCENE_HDR_FORMAT {
            fragment.shader_defs.push("FSR_HDR".into());
        }
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
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(size_of::<FsrConstants>() as u64),
                    },
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

    /// Queues every stage once for a scene format, sharing the draw path with warmup.
    pub(super) fn stages(
        &mut self,
        cache: &PipelineCache,
        format: TextureFormat,
    ) -> UpscalePipelines {
        UpscalePipelines {
            bilinear: self.specialize(
                cache,
                UpscaleKey {
                    format,
                    pass: UpscalePass::Bilinear,
                },
            ),
            easu: self.specialize(
                cache,
                UpscaleKey {
                    format,
                    pass: UpscalePass::Easu,
                },
            ),
            rcas: self.specialize(
                cache,
                UpscaleKey {
                    format,
                    pass: UpscalePass::Rcas,
                },
            ),
        }
    }

    /// Binds a completed world image without sampling the presentation attachment.
    pub(super) fn binding(
        &self,
        device: &RenderDevice,
        cache: &PipelineCache,
        source: &TextureView,
        sampler: &Sampler,
        constants: FsrConstants,
    ) -> BindGroup {
        let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("FSR1 constants"),
            contents: bytemuck::bytes_of(&constants),
            usage: BufferUsages::UNIFORM,
        });
        device.create_bind_group(
            "world upscale",
            &cache.get_bind_group_layout(&self.layout),
            &BindGroupEntries::sequential((source, sampler, buffer.as_entire_binding())),
        )
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for UpscalePipeline {
    /// Warms both FSR passes and their fallback, including HDR camera variants.
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
        ids.extend(self.stages(cache, format).ids());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easu_constants_match_reference_half_and_unequal_scales() {
        // FsrEasuCon with a viewport equal to its source resource.
        let c = FsrConstants::new([1280, 720], [2560, 1440], None);
        assert_eq!(c.con0, [0.5, 0.5, -0.25, -0.25]);
        assert_eq!(c.con1, [0.00078125, 1.0 / 720.0, 0.00078125, -1.0 / 720.0]);
        assert_eq!(c.con2, [-0.00078125, 1.0 / 360.0, 0.00078125, 1.0 / 360.0]);
        assert_eq!(c.con3, [0.0, 1.0 / 180.0, 0.0, 0.0]);
        let c = FsrConstants::new([3, 2], [4, 4], None);
        assert_eq!(c.con0, [0.75, 0.5, -0.125, -0.25]);
        assert_eq!(c.con1, [1.0 / 3.0, 0.5, 1.0 / 3.0, -0.5]);
        assert_eq!(c.con2, [-1.0 / 3.0, 1.0, 1.0 / 3.0, 1.0]);
        assert_eq!(c.con3, [0.0, 2.0, 0.0, 0.0]);
    }

    #[test]
    fn rcas_constants_convert_stops_to_reference_linear_strength() {
        for (stops, expected) in [(0.0, 1.0), (0.2, 0.8705506), (1.0, 0.5), (2.0, 0.25)] {
            let c = FsrConstants::new([1, 1], [1, 1], Some(stops));
            assert!((c.rcas[0] - expected).abs() < 1e-7);
        }
        assert_eq!(FsrConstants::new([1, 1], [1, 1], None).rcas[0], 0.0);
    }
}
