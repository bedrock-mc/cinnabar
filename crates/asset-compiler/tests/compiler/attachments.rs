use super::support::*;

fn generated_vine_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:vine")
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        record
            .model_state
            .get(ModelStateField::Connections)
            .expect("vine direction bits")
    });
    assert_eq!(records.len(), 16, "protocol-1001 vine state count");
    for (mask, record) in records.iter_mut().enumerate() {
        assert_eq!(
            record.model_state.get(ModelStateField::Connections),
            Some(mask as u32),
            "protocol-1001 vine mask ordering"
        );
        record.model_family = ModelFamily::Vine;
        record.sequential_id = mask as u32;
        record.network_hash = 20_000 + mask as u32;
    }
    records
}

fn oriented_vine_pixels() -> Vec<[u8; 4]> {
    (0..TILE_SIZE)
        .flat_map(|y| {
            (0..TILE_SIZE).map(move |x| {
                [
                    3 + x as u8 * 11,
                    5 + y as u8 * 13,
                    7 + (x as u8 ^ y as u8) * 9,
                    255,
                ]
            })
        })
        .collect()
}

#[test]
fn compiler_compiles_all_vine_masks_as_exact_tinted_attachment_planes() {
    let directory = tempfile::tempdir().expect("create vine fixture");
    write_pack(
        directory.path(),
        r#"{
            "vine":{"textures":"vine"},
            "decoy":{"textures":"decoy"}
        }"#,
        r#"{"texture_data":{
            "vine":{"textures":"textures/blocks/vine"},
            "decoy":{"textures":"textures/blocks/decoy"}
        }}"#,
        "[]",
    );
    let vine_pixels = oriented_vine_pixels();
    write_png(
        directory.path(),
        "textures/blocks/vine",
        TILE_SIZE,
        TILE_SIZE,
        &vine_pixels,
    );
    write_png(
        directory.path(),
        "textures/blocks/decoy",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [211, 3, 149, 255]),
    );

    let compiled = compile_pack(directory.path(), &generated_vine_records())
        .expect("compile every protocol-1001 vine mask");
    assert_eq!(compiled.visuals.len(), 16);
    assert_eq!(
        compiled.model_templates.len(),
        16,
        "one compact template per mask"
    );
    assert_eq!(
        compiled.model_quads.len(),
        32,
        "four bits each occur in eight masks"
    );

    let expected_planes = [
        (
            1_u32,
            6_u32,
            0_usize,
            [[0, 0, 255], [256, 0, 255], [256, 256, 255], [0, 256, 255]],
        ),
        (
            2_u32,
            3_u32,
            2_usize,
            [[1, 0, 0], [1, 0, 256], [1, 256, 256], [1, 256, 0]],
        ),
        (
            4_u32,
            5_u32,
            0_usize,
            [[0, 0, 1], [0, 256, 1], [256, 256, 1], [256, 0, 1]],
        ),
        (
            8_u32,
            4_u32,
            2_usize,
            [[255, 0, 0], [255, 256, 0], [255, 256, 256], [255, 0, 256]],
        ),
    ];
    let expected_rgba = vine_pixels
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .collect::<Vec<_>>();

    for (mask, visual) in compiled.visuals.iter().enumerate() {
        assert_eq!(
            visual.kind,
            VisualKind::Model,
            "mask {mask} diagnostic fallback"
        );
        assert_ne!(
            visual.model_template,
            assets::NO_MODEL_TEMPLATE,
            "mask {mask}"
        );
        assert!(
            !visual.flags.intersects(
                BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
            ),
            "mask {mask}: fake full-block semantics"
        );
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.flags, 0, "mask {mask}");
        assert_eq!(
            template.quad_count,
            (mask as u32).count_ones(),
            "mask {mask}"
        );
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        let expected = expected_planes
            .iter()
            .filter(|(bit, _, _, _)| mask as u32 & bit != 0)
            .collect::<Vec<_>>();
        assert_eq!(quads.len(), expected.len(), "mask {mask}");
        for (quad, (bit, face, tangent_axis, positions)) in quads.iter().zip(expected) {
            assert_eq!(quad.positions, *positions, "mask {mask} bit {bit}");
            for (position, uv) in quad.positions.iter().zip(quad.uvs) {
                assert_eq!(
                    uv,
                    [
                        position[*tangent_axis] as u16 * 16,
                        (256 - position[1] as u16) * 16,
                    ],
                    "mask {mask} bit {bit}: UV must preserve the asymmetric texture's horizontal and vertical axes"
                );
            }
            assert_eq!(
                quad.flags,
                MODEL_QUAD_FLAG_TWO_SIDED | face,
                "mask {mask} bit {bit}: attachment planes must be two-sided and never support-culled"
            );
            assert_eq!(
                quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK,
                0,
                "mask {mask} bit {bit}"
            );
            let material = compiled.materials[quad.material as usize];
            assert_eq!(
                material.flags,
                MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT,
                "mask {mask} bit {bit}"
            );
            assert_eq!(
                mip_layer(&compiled, 0, material.texture.layer()),
                expected_rgba,
                "mask {mask} bit {bit}: selected the wrong terrain layer"
            );
            assert!(
                quad.positions.iter().any(|position| position[1] == 0)
                    && quad.positions.iter().any(|position| position[1] == 256),
                "mask {mask} bit {bit}: unexpected horizontal/top attachment plane"
            );
        }
    }

    let bytes = encode_blob(&compiled).expect("encode all vine templates, including mask zero");
    let runtime = RuntimeAssets::decode(&bytes).expect("decode all vine templates");
    assert_eq!(runtime.model_templates(), compiled.model_templates.as_ref());
    assert_eq!(runtime.model_quads(), compiled.model_quads.as_ref());
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_vine_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile requested pinned pack");
    let vines = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Vine)
        .collect::<Vec<_>>();
    assert_eq!(vines.len(), 16, "protocol-1001 vine state count");
    for record in vines {
        let mask = record
            .model_state
            .get(ModelStateField::Connections)
            .expect("vine direction bits");
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "mask {mask}");
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.quad_count, mask.count_ones(), "mask {mask}");
        for quad in &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
        {
            assert_eq!(
                compiled.materials[quad.material as usize].flags,
                MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT,
                "mask {mask}"
            );
        }
    }
}

