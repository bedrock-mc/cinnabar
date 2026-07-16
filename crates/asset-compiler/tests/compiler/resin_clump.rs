use super::support::*;

fn resin_clump_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:resin_clump")
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        record
            .model_state
            .get(ModelStateField::Connections)
            .expect("resin direction mask")
    });
    assert_eq!(records.len(), 64);
    records
}

fn resin_clump_pixels() -> Vec<[u8; 4]> {
    (0..TILE_SIZE)
        .flat_map(|y| {
            (0..TILE_SIZE).map(move |x| {
                [
                    11 + x as u8 * 7,
                    17 + y as u8 * 5,
                    23 + (x as u8 ^ y as u8) * 3,
                    if (x + y * 3) % 5 == 0 { 255 } else { 0 },
                ]
            })
        })
        .collect()
}

fn write_resin_clump_pack(root: &Path) -> Vec<[u8; 4]> {
    write_pack(
        root,
        r#"{"resin_clump":{"carried_textures":"resin_clump_carried","textures":"resin_clump"}}"#,
        r#"{"texture_data":{"resin_clump":{"textures":"textures/blocks/resin_clump"}}}"#,
        "[]",
    );
    let pixels = resin_clump_pixels();
    write_png(
        root,
        "textures/blocks/resin_clump",
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
    );
    pixels
}

#[test]
fn compiler_emits_exact_resin_clump_material_planes_and_zero_alias() {
    let directory = tempfile::tempdir().expect("create resin-clump fixture");
    let expected_pixels = write_resin_clump_pack(directory.path());
    let mut records = resin_clump_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile exact resin clumps");

    assert_eq!(
        compiled.materials.len(),
        2,
        "diagnostic plus one resin material"
    );
    assert_eq!(
        compiled.model_templates.len(),
        63,
        "mask zero aliases mask 63"
    );
    assert_eq!(
        compiled.model_quads.len(),
        192,
        "six bits occur in 32 masks"
    );
    let expected_planes = [
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
    let material = compiled.visuals[2930].faces[0];
    assert_ne!(material, DIAGNOSTIC_MATERIAL);
    assert_eq!(
        compiled.materials[material as usize].flags,
        MATERIAL_FLAG_ALPHA_CUTOUT
    );
    assert_eq!(
        compiled.materials[material as usize].animation,
        assets::NO_ANIMATION
    );
    assert_eq!(
        mip_layer(
            &compiled,
            0,
            compiled.materials[material as usize].texture.layer()
        ),
        expected_pixels
            .iter()
            .flat_map(|pixel| pixel.iter().copied())
            .collect::<Vec<_>>()
    );

    assert_eq!(
        compiled.visuals[2930].model_template, compiled.visuals[2993].model_template,
        "mask zero must alias native-normalized mask 63"
    );
    for (mask, record) in records.iter().enumerate() {
        assert_eq!(record.sequential_id, 2930 + mask as u32);
        let effective = if mask == 0 { 63 } else { mask as u32 };
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "mask {mask}");
        assert_eq!(visual.flags, BlockFlags::empty(), "mask {mask}");
        assert_eq!(visual.faces, [material; 6], "mask {mask}");
        assert_eq!(visual.variant, 0, "mask {mask}");
        let quads = template_quads(&compiled, visual.model_template);
        assert_eq!(quads.len(), effective.count_ones() as usize, "mask {mask}");
        let expected = expected_planes
            .iter()
            .filter(|(bit, _, _)| effective & bit != 0);
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
            assert_eq!(quad.material, material, "mask {mask} bit {bit}");
            assert_eq!(
                quad.flags,
                MODEL_QUAD_FLAG_TWO_SIDED | face,
                "mask {mask} bit {bit}"
            );
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
        }
    }

    let baseline = encode_blob(&compiled).expect("encode exact resin assets");
    records.reverse();
    let reversed =
        compile_pack(directory.path(), &records).expect("compile reversed resin records");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_resin_clump_admission_fails_closed_as_a_complete_family() {
    let directory = tempfile::tempdir().expect("create resin-clump fixture");
    write_resin_clump_pack(directory.path());
    let records = resin_clump_records();

    let mut families = Vec::new();
    let mut missing = records.clone();
    missing.pop();
    families.push(("missing state", missing));
    let mut wrong_id = records.clone();
    wrong_id[0].sequential_id = 2929;
    families.push(("wrong ID", wrong_id));
    let mut wrong_state = records.clone();
    wrong_state[0].canonical_state =
        r#"{"multi_face_direction_bits":{"type":"byte","value":0}}"#.into();
    families.push(("wrong canonical type", wrong_state));
    let mut extra_state = records.clone();
    extra_state[0].canonical_state = r#"{"extra":{"type":"int","value":0},"multi_face_direction_bits":{"type":"int","value":0}}"#.into();
    families.push(("extra canonical key", extra_state));
    let mut wrong_projection = records.clone();
    wrong_projection[0].model_state = encoded_model_record(
        2930,
        80_000,
        "minecraft:resin_clump",
        ModelFamily::ResinClump,
        &[(ModelStateField::Connections, 1)],
    )
    .model_state;
    families.push(("model-state disagreement", wrong_projection));
    let mut extra_projection = records.clone();
    extra_projection[0].model_state = encoded_model_record(
        2930,
        80_000,
        "minecraft:resin_clump",
        ModelFamily::ResinClump,
        &[
            (ModelStateField::Connections, 0),
            (ModelStateField::Orientation, 0),
        ],
    )
    .model_state;
    families.push(("extra model-state field", extra_projection));
    let mut wrong_role = records.clone();
    wrong_role[0].contributor_role = ContributorRole::LiquidAdditional;
    families.push(("wrong role", wrong_role));
    let mut wrong_family = records.clone();
    wrong_family[0].model_family = ModelFamily::GlowLichen;
    families.push(("wrong family", wrong_family));
    let mut wrong_name = records.clone();
    wrong_name[0].name = "minecraft:resin_clumps".into();
    families.push(("wrong name", wrong_name));
    let mut wrong_flags = records.clone();
    wrong_flags[0].flags = BlockFlags::CUBE_GEOMETRY;
    families.push(("wrong flags", wrong_flags));
    let mut wrong_coverage = records.clone();
    wrong_coverage[0].face_coverage = 1;
    families.push(("wrong face coverage", wrong_coverage));
    let mut wrong_shape = records.clone();
    wrong_shape[0].collision_seed.shape_id = 1;
    families.push(("wrong collision shape", wrong_shape));
    let mut wrong_confidence = records.clone();
    wrong_confidence[0].collision_seed.confidence = CollisionConfidence::ReviewedVisibleBounds;
    families.push(("wrong collision confidence", wrong_confidence));
    let mut wrong_boxes = records.clone();
    wrong_boxes[0].collision_seed.boxes = vec![CollisionBox::default()].into_boxed_slice();
    families.push(("unexpected collision box", wrong_boxes));
    let mut duplicate = records.clone();
    duplicate[63].canonical_state = duplicate[62].canonical_state.clone();
    duplicate[63].model_state = duplicate[62].model_state;
    families.push(("duplicate selector", duplicate));

    for (label, family) in families {
        let compiled =
            compile_pack(directory.path(), &family).expect("compile rejected resin family");
        assert!(
            family.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "invalid family `{label}` leaked a supported visual"
        );
    }
}

