use super::*;

#[test]
fn vanilla_base_pipeline_construction_matches_baseline() {
    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    let mut pipelines = ChunkPipeline::from_world(&mut World::new());
    for msaa in [Msaa::Off, Msaa::Sample4] {
        for hdr in [false, true] {
            let key = ChunkPipelineKey {
                msaa,
                hdr,
                enhanced: false,
            };
            for (variants, shader, source, blended, cull) in [
                (
                    &mut pipelines.variants,
                    CHUNK_SHADER_HANDLE,
                    include_str!("../../chunk.wgsl"),
                    false,
                    Some(CullFace::Back),
                ),
                (
                    &mut pipelines.model_variants,
                    MODEL_SHADER_HANDLE,
                    include_str!("../../model.wgsl"),
                    false,
                    None,
                ),
                (
                    &mut pipelines.transparent_model_variants,
                    MODEL_SHADER_HANDLE,
                    include_str!("../../model.wgsl"),
                    true,
                    None,
                ),
                (
                    &mut pipelines.liquid_variants,
                    LIQUID_SHADER_HANDLE,
                    include_str!("../../liquid.wgsl"),
                    true,
                    None,
                ),
                (
                    &mut pipelines.depth_liquid_variants,
                    LIQUID_SHADER_HANDLE,
                    include_str!("../../liquid.wgsl"),
                    false,
                    None,
                ),
            ] {
                let id = variants.specialize(&cache, key).unwrap();
                let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, id);
                assert_eq!(descriptor.multisample.count, msaa.samples());
                assert_eq!(descriptor.primitive.cull_mode, cull);
                assert_eq!(descriptor.vertex.shader, shader);
                let fragment_state = descriptor.fragment.as_ref().unwrap();
                assert_eq!(fragment_state.shader, shader);
                let module =
                    naga::front::wgsl::parse_str(&crate::shader_source::standalone(source, &[]))
                        .unwrap();
                for (stage, name) in [
                    (
                        naga::ShaderStage::Vertex,
                        descriptor.vertex.entry_point.as_deref(),
                    ),
                    (
                        naga::ShaderStage::Fragment,
                        fragment_state.entry_point.as_deref(),
                    ),
                ] {
                    let candidates = module
                        .entry_points
                        .iter()
                        .filter(|entry| entry.stage == stage);
                    assert_eq!(
                        candidates
                            .filter(|entry| name.is_none_or(|name| entry.name == name))
                            .count(),
                        1,
                        "the selected entry point must exist and be unambiguous"
                    );
                }
                let colour = fragment_state.targets[0].as_ref().unwrap();
                assert_eq!(
                    colour.format,
                    if hdr {
                        ViewTarget::TEXTURE_FORMAT_HDR
                    } else {
                        TextureFormat::bevy_default()
                    }
                );
                assert_eq!(colour.blend, blended.then_some(BlendState::ALPHA_BLENDING));
                let depth = descriptor.depth_stencil.as_ref().unwrap();
                assert_eq!(depth.format, CORE_3D_DEPTH_FORMAT);
                assert_eq!(depth.depth_compare, CompareFunction::GreaterEqual);
                assert_eq!(depth.depth_write_enabled, !blended);
                for definition in descriptor
                    .vertex
                    .shader_defs
                    .iter()
                    .chain(&fragment_state.shader_defs)
                {
                    let name = match definition {
                        bevy::shader::ShaderDefVal::Bool(name, _)
                        | bevy::shader::ShaderDefVal::Int(name, _)
                        | bevy::shader::ShaderDefVal::UInt(name, _) => name,
                    };
                    assert!(
                        name != "ENHANCED" && name != "ENHANCED_SHADOW",
                        "vanilla specialization must keep Enhanced lighting disabled"
                    );
                }
            }
        }
    }
}

#[test]
fn world_bindings_are_visible_to_the_stages_that_use_them() {
    let layout = chunk_bind_group_layout();
    for source in [
        include_str!("../../chunk.wgsl"),
        include_str!("../../model.wgsl"),
        include_str!("../../liquid.wgsl"),
    ] {
        crate::shader_test_support::assert_binding_visibility(
            &crate::shader_source::standalone(source, &[]),
            0,
            &layout,
        );
    }
}
