//! Canonical descriptors shared by specialization and descriptor contract tests.
use super::*;

/// Builds all terrain variants before specialization adds per-view shader definitions.
pub(super) fn build(compact: bool) -> [RenderPipelineDescriptor; 5] {
    let transparent_bind_group_layout = if compact {
        compact_chunk_bind_group_layout()
    } else {
        chunk_bind_group_layout()
    };
    let bind_group_layout = if compact {
        transparent_bind_group_layout.clone()
    } else {
        opaque_chunk_bind_group_layout()
    };
    let mut descriptor = RenderPipelineDescriptor {
        label: Some("packed chunk pipeline".into()),
        layout: vec![bind_group_layout.clone(), crate::lighting::layout()],
        vertex: VertexState {
            shader: CHUNK_SHADER_HANDLE,
            // Opaque terrain fetches its vertex index and first instance; see `chunk.wesl`.
            buffers: vec![draw_offsets_layout()],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: CHUNK_SHADER_HANDLE,
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: render_scene::SCENE_COLOR_FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        primitive: PrimitiveState {
            // Native cutout leaves disable culling; opaque/deep faces keep
            // their single-sided policy through the material fragment gate.
            cull_mode: None,
            ..default()
        },
        depth_stencil: Some(DepthStencilState {
            format: CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::GreaterEqual),
            stencil: default(),
            bias: default(),
        }),
        ..default()
    };
    if compact {
        descriptor.vertex.shader_defs.push("PACKED_TERRAIN".into());
        descriptor
            .fragment
            .as_mut()
            .unwrap()
            .shader_defs
            .push("PACKED_TERRAIN".into());
    }
    let mut model_descriptor = descriptor.clone();
    model_descriptor.label = Some("packed model pipeline".into());
    model_descriptor.vertex.shader = MODEL_SHADER_HANDLE;
    model_descriptor
        .fragment
        .as_mut()
        .expect("model fragment")
        .shader = MODEL_SHADER_HANDLE;
    model_descriptor
        .fragment
        .as_mut()
        .expect("model fragment")
        .entry_point = Some("fragment".into());
    model_descriptor.primitive.cull_mode = None;
    // Single-sided opaque cube runs: hardware culling replaces both fragment discards.
    let mut solid_descriptor = descriptor.clone();
    solid_descriptor.label = Some("packed solid chunk pipeline".into());
    solid_descriptor.primitive.cull_mode = Some(bevy::render::render_resource::Face::Back);
    solid_descriptor
        .fragment
        .as_mut()
        .expect("solid fragment")
        .entry_point = Some("fragment_solid".into());
    let mut transparent_descriptor = descriptor.clone();
    transparent_descriptor.label = Some("packed transparent terrain pipeline".into());
    transparent_descriptor.vertex.shader = TRANSPARENT_SHADER_HANDLE;
    transparent_descriptor.vertex.entry_point = Some("vertex".into());
    // Sorted transparency is never GPU-culled and indexes its refs by instance.
    transparent_descriptor.vertex.buffers.clear();
    transparent_descriptor.layout[0] = transparent_bind_group_layout.clone();
    let transparent_fragment = transparent_descriptor
        .fragment
        .as_mut()
        .expect("transparent fragment");
    transparent_fragment.shader = TRANSPARENT_SHADER_HANDLE;
    transparent_fragment.entry_point = Some("fragment".into());
    terrain_blend::apply(&mut transparent_descriptor);
    transparent_descriptor.primitive.cull_mode = None;
    let mut depth_liquid_descriptor = descriptor.clone();
    depth_liquid_descriptor.label = Some("packed depth-writing liquid pipeline".into());
    depth_liquid_descriptor.vertex.shader = LIQUID_SHADER_HANDLE;
    depth_liquid_descriptor.vertex.entry_point = Some("vertex_depth".into());
    let depth_fragment = depth_liquid_descriptor
        .fragment
        .as_mut()
        .expect("depth-writing liquid fragment");
    depth_fragment.shader = LIQUID_SHADER_HANDLE;
    depth_fragment.entry_point = Some("fragment_depth".into());
    depth_liquid_descriptor.primitive.cull_mode = None;
    depth_liquid_descriptor.primitive.front_face = LIQUID_FRONT_FACE;
    // Apply after cloning: model geometry includes the opaque stairs/slabs
    // that must win a shared plane against a two-sided cutout cube.
    terrain_cutout::apply(&mut descriptor);
    [
        descriptor,
        solid_descriptor,
        model_descriptor,
        transparent_descriptor,
        depth_liquid_descriptor,
    ]
}

#[cfg(test)]
mod tests;
