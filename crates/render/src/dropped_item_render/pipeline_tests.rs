use bevy::render::render_resource::ShaderStages;

#[test]
fn production_item_shader_resolves_shared_foil_constants() {
    let shader = super::item_shader(include_str!("../dropped_item.wesl"), "dropped_item.wesl");
    let bevy::shader::Source::Wesl(source) = shader.source else {
        unreachable!()
    };
    for definitions in [&[][..], &["ALPHA_TO_COVERAGE"][..]] {
        let source = crate::shader_source::standalone_prepared(&source, definitions);
        let module = naga::front::wgsl::parse_str(&source).expect("production item shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("production item shader validates");
    }
}

// The item fragment stage reads the view for distance fog; a vertex-only binding fails validation.
#[test]
fn fragment_view_reads_are_visible_to_the_fragment_stage() {
    let source = crate::shader_source::composed(include_str!("../dropped_item.wesl"), &[]);
    assert!(crate::shader_test_support::fragment_reads_binding(
        &source, 0, 0
    ));
    assert!(
        super::item_bind_group_layout().entries[0]
            .visibility
            .contains(ShaderStages::FRAGMENT)
    );
}

#[test]
fn dropped_item_coverage_is_prewarmed_at_each_sample_count() {
    use super::*;
    use crate::pipeline_warmup::{PrewarmPipelines, WarmView};
    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    let mut pipeline = ItemPipeline::from_world(&mut World::new());
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        let mut ids = Vec::new();
        pipeline
            .prewarm(
                &cache,
                WarmView {
                    msaa,
                    hdr: false,
                    enhanced: false,
                    shader_pack: false,
                    output: None,
                },
                &mut ids,
            )
            .unwrap();
        for cull_back in [false, true]
            .into_iter()
            .filter(|&cull| !cull || super::closed_item_culling(msaa))
        {
            for invert_culling in [false, true] {
                let id = pipeline
                    .variants
                    .specialize(
                        &cache,
                        ItemPipelineKey {
                            msaa,
                            hdr: false,
                            cull_back,
                            invert_culling,
                        },
                    )
                    .unwrap();
                assert!(ids.contains(&id));
                let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, id);
                assert_eq!(
                    descriptor.multisample.alpha_to_coverage_enabled,
                    msaa.samples() > 1
                );
                assert_eq!(
                    descriptor.primitive.cull_mode,
                    cull_back.then_some(Face::Back)
                );
                assert_eq!(
                    descriptor.primitive.front_face,
                    if invert_culling {
                        FrontFace::Cw
                    } else {
                        FrontFace::Ccw
                    }
                );
            }
        }
    }
}

#[test]
fn alpha_to_coverage_keeps_rear_surfaces_available() {
    assert!(super::closed_item_culling(super::Msaa::Off));
    for msaa in [
        super::Msaa::Sample2,
        super::Msaa::Sample4,
        super::Msaa::Sample8,
    ] {
        assert!(!super::closed_item_culling(msaa));
    }
}
