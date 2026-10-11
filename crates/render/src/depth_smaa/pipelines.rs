use super::*;
use crate::pipeline_warmup::{PrewarmPipelines, WarmView, WarmupIds};
use bevy::{
    anti_alias::smaa::SmaaInfoUniform,
    asset::uuid_handle,
    render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer},
};

pub(super) const EDGE_SHADER: Handle<Shader> = uuid_handle!("f07777f3-8f47-4673-abca-02296cbd4521");
pub(super) const RESTORE_SHADER: Handle<Shader> =
    uuid_handle!("8c730613-b95d-4dbf-956d-d1fb35e56f17");

#[derive(Clone, Copy)]
pub(super) struct PipelineIds {
    pub edge: CachedRenderPipelineId,
    pub weights: CachedRenderPipelineId,
    pub blend: CachedRenderPipelineId,
    pub restore: CachedRenderPipelineId,
}

#[derive(Resource)]
pub(super) struct DepthSmaaPipelines {
    pub post_layout: BindGroupLayoutDescriptor,
    pub restore_layout: BindGroupLayoutDescriptor,
    weights_layout: BindGroupLayoutDescriptor,
    blend_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    variants: Vec<(u32, TextureFormat, PipelineIds)>,
}

/// Loads Bevy's registered SMAA shader instead of duplicating its search and blending code.
pub(super) fn init(mut commands: Commands, server: Res<AssetServer>) {
    let float = texture_2d(TextureSampleType::Float { filterable: true });
    commands.insert_resource(DepthSmaaPipelines {
        post_layout: BindGroupLayoutDescriptor::new(
            "SMAA postprocess bind group layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    float,
                    uniform_buffer::<SmaaInfoUniform>(true)
                        .visibility(ShaderStages::VERTEX_FRAGMENT),
                ),
            ),
        ),
        weights_layout: BindGroupLayoutDescriptor::new(
            "SMAA blending weight calculation bind group layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (float, sampler(SamplerBindingType::Filtering), float, float),
            ),
        ),
        blend_layout: BindGroupLayoutDescriptor::new(
            "SMAA neighborhood blending bind group layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (float, sampler(SamplerBindingType::Filtering)),
            ),
        ),
        restore_layout: BindGroupLayoutDescriptor::new(
            "SMAA restore",
            &BindGroupLayoutEntries::single(ShaderStages::FRAGMENT, float),
        ),
        shader: server.load("embedded://bevy_anti_alias/smaa/smaa.wgsl"),
        variants: Vec::new(),
    });
}

impl DepthSmaaPipelines {
    /// Counts cached sample-count and colour-format variants for the warmup contract.
    #[cfg(test)]
    pub(super) fn variants_len(&self) -> usize {
        self.variants.len()
    }

