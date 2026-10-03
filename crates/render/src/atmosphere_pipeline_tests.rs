use super::*;

#[test]
fn atmosphere_pipeline_specializes_msaa_and_keeps_reversed_z_without_depth_writes() {
    let (app, _) = crate::queue_review_support::app();
    let cache = app.world().resource::<PipelineCache>();
    let mut pipeline = AtmospherePipeline::from_world(&mut World::new());
    crate::shader_test_support::assert_binding_visibility(
        &crate::shader_source::standalone(include_str!("atmosphere.wgsl"), &[]),
        0,
        &pipeline.bind_group_layout,
    );
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        for hdr in [false, true] {
            let id = pipeline
                .variants
                .specialize(cache, AtmospherePipelineKey { msaa, hdr })
                .unwrap();
            let descriptor = cache.get_render_pipeline_descriptor(id);
            assert_eq!(descriptor.multisample.count, msaa.samples());
            let depth = descriptor.depth_stencil.as_ref().unwrap();
            assert_eq!(depth.format, CORE_3D_DEPTH_FORMAT);
            assert_eq!(depth.depth_compare, CompareFunction::GreaterEqual);
            assert!(!depth.depth_write_enabled);
            let colour = descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap();
            assert_eq!(colour.blend, Some(BlendState::ALPHA_BLENDING));
            assert_eq!(
                colour.format,
                if hdr {
                    ViewTarget::TEXTURE_FORMAT_HDR
                } else {
                    TextureFormat::bevy_default()
                }
            );
        }
    }
}
