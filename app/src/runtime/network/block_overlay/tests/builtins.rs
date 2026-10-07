use super::*;

#[test]
fn versioned_builtin_full_block_resolves_textured_cube_without_pack_geometry() {
    let view = view();
    let blocks = CustomBlocks {
        blocks: [
            "minecraft:geometry.full_block",
            "minecraft:geometry.full_block_v1",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, identifier)| {
            block(
                &format!("test:full_block_{index}"),
                1,
                CustomBlockVisuals {
                    base: CustomVisualComponents {
                        geometry: Some(identifier.into()),
                        materials: Some(Box::new([
                            CustomMaterialInstance {
                                name: "side".into(),
                                texture: "gen".into(),
                                render_method: None,
                                tint_method: None,
                                ambient_occlusion: None,
                                face_dimming: None,
                            },
                            CustomMaterialInstance {
                                name: "up".into(),
                                texture: "lucky".into(),
                                render_method: None,
                                tint_method: None,
                                ambient_occlusion: None,
                                face_dimming: None,
                            },
                            CustomMaterialInstance {
                                name: "down".into(),
                                texture: "lucky".into(),
                                render_method: None,
                                tint_method: None,
                                ambient_occlusion: None,
                                face_dimming: None,
                            },
                        ])),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
        })
        .collect(),
        ..Default::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view, &blocks, hashed, None).unwrap();
        assert_eq!(compiled.gaps.missing_geometry, 0);
        assert_eq!(compiled.gaps.missing_textures, 0);
        for visual in &compiled.overlay.visuals {
            assert_eq!(visual.kind, VisualKind::Cube);
            assert!(
                visual
                    .flags
                    .contains(assets::BlockFlags::OCCLUDES_FULL_FACE)
            );
            assert_eq!(visual.support, VisualSupport::Exact);
            assert!(visual.faces.iter().all(|material| *material != 0));
        }
        let [modern, original]: [assets::BlockVisual; 2] =
            compiled.overlay.visuals[..].try_into().unwrap();
        for face in [
            BlockFace::West,
            BlockFace::East,
            BlockFace::Up,
            BlockFace::North,
            BlockFace::South,
        ] {
            assert_eq!(modern.faces[face as usize], original.faces[face as usize]);
        }
        let down = |visual: assets::BlockVisual| {
            compiled.overlay.materials[visual.faces[BlockFace::Down as usize] as usize]
        };
        assert_eq!(down(modern).texture, down(original).texture);
        assert_eq!(down(modern).animation, down(original).animation);
        for (corner, expected) in [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
            .into_iter()
            .enumerate()
        {
            let original_uv = render::greedy_texture_uv(
                meshing::Face::NegativeY,
                corner as u32,
                1,
                1,
                down(original).flags,
            );
            let modern_uv = render::greedy_texture_uv(
                meshing::Face::NegativeY,
                corner as u32,
                1,
                1,
                down(modern).flags,
            );
            assert_eq!(original_uv, expected);
            assert_eq!(modern_uv, [1.0 - expected[0], 1.0 - expected[1]]);
        }
        assert_ne!(
            original.faces[BlockFace::Up as usize],
            original.faces[BlockFace::North as usize]
        );
    }
}