    /// The edge pass binds only depth, with the actual scene sample count.
    pub(super) fn depth_layout(&self, samples: u32) -> BindGroupLayoutDescriptor {
        BindGroupLayoutDescriptor::new(
            "SMAA world depth",
            &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Depth,
                    view_dimension: TextureViewDimension::D2,
                    multisampled: samples > 1,
                },
                count: None,
            }],
        )
    }

    /// Memoizes the same finite pipeline keys used by warmup and drawing.
    pub(super) fn ids(
        &mut self,
        cache: &PipelineCache,
        samples: u32,
        format: TextureFormat,
    ) -> PipelineIds {
        if let Some((_, _, ids)) = self
            .variants
            .iter()
            .find(|(s, f, _)| *s == samples && *f == format)
        {
            return *ids;
        }
        let ids = PipelineIds {
            edge: cache.queue_render_pipeline(self.edge_descriptor(samples)),
            weights: cache.queue_render_pipeline(self.blend_descriptor(true, format)),
            blend: cache.queue_render_pipeline(self.blend_descriptor(false, format)),
            restore: cache.queue_render_pipeline(self.restore_descriptor(samples, format)),
        };
        self.variants.push((samples, format, ids));
        ids
    }

    /// Marks only depth discontinuities in the stencil for Bevy's spatial weight search.
    fn edge_descriptor(&self, samples: u32) -> RenderPipelineDescriptor {
        let defs = if samples > 1 {
            vec!["MULTISAMPLED".into()]
        } else {
            vec![]
        };
        let mut descriptor = descriptor(
            EDGE_SHADER,
            "vertex",
            "fragment",
            defs,
            vec![self.depth_layout(samples)],
            TextureFormat::Rg8Unorm,
        );
        descriptor.depth_stencil = Some(stencil(true));
        descriptor
    }

    /// Runs stock SMAA 1x with fixed High quality and no temporal subsamples.
    fn blend_descriptor(&self, weights: bool, format: TextureFormat) -> RenderPipelineDescriptor {
        let (define, vertex, fragment, layout) = if weights {
            (
                "SMAA_BLENDING_WEIGHT_CALCULATION",
                "blending_weight_calculation_vertex_main",
                "blending_weight_calculation_fragment_main",
                self.weights_layout.clone(),
            )
        } else {
            (
                "SMAA_NEIGHBORHOOD_BLENDING",
                "neighborhood_blending_vertex_main",
                "neighborhood_blending_fragment_main",
                self.blend_layout.clone(),
            )
        };
        let mut descriptor = descriptor(
            self.shader.clone(),
            vertex,
            fragment,
            vec![define.into(), "SMAA_PRESET_HIGH".into()],
            vec![self.post_layout.clone(), layout],
            if weights {
                TextureFormat::Rgba8Unorm
            } else {
                format
            },
        );
        if weights {
            descriptor.depth_stencil = Some(stencil(false));
        }
        descriptor
    }

    /// Broadcasts the filtered world back to existing samples so overlays stay outside SMAA.
    fn restore_descriptor(&self, samples: u32, format: TextureFormat) -> RenderPipelineDescriptor {
        let mut descriptor = descriptor(
            RESTORE_SHADER,
            "vertex",
            "fragment",
            vec![],
            vec![self.restore_layout.clone()],
            format,
        );
        descriptor.multisample.count = samples;
        descriptor
    }
}

impl PrewarmPipelines for DepthSmaaPipelines {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError> {
        let format = if view.hdr {
            crate::SCENE_HDR_FORMAT
        } else {
            crate::SCENE_COLOR_FORMAT
        };
        let pipelines = self.ids(cache, view.msaa.samples(), format);
        ids.extend([
            pipelines.edge,
            pipelines.weights,
            pipelines.blend,
            pipelines.restore,
        ]);
        Ok(())
    }
}

/// Describes a spatial full-screen pass without blending or additional scene attachments.
fn descriptor(
    shader: Handle<Shader>,
    vertex: &'static str,
    fragment: &'static str,
    defs: Vec<bevy::shader::ShaderDefVal>,
    layout: Vec<BindGroupLayoutDescriptor>,
    format: TextureFormat,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("depth SMAA 1x".into()),
        layout,
        vertex: VertexState {
            shader: shader.clone(),
            shader_defs: defs.clone(),
            entry_point: Some(vertex.into()),
            buffers: vec![],
        },
        fragment: Some(FragmentState {
            shader,
            shader_defs: defs,
            entry_point: Some(fragment.into()),
            targets: vec![Some(ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        ..default()
    }
}

/// Shares Bevy's stencil convention: one marks an edge and gates its weight search.
fn stencil(write: bool) -> DepthStencilState {
    let face = StencilFaceState {
        compare: if write {
            CompareFunction::Always
        } else {
            CompareFunction::Equal
        },
        fail_op: StencilOperation::Keep,
        depth_fail_op: StencilOperation::Keep,
        pass_op: if write {
            StencilOperation::Replace
        } else {
            StencilOperation::Keep
        },
    };
    DepthStencilState {
        format: TextureFormat::Stencil8,
        depth_write_enabled: Some(false),
        depth_compare: Some(CompareFunction::Always),
        stencil: StencilState {
            front: face,
            back: face,
            read_mask: 1,
            write_mask: u32::from(write),
        },
        bias: default(),
    }
}