fn generated_multiface_records(
    name: &str,
    family: ModelFamily,
    sequential_start: u32,
) -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == name)
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        record
            .model_state
            .get(ModelStateField::Connections)
            .expect("multi-face direction bits")
    });
    assert_eq!(records.len(), 64, "{name} protocol-1001 state count");
    for (mask, record) in records.iter_mut().enumerate() {
        assert_eq!(
            record.model_state.get(ModelStateField::Connections),
            Some(mask as u32),
            "{name} protocol-1001 mask ordering"
        );
        assert_eq!(record.model_family, family, "{name} dedicated family");
        record.sequential_id = sequential_start + mask as u32;
        record.network_hash = 30_000 + sequential_start + mask as u32;
    }
    records
}

#[test]
fn compiler_compiles_glow_lichen_and_sculk_vein_as_distinct_exact_multiface_planes() {
    let directory = tempfile::tempdir().expect("create multiface fixture");
    write_pack(
        directory.path(),
        r#"{
            "glow_lichen":{"textures":"glow_lichen"},
            "sculk_vein":{"textures":"sculk_vein"}
        }"#,
        r#"{"texture_data":{
            "glow_lichen":{"textures":"textures/blocks/glow_lichen"},
            "sculk_vein":{"textures":"textures/blocks/sculk_vein"}
        }}"#,
        "[]",
    );
    let lichen_pixels = oriented_vine_pixels();
    let sculk_pixels = solid(TILE_SIZE, TILE_SIZE, [9, 91, 117, 173]);
    write_png(
        directory.path(),
        "textures/blocks/glow_lichen",
        TILE_SIZE,
        TILE_SIZE,
        &lichen_pixels,
    );
    write_png(
        directory.path(),
        "textures/blocks/sculk_vein",
        TILE_SIZE,
        TILE_SIZE,
        &sculk_pixels,
    );

    let mut records =
        generated_multiface_records("minecraft:glow_lichen", ModelFamily::GlowLichen, 0);
    records.extend(generated_multiface_records(
        "minecraft:sculk_vein",
        ModelFamily::SculkVein,
        64,
    ));
    let compiled = compile_pack(directory.path(), &records)
        .expect("compile both exact protocol-1001 multiface families");

    assert_eq!(compiled.visuals.len(), 128);
    assert_eq!(
        compiled.model_templates.len(),
        126,
        "mask 0 aliases mask 63 per family"
    );
    assert_eq!(
        compiled.model_quads.len(),
        384,
        "six bits occur in 32 nonzero masks per family"
    );
    let glow_lichen_planes = [
        (
            1_u32,
            1_u32,
            [[0, 1, 0], [0, 1, 256], [256, 1, 256], [256, 1, 0]],
        ),
        (
            2_u32,
            2_u32,
            [[0, 255, 0], [256, 255, 0], [256, 255, 256], [0, 255, 256]],
        ),
        (
            4_u32,
            6_u32,
            [[0, 0, 255], [256, 0, 255], [256, 256, 255], [0, 256, 255]],
        ),
        (
            8_u32,
            3_u32,
            [[1, 0, 0], [1, 0, 256], [1, 256, 256], [1, 256, 0]],
        ),
        (
            16_u32,
            5_u32,
            [[0, 0, 1], [0, 256, 1], [256, 256, 1], [256, 0, 1]],
        ),
        (
            32_u32,
            4_u32,
            [[255, 0, 0], [255, 256, 0], [255, 256, 256], [255, 0, 256]],
        ),
    ];
    let sculk_vein_planes = [
        glow_lichen_planes[0],
        glow_lichen_planes[1],
        (4, 5, glow_lichen_planes[4].2),
        (8, 6, glow_lichen_planes[2].2),
        (16, 3, glow_lichen_planes[3].2),
        (32, 4, glow_lichen_planes[5].2),
    ];

    for (family_start, expected_pixels, expected_planes) in [
        (0_usize, &lichen_pixels, glow_lichen_planes),
        (64, &sculk_pixels, sculk_vein_planes),
    ] {
        assert_eq!(
            compiled.visuals[family_start].model_template,
            compiled.visuals[family_start + 63].model_template,
            "mask 0 must use Bedrock's all-face fallback"
        );
        for mask in 0..64_usize {
            let effective = if mask == 0 { 63 } else { mask as u32 };
            let visual = compiled.visuals[family_start + mask];
            assert_eq!(
                visual.kind,
                VisualKind::Model,
                "family {family_start} mask {mask}"
            );
            assert!(!visual.flags.intersects(
                BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
            ));
            let template = compiled.model_templates[visual.model_template as usize];
            assert_eq!(template.quad_count, effective.count_ones(), "mask {mask}");
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            let expected = expected_planes
                .iter()
                .filter(|(bit, _, _)| effective & bit != 0)
                .collect::<Vec<_>>();
            for (quad, (bit, face, positions)) in quads.iter().zip(expected) {
                assert_eq!(quad.positions, *positions, "mask {mask} bit {bit}");
                let expected_uvs = positions.map(|[x, y, z]| {
                    if matches!(face, 1 | 2) {
                        [(x as u16) * 16, (z as u16) * 16]
                    } else {
                        let tangent = if matches!(face, 5 | 6) { x } else { z };
                        [(tangent as u16) * 16, ((256 - y) as u16) * 16]
                    }
                });
                assert_eq!(quad.uvs, expected_uvs, "mask {mask} bit {bit}");
                assert_eq!(
                    quad.flags,
                    MODEL_QUAD_FLAG_TWO_SIDED | face,
                    "mask {mask} bit {bit}"
                );
                assert_eq!(
                    quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK,
                    0,
                    "mask {mask} bit {bit}"
                );
                let material = compiled.materials[quad.material as usize];
                assert_eq!(
                    material.flags, MATERIAL_FLAG_ALPHA_CUTOUT,
                    "mask {mask} bit {bit}"
                );
                assert_eq!(
                    mip_layer(&compiled, 0, material.texture.layer()),
                    expected_pixels
                        .iter()
                        .flat_map(|pixel| pixel.iter().copied())
                        .collect::<Vec<_>>(),
                    "family {family_start} mask {mask} bit {bit}: wrong texture"
                );
            }
        }
    }
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_multiface_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile requested pinned pack");
    for family in [ModelFamily::GlowLichen, ModelFamily::SculkVein] {
        let family_records = records
            .iter()
            .filter(|record| record.model_family == family)
            .collect::<Vec<_>>();
        assert_eq!(family_records.len(), 64, "{family:?}");
        let first_visual = compiled.visuals[family_records[0].sequential_id as usize];
        let material = compiled.materials[first_visual.faces[0] as usize];
        match family {
            ModelFamily::GlowLichen => assert_eq!(material.animation, assets::NO_ANIMATION),
            ModelFamily::SculkVein => {
                assert_ne!(material.animation, assets::NO_ANIMATION);
                let animation = compiled.animations[material.animation as usize];
                assert_eq!(animation.frame_count, 4);
                assert_eq!(animation.ticks_per_frame, 20);
            }
            _ => unreachable!(),
        }
        for record in family_records {
            let mask = record
                .model_state
                .get(ModelStateField::Connections)
                .expect("multi-face direction bits");
            let visual = compiled.visuals[record.sequential_id as usize];
            assert_eq!(visual.kind, VisualKind::Model, "{family:?} mask {mask}");
            let template = compiled.model_templates[visual.model_template as usize];
            assert_eq!(
                template.quad_count,
                if mask == 0 { 6 } else { mask.count_ones() }
            );
        }
    }
}
