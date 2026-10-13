use super::*;

/// Composes the production velocity shader with its shared Rust storage strides.
pub(super) fn shader_source(defines: &[&str]) -> String {
    let source = crate::material_shader::source(include_str!("../../actor.wesl"))
        .replace(
            "ACTOR_GPU_INSTANCE_WORDS",
            &render_model::ACTOR_GPU_INSTANCE_WORDS.to_string(),
        )
        .replace(
            "ACTOR_RIG_VERTEX_WORDS",
            &render_model::ACTOR_RIG_VERTEX_WORDS.to_string(),
        );
    crate::shader_source::standalone_prepared(&source, defines)
}

#[test]
fn actor_velocity_shaders_validate_with_and_without_multisample_cutout_coverage() {
    for defines in [
        &["ACTOR_MOTION"][..],
        &["ACTOR_MOTION", "ALPHA_TO_COVERAGE"][..],
    ] {
        let wgsl = shader_source(defines);
        let module = naga::front::wgsl::parse_str(&wgsl)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&wgsl)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[test]
fn actor_velocity_preserves_cutout_culling_and_reads_depth_without_changing_it() {
    let material =
        assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
            alpha_test: true,
            cull: true,
            depth_write: true,
            ..Default::default()
        }));
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    MotionSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Sample4,
                hdr: false,
                enhanced: false,
                material,
            },
            &mut descriptor,
        )
        .unwrap();
    assert_eq!(
        descriptor.primitive.cull_mode,
        Some(bevy::render::render_resource::Face::Back)
    );
    assert_eq!(descriptor.multisample.count, 4);
    assert!(descriptor.multisample.alpha_to_coverage_enabled);
    let depth = descriptor.depth_stencil.unwrap();
    assert_eq!(depth.depth_compare, Some(CompareFunction::Equal));
    assert_eq!(depth.depth_write_enabled, Some(false));
    assert!(eligible(material));
    assert!(!eligible(assets::EntityRenderMaterial::Default.word(Some(
        assets::EntityRenderMaterialState {
            blend: true,
            depth_write: false,
            ..Default::default()
        }
    ))));
}