#[test]
fn compiler_resin_clump_rejects_nonexact_pack_routes_and_animation() {
    let records = resin_clump_records();
    let cases = [
        (
            "scalar alias",
            r#"{"resin_clump":{"textures":"resin_alias"}}"#,
            r#"{"texture_data":{"resin_alias":{"textures":"textures/blocks/resin_clump"}}}"#,
            "[]",
        ),
        (
            "face map",
            r#"{"resin_clump":{"textures":{"down":"resin_clump","up":"resin_clump","north":"resin_clump","east":"resin_clump","south":"resin_clump","west":"resin_clump"}}}"#,
            r#"{"texture_data":{"resin_clump":{"textures":"textures/blocks/resin_clump"}}}"#,
            "[]",
        ),
        (
            "one-element array",
            r#"{"resin_clump":{"textures":"resin_clump"}}"#,
            r#"{"texture_data":{"resin_clump":{"textures":["textures/blocks/resin_clump"]}}}"#,
            "[]",
        ),
        (
            "overlong array",
            r#"{"resin_clump":{"textures":"resin_clump"}}"#,
            r#"{"texture_data":{"resin_clump":{"textures":["textures/blocks/resin_clump","textures/blocks/resin_clump"]}}}"#,
            "[]",
        ),
        (
            "overlay tint",
            r#"{"resin_clump":{"textures":"resin_clump"}}"#,
            r##"{"texture_data":{"resin_clump":{"textures":{"path":"textures/blocks/resin_clump","overlay_color":"#ffffff"}}}}"##,
            "[]",
        ),
        (
            "flipbook",
            r#"{"resin_clump":{"textures":"resin_clump"}}"#,
            r#"{"texture_data":{"resin_clump":{"textures":"textures/blocks/resin_clump"}}}"#,
            r#"[{"flipbook_texture":"textures/blocks/resin_clump","atlas_tile":"resin_clump"}]"#,
        ),
    ];
    for (label, blocks, terrain, flipbooks) in cases {
        let malformed = tempfile::tempdir().expect("create malformed resin pack");
        write_pack(malformed.path(), blocks, terrain, flipbooks);
        write_png(
            malformed.path(),
            "textures/blocks/resin_clump",
            TILE_SIZE,
            TILE_SIZE,
            &resin_clump_pixels(),
        );
        let compiled =
            compile_pack(malformed.path(), &records).expect("compile malformed resin pack");
        assert!(
            records.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "nonexact route `{label}` was accepted"
        );
    }

    let carried_only = tempfile::tempdir().expect("create carried-only resin pack");
    write_pack(
        carried_only.path(),
        r#"{"resin_clump":{"carried_textures":"resin_clump"}}"#,
        r#"{"texture_data":{"resin_clump":{"textures":"textures/blocks/resin_clump"}}}"#,
        "[]",
    );
    write_png(
        carried_only.path(),
        "textures/blocks/resin_clump",
        TILE_SIZE,
        TILE_SIZE,
        &resin_clump_pixels(),
    );
    let compiled = compile_pack(carried_only.path(), &records).expect("compile carried-only pack");
    assert!(records.iter().all(|record| {
        compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
    }));
}

