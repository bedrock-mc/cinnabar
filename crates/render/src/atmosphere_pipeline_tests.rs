use super::*;

#[test]
fn atmosphere_pipeline_specializes_msaa_and_keeps_reversed_z_without_depth_writes() {
    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    let mut pipeline = AtmospherePipeline::from_world(&mut World::new());
    crate::shader_test_support::assert_binding_visibility(
        &crate::shader_source::standalone(include_str!("atmosphere.wgsl"), &[]),
        0,
        &pipeline.bind_group_layout,
    );
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        for hdr in [false, true] {
            for stars in [false, true] {
                let id = pipeline
                    .variants
                    .specialize(&cache, AtmospherePipelineKey { msaa, hdr, stars })
                    .unwrap();
                let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, id);
                assert_eq!(descriptor.multisample.count, msaa.samples());
                let depth = descriptor.depth_stencil.as_ref().unwrap();
                assert_eq!(depth.format, CORE_3D_DEPTH_FORMAT);
                assert_eq!(depth.depth_compare, Some(CompareFunction::GreaterEqual));
                assert_eq!(depth.depth_write_enabled, Some(false));
                let colour = descriptor.fragment.as_ref().unwrap().targets[0]
                    .as_ref()
                    .unwrap();
                assert_eq!(colour.blend, stars.then(native_star_blend));
                assert_eq!(
                    colour.write_mask,
                    ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE
                );
                assert_eq!(
                    colour.format,
                    if hdr {
                        crate::SCENE_HDR_FORMAT
                    } else {
                        crate::SCENE_COLOR_FORMAT
                    }
                );
            }
        }
    }
}
