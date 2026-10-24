use super::*;

/// Exercise the descriptors actually consumed by specialization, for both storage layouts.
#[test]
fn terrain_descriptors_preserve_model_liquid_and_blend_contracts() {
    for compact in [false, true] {
        let [cutout, solid, model, transparent, liquid] = build(compact);
        assert_eq!(
            solid.primitive.cull_mode,
            Some(bevy::render::render_resource::Face::Back)
        );
        for descriptor in [&cutout, &model, &transparent, &liquid] {
            assert_eq!(descriptor.primitive.cull_mode, None);
        }
        assert_eq!(model.vertex.shader, MODEL_SHADER_HANDLE);
        assert_eq!(model.fragment.as_ref().unwrap().shader, MODEL_SHADER_HANDLE);
        assert_eq!(liquid.vertex.shader, LIQUID_SHADER_HANDLE);
        assert_eq!(liquid.vertex.entry_point.as_deref(), Some("vertex_depth"));
        assert_eq!(
            liquid.fragment.as_ref().unwrap().entry_point.as_deref(),
            Some("fragment_depth")
        );
        for descriptor in [&model, &liquid] {
            assert_eq!(
                descriptor.fragment.as_ref().unwrap().targets[0]
                    .as_ref()
                    .unwrap()
                    .blend,
                None
            );
            assert_eq!(
                descriptor
                    .depth_stencil
                    .as_ref()
                    .unwrap()
                    .depth_write_enabled,
                Some(true)
            );
        }
        let fragment = transparent.fragment.as_ref().unwrap();
        let target = fragment.targets[0].as_ref().unwrap();
        assert_eq!(target.blend, Some(BlendState::ALPHA_BLENDING));
        assert_eq!(
            target.write_mask,
            ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE
        );
        assert!(fragment.shader_defs.contains(&"NATIVE_GAMMA_BLEND".into()));
        assert_eq!(
            transparent
                .depth_stencil
                .as_ref()
                .unwrap()
                .depth_write_enabled,
            Some(true)
        );
        assert_eq!(
            transparent.depth_stencil.as_ref().unwrap().depth_compare,
            Some(CompareFunction::GreaterEqual)
        );
        assert!(transparent.vertex.buffers.is_empty());
        assert_eq!(
            model.layout.len(),
            2,
            "geometry and lighting bindings are shared"
        );
        assert_eq!(liquid.layout.len(), model.layout.len());
    }
}
