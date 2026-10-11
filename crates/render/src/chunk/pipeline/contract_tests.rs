use super::*;

#[test]
fn vanilla_base_pipeline_construction_matches_baseline() {
    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    let mut pipelines = ChunkPipeline::from_world(&mut World::new());
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        for hdr in [false, true] {
            let key = ChunkPipelineKey {
                msaa,
                hdr,
                enhanced: false,
            };
            for (variants, shader, source, blended) in [
                (
                    &mut pipelines.variants,
                    CHUNK_SHADER_HANDLE,
                    include_str!("../../chunk.wesl"),
                    false,
                ),
                (
                    &mut pipelines.model_variants,
                    MODEL_SHADER_HANDLE,
                    include_str!("../../model.wesl"),
                    false,
                ),
                (
                    &mut pipelines.transparent_variants,
                    TRANSPARENT_SHADER_HANDLE,
                    include_str!("../../transparent_terrain.wesl"),
                    true,
                ),
                (
                    &mut pipelines.depth_liquid_variants,
                    LIQUID_SHADER_HANDLE,
                    include_str!("../../liquid.wesl"),
                    false,
                ),
            ] {
                let id = variants.specialize(&cache, key).unwrap();
                let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, id);
                assert_eq!(descriptor.multisample.count, msaa.samples());
                assert_eq!(
                    descriptor.multisample.alpha_to_coverage_enabled,
                    msaa.samples() > 1 && !blended && shader != LIQUID_SHADER_HANDLE
                );
                assert_eq!(descriptor.primitive.cull_mode, None);
                assert_eq!(
                    descriptor.primitive.front_face,
                    if shader == LIQUID_SHADER_HANDLE {
                        bevy::render::render_resource::FrontFace::Cw
                    } else {
                        bevy::render::render_resource::FrontFace::Ccw
                    }
                );
                assert_eq!(descriptor.vertex.shader, shader);
                let fragment_state = descriptor.fragment.as_ref().unwrap();
                assert_eq!(fragment_state.shader, shader);
                // The transparent shader composes the liquid and model modules; only the
                // composer can resolve their shared names.
                let module = if shader == TRANSPARENT_SHADER_HANDLE {
                    crate::shader_source::composed(source, &[])
                } else {
                    crate::shader_source::standalone(source, &[])
                };
                let module = naga::front::wgsl::parse_str(&module).unwrap();
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
                        crate::SCENE_HDR_FORMAT
                    } else if blended {
                        crate::SCENE_COLOR_FORMAT.remove_srgb_suffix()
                    } else {
                        crate::SCENE_COLOR_FORMAT
                    }
                );
                assert_eq!(
                    fragment_state
                        .shader_defs
                        .contains(&"NATIVE_GAMMA_BLEND".into()),
                    blended && !hdr
                );
                assert_eq!(colour.blend, blended.then_some(BlendState::ALPHA_BLENDING));
                assert_eq!(
                    colour.write_mask,
                    if blended {
                        ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE
                    } else {
                        ColorWrites::ALL
                    }
                );
                let depth = descriptor.depth_stencil.as_ref().unwrap();
                assert_eq!(depth.format, CORE_3D_DEPTH_FORMAT);
                assert_eq!(depth.depth_compare, Some(CompareFunction::GreaterEqual));
                assert_eq!(depth.depth_write_enabled, Some(true));
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
    let layout = opaque_chunk_bind_group_layout();
    for source in [
        include_str!("../../chunk.wesl"),
        include_str!("../../model.wesl"),
        include_str!("../../liquid.wesl"),
    ] {
        crate::shader_test_support::assert_binding_visibility(
            &crate::shader_source::standalone(source, &[]),
            0,
            &layout,
        );
    }
    crate::shader_test_support::assert_binding_visibility(
        &crate::shader_source::composed(include_str!("../../transparent_terrain.wesl"), &[]),
        0,
        &chunk_bind_group_layout(),
    );
}

/// Whether `function` or anything it calls can discard.
fn discards(module: &naga::Module, block: &naga::Block) -> bool {
    block.iter().any(|statement| match statement {
        naga::Statement::Kill => true,
        naga::Statement::Block(inner) => discards(module, inner),
        naga::Statement::If { accept, reject, .. } => {
            discards(module, accept) || discards(module, reject)
        }
        naga::Statement::Switch { cases, .. } => {
            cases.iter().any(|case| discards(module, &case.body))
        }
        naga::Statement::Loop {
            body, continuing, ..
        } => discards(module, body) || discards(module, continuing),
        naga::Statement::Call { function, .. } => {
            discards(module, &module.functions[*function].body)
        }
        _ => false,
    })
}

#[test]
fn solid_cube_pipeline_culls_back_faces_without_fragment_discards() {
    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    let mut pipelines = ChunkPipeline::from_world(&mut World::new());
    let source = crate::shader_source::standalone(include_str!("../../chunk.wesl"), &[]);
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let entry = |name: &str| {
        let entry = module.entry_points.iter().find(|entry| entry.name == name);
        &entry.expect("fragment entry exists").function.body
    };
    assert!(
        discards(&module, entry("fragment")),
        "cutout keeps its gates"
    );
    assert!(!discards(&module, entry("fragment_solid")));
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        for hdr in [false, true] {
            let key = ChunkPipelineKey {
                msaa,
                hdr,
                enhanced: false,
            };
            let solid = pipelines.solid_variants.specialize(&cache, key).unwrap();
            let solid = crate::queue_review_support::queued_descriptor(&mut cache, solid).clone();
            let cutout = pipelines.variants.specialize(&cache, key).unwrap();
            let cutout = crate::queue_review_support::queued_descriptor(&mut cache, cutout);
            assert_eq!(
                solid.primitive.cull_mode,
                Some(bevy::render::render_resource::Face::Back)
            );
            assert_eq!(cutout.primitive.cull_mode, None);
            let fragment = solid.fragment.as_ref().unwrap();
            assert_eq!(fragment.entry_point.as_deref(), Some("fragment_solid"));
            // Solid cubes retain their own culling and full sample coverage.
            let mut expected = cutout.clone();
            expected.label = solid.label.clone();
            expected.primitive.cull_mode = solid.primitive.cull_mode;
            expected.fragment.as_mut().unwrap().entry_point = fragment.entry_point.clone();
            expected.multisample.alpha_to_coverage_enabled = false;
            expected
                .fragment
                .as_mut()
                .unwrap()
                .shader_defs
                .retain(|definition| definition != &crate::alpha_coverage::SHADER_DEF.into());
            assert_eq!(format!("{solid:?}"), format!("{expected:?}"));
        }
    }
}

#[test]
fn cutout_pipeline_warmup_covers_every_sample_count() {
    use crate::pipeline_warmup::{PrewarmPipelines, WarmView};
    let (app, _) = crate::queue_review_support::app();
    let cache = app.world().resource::<PipelineCache>();
    let mut pipelines = ChunkPipeline::from_world(&mut World::new());
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        for hdr in [false, true] {
            let mut ids = Vec::new();
            pipelines
                .prewarm(
                    cache,
                    WarmView {
                        msaa,
                        hdr,
                        enhanced: false,
                        output: None,
                    },
                    &mut ids,
                )
                .unwrap();
            let key = ChunkPipelineKey {
                msaa,
                hdr,
                enhanced: false,
            };
            for variants in [&mut pipelines.variants, &mut pipelines.model_variants] {
                assert!(ids.contains(&variants.specialize(cache, key).unwrap()));
            }
        }
    }
}
