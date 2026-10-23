//! Render-world pipelines for blending animation frames into the atlas.

use super::animate::{
    BLEND_INSTANCE_ATTRIBUTES, BLEND_SOURCE_ENTRIES, BlendInstance, BlendPipelines,
};
use super::upload::{COLOR_STORAGE_FORMAT, PBR_FORMAT};
use bevy::{
    asset::{Handle, uuid_handle},
    mesh::VertexBufferLayout,
    prelude::{Commands, Res, Resource, Shader},
    render::render_resource::{
        BindGroupLayoutDescriptor, CachedRenderPipelineId, ColorTargetState, ColorWrites,
        FragmentState, PipelineCache, RenderPipelineDescriptor, TextureFormat, VertexState,
        VertexStepMode,
    },
};

pub(crate) const BLEND_SHADER: Handle<Shader> =
    uuid_handle!("cb81ab4c-7783-4291-a169-a0a9d1d70c72");

/// Queued blend pipelines and the frame-store layout they bind.
#[derive(Resource)]
pub(crate) struct AtlasBlendPipelines {
    pub layout: BindGroupLayoutDescriptor,
    colour: CachedRenderPipelineId,
    layers: CachedRenderPipelineId,
}

impl AtlasBlendPipelines {
    /// Both pipelines once compiled.
    pub fn get<'a>(&self, cache: &'a PipelineCache) -> Option<BlendPipelines<'a>> {
        Some(BlendPipelines {
            colour: cache.get_render_pipeline(self.colour)?,
            layers: cache.get_render_pipeline(self.layers)?,
        })
    }
}

/// Describes an animation blend pass with the supplied attachment formats.
pub(super) fn descriptor(
    layout: &BindGroupLayoutDescriptor,
    entry_point: &'static str,
    formats: &[TextureFormat],
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("shader-pack animated sprite blend".into()),
        layout: vec![layout.clone()],
        vertex: VertexState {
            shader: BLEND_SHADER,
            entry_point: Some("vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: size_of::<BlendInstance>() as u64,
                step_mode: VertexStepMode::Instance,
                attributes: BLEND_INSTANCE_ATTRIBUTES.to_vec(),
            }],
            ..Default::default()
        },
        fragment: Some(FragmentState {
            shader: BLEND_SHADER,
            entry_point: Some(entry_point.into()),
            targets: formats
                .iter()
                .map(|&format| {
                    Some(ColorTargetState {
                        format,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })
                })
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Queues the colour-only (encoded UNORM storage) pipeline and the colour + LabPBR (linear) one.
pub(crate) fn init_blend_pipelines(
    mut commands: Commands,
    kernels: Option<Res<super::super::kernels::Kernels>>,
    existing: Option<Res<AtlasBlendPipelines>>,
) {
    if existing.is_some() {
        return;
    }
    let Some(kernels) = kernels else {
        return;
    };
    let cache = &kernels.cache;
    let layout =
        BindGroupLayoutDescriptor::new("shader-pack frame blend sources", &BLEND_SOURCE_ENTRIES);
    commands.insert_resource(AtlasBlendPipelines {
        colour: cache.queue_render_pipeline(descriptor(
            &layout,
            "fragment_colour",
            &[COLOR_STORAGE_FORMAT],
        )),
        layers: cache.queue_render_pipeline(descriptor(
            &layout,
            "fragment_layers",
            &[COLOR_STORAGE_FORMAT, PBR_FORMAT, PBR_FORMAT],
        )),
        layout,
    });
}