#[test]
fn compiler_resin_clump_rejects_a_nonexact_route_even_when_its_descriptor_is_interned() {
    let directory = tempfile::tempdir().expect("create resin descriptor-bypass fixture");
    write_pack(
        directory.path(),
        r#"{
            "resin_clump":{"textures":{"down":"resin_clump","up":"resin_clump","north":"resin_clump","east":"resin_clump","south":"resin_clump","west":"resin_clump"}},
            "resin_descriptor_bypass":{"textures":"resin_clump"}
        }"#,
        r#"{"texture_data":{"resin_clump":{"textures":"textures/blocks/resin_clump"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/resin_clump",
        TILE_SIZE,
        TILE_SIZE,
        &resin_clump_pixels(),
    );
    let mut records = resin_clump_records();
    records.push(model_record(
        4_000,
        94_000,
        "minecraft:resin_descriptor_bypass",
        "{}",
        ModelFamily::Cross,
    ));

    let compiled = compile_pack(directory.path(), &records).expect("compile descriptor bypass");
    assert_eq!(compiled.visuals[4_000].kind, VisualKind::Cross);
    assert!(records[..64].iter().all(|record| {
        compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
    }));
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at the ignored pinned vanilla resource pack"]
fn compiler_real_pinned_pack_admits_all_exact_resin_clump_records() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .expect("set PINNED_VANILLA_PACK to the ignored pinned vanilla resource pack");
    let mut records = resin_clump_records();
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned resin clumps");

    assert_eq!(records.len(), 64);
    assert_eq!(compiled.materials.len(), 2, "diagnostic plus resin");
    assert_eq!(compiled.model_templates.len(), 63);
    assert_eq!(compiled.model_quads.len(), 192);
    let family_materials = records
        .iter()
        .flat_map(|record| compiled.visuals[record.sequential_id as usize].faces)
        .collect::<HashSet<_>>();
    assert_eq!(family_materials.len(), 1, "one exact resin material");
    let material = *family_materials.iter().next().expect("resin material");
    assert_ne!(material, DIAGNOSTIC_MATERIAL);
    assert_eq!(
        compiled.materials[material as usize].flags,
        MATERIAL_FLAG_ALPHA_CUTOUT
    );
    assert_eq!(
        compiled.materials[material as usize].animation,
        assets::NO_ANIMATION
    );
    assert!(records.iter().all(|record| {
        let visual = compiled.visuals[record.sequential_id as usize];
        visual.kind == VisualKind::Model && visual.flags.is_empty() && visual.faces == [material; 6]
    }));
    assert_eq!(
        compiled.visuals[2930].model_template, compiled.visuals[2993].model_template,
        "mask zero aliases native-normalized mask 63"
    );

    let baseline = encode_blob(&compiled).expect("encode pinned resin assets");
    records.reverse();
    let reversed =
        compile_pack(Path::new(&pack), &records).expect("compile reversed pinned resin records");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}
