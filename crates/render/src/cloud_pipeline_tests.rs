use super::*;

#[test]
fn cloud_pipeline_is_transparent_depth_aware_and_specializes_from_each_view() {
    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    let mut pipeline = CloudPipeline::from_world(&mut World::new());
    crate::shader_test_support::assert_binding_visibility(
        &crate::shader_source::standalone(include_str!("cloud.wgsl"), &[]),
        0,
        &pipeline.bind_group_layout,
    );
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        for hdr in [false, true] {
            let id = pipeline
                .variants
                .specialize(&cache, CloudPipelineKey { msaa, hdr })
                .unwrap();
            let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, id);
            assert_eq!(descriptor.multisample.count, msaa.samples());
            assert_eq!(descriptor.primitive.front_face, FrontFace::Ccw);
            assert_eq!(descriptor.primitive.cull_mode, Some(Face::Back));
            let depth = descriptor.depth_stencil.as_ref().unwrap();
            assert_eq!(depth.format, CORE_3D_DEPTH_FORMAT);
            assert_eq!(depth.depth_compare, CompareFunction::Greater);
            assert!(!depth.depth_write_enabled);
            let colour = descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap();
            assert_eq!(colour.blend, Some(BlendState::ALPHA_BLENDING));
            assert_eq!(
                colour.write_mask,
                ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE
            );
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
