use super::support::*;

fn farmland_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:farmland")
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        record
            .model_state
            .get(ModelStateField::Growth)
            .expect("farmland moisture")
    });
    assert_eq!(records.len(), 8);
    records
}

fn write_farmland_pack(root: &Path) {
    write_pack(
        root,
        r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
        r#"{"texture_data":{
            "farmland_side":{"textures":"textures/blocks/dirt"},
            "farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}
        }}"#,
        "[]",
    );
    for (name, seed) in [("dirt", 11_u8), ("farmland_wet", 71), ("farmland_dry", 131)] {
        let pixels = (0..TILE_SIZE * TILE_SIZE)
            .map(|index| {
                let x = index % TILE_SIZE;
                let y = index / TILE_SIZE;
                [
                    seed.wrapping_add(x as u8 * 3),
                    seed.wrapping_add(y as u8 * 5),
                    seed.wrapping_add((x as u8 ^ y as u8) * 7),
                    255,
                ]
            })
            .collect::<Vec<_>>();
        write_png(
            root,
            &format!("textures/blocks/{name}"),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
}

#[test]
fn compiler_emits_two_exact_farmland_templates_and_three_opaque_materials() {
    let directory = tempfile::tempdir().expect("create farmland fixture");
    write_farmland_pack(directory.path());
    let mut records = farmland_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile exact farmland family");

    assert_eq!(compiled.materials.len(), 4);
    assert_eq!(compiled.model_templates.len(), 2);
    assert_eq!(compiled.model_quads.len(), 12);
    let dry = compiled.visuals[6_122];
    let wet = compiled.visuals[6_123];
    assert_ne!(
        dry.faces[BlockFace::Up as usize],
        wet.faces[BlockFace::Up as usize]
    );
    for visual in [dry, wet] {
        assert_eq!(visual.kind, VisualKind::Model);
        assert_eq!(visual.flags, BlockFlags::empty());
        assert_eq!(visual.variant, 0);
        let side = visual.faces[BlockFace::West as usize];
        assert_eq!(
            visual.faces,
            [side, side, side, visual.faces[3], side, side]
        );
        for material in [side, visual.faces[BlockFace::Up as usize]] {
            assert_ne!(material, DIAGNOSTIC_MATERIAL);
            assert_eq!(compiled.materials[material as usize].flags, 0);
            assert_eq!(
                compiled.materials[material as usize].animation,
                assets::NO_ANIMATION
            );
        }
    }
    for (amount, record) in records.iter().enumerate() {
        assert_eq!(record.sequential_id, 6_122 + amount as u32);
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "amount {amount}");
        assert_eq!(
            visual.model_template,
            if amount == 0 {
                dry.model_template
            } else {
                wet.model_template
            }
        );
        assert_eq!(
            visual.faces[3],
            if amount == 0 {
                dry.faces[3]
            } else {
                wet.faces[3]
            }
        );
        let quads = template_quads(&compiled, visual.model_template);
        assert_eq!(quads.len(), 6);
        let expected_positions = [
            [[0, 0, 0], [0, 0, 256], [0, 240, 256], [0, 240, 0]],
            [[256, 0, 0], [256, 240, 0], [256, 240, 256], [256, 0, 256]],
            [[0, 0, 0], [256, 0, 0], [256, 0, 256], [0, 0, 256]],
            [[0, 240, 0], [0, 240, 256], [256, 240, 256], [256, 240, 0]],
            [[0, 0, 0], [0, 240, 0], [256, 240, 0], [256, 0, 0]],
            [[0, 0, 256], [256, 0, 256], [256, 240, 256], [0, 240, 256]],
        ];
        let expected_uvs = [
            [[0, 4096], [4096, 4096], [4096, 256], [0, 256]],
            [[0, 4096], [0, 256], [4096, 256], [4096, 4096]],
            [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
            [[0, 0], [0, 4096], [4096, 4096], [4096, 0]],
            [[0, 4096], [0, 256], [4096, 256], [4096, 4096]],
            [[0, 4096], [4096, 4096], [4096, 256], [0, 256]],
        ];
        for (index, quad) in quads.iter().enumerate() {
            assert_eq!(
                quad.positions, expected_positions[index],
                "amount {amount} quad {index}"
            );
            assert_eq!(
                quad.uvs, expected_uvs[index],
                "amount {amount} quad {index}"
            );
            assert_eq!(quad.material, visual.faces[index]);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
        }
    }
    let baseline = encode_blob(&compiled).expect("encode farmland assets");
    records.reverse();
    let reversed = compile_pack(directory.path(), &records).expect("compile reversed farmland");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_farmland_admission_fails_closed_atomically() {
    let directory = tempfile::tempdir().expect("create farmland fixture");
    write_farmland_pack(directory.path());
    let records = farmland_records();
    let mut cases = Vec::new();
    let mut missing = records.clone();
    missing.pop();
    cases.push(("missing", missing));
    let mut wrong_type = records.clone();
    wrong_type[0].canonical_state = r#"{"moisturized_amount":{"type":"byte","value":0}}"#.into();
    cases.push(("wrong type", wrong_type));
    let mut extra = records.clone();
    extra[0].canonical_state =
        r#"{"moisturized_amount":{"type":"int","value":0},"extra":{"type":"int","value":0}}"#
            .into();
    cases.push(("extra key", extra));
    let mut wrong_projection = records.clone();
    wrong_projection[0].model_state = encoded_model_record(
        6_122,
        1,
        "minecraft:farmland",
        ModelFamily::Cuboid,
        &[(ModelStateField::Growth, 1)],
    )
    .model_state;
    cases.push(("projection", wrong_projection));
    let mut wrong_id = records.clone();
    wrong_id[0].sequential_id = 6_121;
    cases.push(("id", wrong_id));
    let mut wrong_role = records.clone();
    wrong_role[0].contributor_role = ContributorRole::LiquidAdditional;
    cases.push(("role", wrong_role));
    let mut wrong_family = records.clone();
    wrong_family[0].model_family = ModelFamily::Crop;
    cases.push(("family", wrong_family));
    let mut wrong_flags = records.clone();
    wrong_flags[0].flags = BlockFlags::CUBE_GEOMETRY;
    cases.push(("flags", wrong_flags));
    let mut wrong_coverage = records.clone();
    wrong_coverage[0].face_coverage = 1;
    cases.push(("coverage", wrong_coverage));
    let mut wrong_shape = records.clone();
    wrong_shape[0].collision_seed.shape_id += 1;
    cases.push(("shape", wrong_shape));
    let mut wrong_confidence = records.clone();
    wrong_confidence[0].collision_seed.confidence = CollisionConfidence::ReviewedVisibleBounds;
    cases.push(("confidence", wrong_confidence));
    let mut wrong_bounds = records.clone();
    wrong_bounds[0].collision_seed.boxes[0].max_y += 1;
    cases.push(("bounds", wrong_bounds));
    let mut duplicate = records.clone();
    duplicate[7].canonical_state = duplicate[6].canonical_state.clone();
    duplicate[7].model_state = duplicate[6].model_state;
    cases.push(("duplicate", duplicate));
    for (label, family) in cases {
        let compiled =
            compile_pack(directory.path(), &family).expect("compile rejected farmland family");
        assert!(
            family.iter().all(
                |record| compiled.visuals[record.sequential_id as usize].kind
                    == VisualKind::Diagnostic
            ),
            "invalid farmland family `{label}` was admitted"
        );
    }
}

#[test]
fn compiler_farmland_rejects_nonexact_sources() {
    let records = farmland_records();
    for (label, blocks, terrain, flipbooks) in [
        (
            "scalar",
            r#"{"farmland":{"textures":"farmland_side"}}"#,
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}}}"#,
            "[]",
        ),
        (
            "wrong order",
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_dry","textures/blocks/farmland_wet"]}}}"#,
            "[]",
        ),
        (
            "tinted",
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            r##"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":[{"path":"textures/blocks/farmland_wet","overlay_color":"#ffffff"},"textures/blocks/farmland_dry"]}}}"##,
            "[]",
        ),
        (
            "flipbook",
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}}}"#,
            r#"[{"flipbook_texture":"textures/blocks/farmland_wet","atlas_tile":"farmland"}]"#,
        ),
        (
            "top carried metadata",
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"],"carried_textures":"textures/blocks/farmland_dry"}}}"#,
            "[]",
        ),
        (
            "top variant alias metadata",
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":[{"path":"textures/blocks/farmland_wet","alias":"wet"},"textures/blocks/farmland_dry"]}}}"#,
            "[]",
        ),
        (
            "side carried metadata",
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt","carried_textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}}}"#,
            "[]",
        ),
    ] {
        let directory = tempfile::tempdir().expect("create malformed farmland fixture");
        write_pack(directory.path(), blocks, terrain, flipbooks);
        for (name, seed) in [("dirt", 11), ("farmland_wet", 71), ("farmland_dry", 131)] {
            write_png(
                directory.path(),
                &format!("textures/blocks/{name}"),
                TILE_SIZE,
                TILE_SIZE,
                &vec![[seed, 0, 0, 255]; (TILE_SIZE * TILE_SIZE) as usize],
            );
        }
        let compiled =
            compile_pack(directory.path(), &records).expect("compile malformed farmland pack");
        assert!(
            records.iter().all(
                |record| compiled.visuals[record.sequential_id as usize].kind
                    == VisualKind::Diagnostic
            ),
            "nonexact farmland source `{label}` was admitted"
        );
    }

    let nonopaque = tempfile::tempdir().expect("create nonopaque farmland fixture");
    write_farmland_pack(nonopaque.path());
    let mut pixels = vec![[11, 17, 23, 255]; (TILE_SIZE * TILE_SIZE) as usize];
    pixels[0][3] = 254;
    write_png(
        nonopaque.path(),
        "textures/blocks/farmland_wet",
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
    );
    let compiled =
        compile_pack(nonopaque.path(), &records).expect("compile nonopaque farmland source");
    assert!(records.iter().all(|record| {
        compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
    }));

    let bypass = tempfile::tempdir().expect("create farmland descriptor bypass");
    write_pack(
        bypass.path(),
        r#"{
            "farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland","north":"farmland_side"}},
            "farmland_descriptor_bypass":{"textures":"farmland_side"}
        }"#,
        r#"{"texture_data":{
            "farmland_side":{"textures":"textures/blocks/dirt"},
            "farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}
        }}"#,
        "[]",
    );
    for (name, seed) in [("dirt", 11), ("farmland_wet", 71), ("farmland_dry", 131)] {
        write_png(
            bypass.path(),
            &format!("textures/blocks/{name}"),
            TILE_SIZE,
            TILE_SIZE,
            &vec![[seed, 0, 0, 255]; (TILE_SIZE * TILE_SIZE) as usize],
        );
    }
    let mut with_bypass = records.clone();
    with_bypass.push(model_record(
        20_000,
        200_000,
        "minecraft:farmland_descriptor_bypass",
        "{}",
        ModelFamily::Cross,
    ));
    let compiled = compile_pack(bypass.path(), &with_bypass).expect("compile descriptor bypass");
    assert_eq!(compiled.visuals[20_000].kind, VisualKind::Cross);
    assert!(records.iter().all(|record| {
        compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
    }));
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at the ignored pinned vanilla resource pack"]
fn compiler_real_pinned_pack_admits_exact_farmland_twice() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK").expect("set PINNED_VANILLA_PACK");
    let records = farmland_records();
    let first = compile_pack(Path::new(&pack), &records).expect("compile pinned farmland");
    let second = compile_pack(Path::new(&pack), &records).expect("compile pinned farmland twice");
    assert_eq!(encode_blob(&first).unwrap(), encode_blob(&second).unwrap());
    assert!(
        records
            .iter()
            .all(|record| first.visuals[record.sequential_id as usize].kind == VisualKind::Model)
    );
}
