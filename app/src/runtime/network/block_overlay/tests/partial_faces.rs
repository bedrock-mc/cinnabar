use super::*;

/// Failed faces keep the diagnostic material while their cube still renders.
#[test]
fn cube_keeps_rendering_with_a_failed_face() {
    let instances =
        [("*", "lucky"), ("up", "absent")].map(|(name, texture)| CustomMaterialInstance {
            name: name.into(),
            texture: texture.into(),
            render_method: None,
            tint_method: None,
            ambient_occlusion: None,
            face_dimming: None,
        });
    let chair = block(
        "test:chair",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: Some(instances.into()),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![chair].into(),
        ..CustomBlocks::default()
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).expect("overlay");
    assert_eq!(compiled.gaps.missing_textures, 1);
    assert_eq!(compiled.overlay.visuals.len(), 1);
    let visual = compiled.overlay.visuals[0];
    assert_eq!(visual.kind, VisualKind::Cube);
    assert_eq!(
        compiled.overlay.materials[visual.faces[BlockFace::Up as usize] as usize],
        compiled.overlay.materials[assets::DIAGNOSTIC_MATERIAL as usize],
        "only the failed top face degrades"
    );
    assert!(
        compiled.overlay.materials[visual.faces[BlockFace::North as usize] as usize]
            .texture
            .page()
            == 1,
        "healthy faces keep their overlay raster"
    );
    let session = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .expect("partial cubes apply");
    assert_eq!(
        session.resolve(NetworkIdMode::Sequential, 1).kind(),
        VisualKind::Cube
    );
}

/// Missing textures and refused materials keep diagnostic quads and authored sidedness.
#[test]
fn model_keeps_quads_and_sidedness_with_failed_materials() {
    let view = view();
    let base = RuntimeAssets::diagnostic();
    let full = base
        .with_block_overlay(
            1,
            &assets::BlockOverlay {
                materials: vec![
                    base.material(0);
                    assets::MAX_MATERIALS - base.materials().len() - 1
                ],
                ..Default::default()
            },
        )
        .expect("reserve all but the diagnostic material slot");
    for (base, texture) in [(&base, "absent"), (&full, "lucky")] {
        for (method, two_sided) in [
            (None, false),
            (Some("opaque"), false),
            (Some("alpha_test"), true),
            (Some("alpha_test_to_opaque"), true),
            (Some("double_sided"), true),
            (Some("blend"), true),
            (Some("blend_to_opaque"), true),
        ] {
            let mut instances = materials(texture).unwrap();
            instances[0].render_method = method.map(Into::into);
            let statue = block(
                "test:statue",
                1,
                CustomBlockVisuals {
                    base: CustomVisualComponents {
                        geometry: Some("geometry.gen".into()),
                        materials: Some(instances),
                        ..CustomVisualComponents::default()
                    },
                    ..CustomBlockVisuals::default()
                },
            );
            let blocks = CustomBlocks {
                blocks: vec![statue].into(),
                ..CustomBlocks::default()
            };
            let compiled = super::super::compile_block_overlay(
                &view,
                &blocks,
                false,
                None,
                assets::MAX_TEXTURE_LAYERS as u32,
                Some(base),
            )
            .expect("overlay");
            assert_eq!(compiled.gaps.missing_textures, 1);
            assert_eq!(compiled.overlay.visuals.len(), 1);
            assert_eq!(compiled.overlay.materials.len(), 1);
            assert_eq!(compiled.overlay.textures[0].layers, 1);
            let visual = compiled.overlay.visuals[0];
            assert_eq!(visual.kind, VisualKind::Model);
            let template = compiled.overlay.model_templates[visual.model_template as usize];
            assert!(template.quad_count > 0, "failed quads stay in the mesh");
            let quads = &compiled.overlay.model_quads[template.quad_start as usize..]
                [..template.quad_count as usize];
            for quad in quads {
                assert_eq!(quad.material, assets::DIAGNOSTIC_MATERIAL);
                assert_eq!(
                    quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED != 0,
                    two_sided,
                    "{texture} {method:?}"
                );
            }
            let session = base
                .with_block_overlay(1, &compiled.overlay)
                .expect("partial models apply");
            assert_eq!(session.model_quads().len(), quads.len());
            for (published, compiled) in session.model_quads().iter().zip(quads) {
                assert_eq!(published.flags, compiled.flags);
            }
        }
    }
}
