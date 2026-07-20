use super::support::*;

fn cactus_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:cactus")
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        record
            .model_state
            .get(ModelStateField::Growth)
            .expect("cactus age")
    });
    assert_eq!(records.len(), 16);
    records
}

fn cactus_pixels(seed: u8) -> Vec<[u8; 4]> {
    (0..TILE_SIZE)
        .flat_map(|y| {
            (0..TILE_SIZE).map(move |x| {
                [
                    seed.wrapping_add(x as u8 * 3),
                    seed.wrapping_add(y as u8 * 5),
                    seed.wrapping_add((x as u8 ^ y as u8) * 7),
                    if x == 0 || x == TILE_SIZE - 1 { 0 } else { 255 },
                ]
            })
        })
        .collect()
}

fn write_cactus_pack(root: &Path) {
    write_pack(
        root,
        r#"{"cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top"}}}"#,
        r#"{"texture_data":{
            "cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},
            "cactus_side":{"textures":"textures/blocks/cactus_side"},
            "cactus_top":{"textures":"textures/blocks/cactus_top"}
        }}"#,
        "[]",
    );
    for (name, seed) in [
        ("cactus_bottom", 11),
        ("cactus_side", 71),
        ("cactus_top", 131),
    ] {
        write_png(
            root,
            &format!("textures/blocks/{name}"),
            TILE_SIZE,
            TILE_SIZE,
            &cactus_pixels(seed),
        );
    }
}

#[test]
fn compiler_emits_one_exact_cactus_template_and_three_cutout_materials() {
    let directory = tempfile::tempdir().expect("create cactus fixture");
    write_cactus_pack(directory.path());
    let mut records = cactus_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile exact cactus family");

    assert_eq!(
        compiled.materials.len(),
        4,
        "diagnostic plus three cactus materials"
    );
    assert_eq!(compiled.model_templates.len(), 1);
    assert_eq!(compiled.model_quads.len(), 6);
    let first = compiled.visuals[13_606];
    let [side_west, side_east, bottom, top, side_north, side_south] = first.faces;
    assert_eq!(side_west, side_east);
    assert_eq!(side_west, side_north);
    assert_eq!(side_west, side_south);
    assert_ne!(side_west, bottom);
    assert_ne!(side_west, top);
    assert_ne!(bottom, top);
    for material in [side_west, bottom, top] {
        assert_ne!(material, DIAGNOSTIC_MATERIAL);
        assert_eq!(
            compiled.materials[material as usize].flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
        assert_eq!(
            compiled.materials[material as usize].animation,
            assets::NO_ANIMATION
        );
    }

    for (age, record) in records.iter().enumerate() {
        assert_eq!(record.sequential_id, 13_606 + age as u32);
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "age {age}");
        assert_eq!(visual.flags, BlockFlags::empty(), "age {age}");
        assert_eq!(visual.faces, first.faces, "age {age}");
        assert_eq!(visual.model_template, first.model_template, "age {age}");
        assert_eq!(visual.variant, 0, "age {age}");
    }

    let expected_positions = [
        [[16, 0, 16], [16, 0, 240], [16, 256, 240], [16, 256, 16]],
        [[240, 0, 16], [240, 256, 16], [240, 256, 240], [240, 0, 240]],
        [[16, 0, 16], [240, 0, 16], [240, 0, 240], [16, 0, 240]],
        [
            [16, 256, 16],
            [16, 256, 240],
            [240, 256, 240],
            [240, 256, 16],
        ],
        [[16, 0, 16], [16, 256, 16], [240, 256, 16], [240, 0, 16]],
        [[16, 0, 240], [240, 0, 240], [240, 256, 240], [16, 256, 240]],
    ];
    let expected_uvs = [
        [[256, 4096], [3840, 4096], [3840, 0], [256, 0]],
        [[256, 4096], [256, 0], [3840, 0], [3840, 4096]],
        [[256, 256], [3840, 256], [3840, 3840], [256, 3840]],
        [[256, 256], [256, 3840], [3840, 3840], [3840, 256]],
        [[256, 4096], [256, 0], [3840, 0], [3840, 4096]],
        [[256, 4096], [3840, 4096], [3840, 0], [256, 0]],
    ];
    let expected_materials = [side_west, side_west, bottom, top, side_west, side_west];
    for (index, quad) in template_quads(&compiled, first.model_template)
        .iter()
        .enumerate()
    {
        assert_eq!(quad.positions, expected_positions[index], "quad {index}");
        assert_eq!(quad.uvs, expected_uvs[index], "quad {index}");
        assert_eq!(quad.material, expected_materials[index], "quad {index}");
        assert_eq!(
            quad.flags & MODEL_QUAD_FLAG_FACE_MASK,
            [3, 4, 1, 2, 5, 6][index]
        );
        assert_eq!(
            quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK,
            0,
            "quad {index}"
        );
        assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0, "quad {index}");
    }

    let baseline = encode_blob(&compiled).expect("encode cactus assets");
    records.reverse();
    let reversed =
        compile_pack(directory.path(), &records).expect("compile reversed cactus family");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_cactus_admission_fails_closed_atomically() {
    let directory = tempfile::tempdir().expect("create cactus fixture");
    write_cactus_pack(directory.path());
    let records = cactus_records();
    let mut families = Vec::new();
    let mut missing = records.clone();
    missing.pop();
    families.push(("missing state", missing));
    let mut wrong_type = records.clone();
    wrong_type[0].canonical_state = r#"{"age":{"type":"byte","value":0}}"#.into();
    families.push(("wrong canonical type", wrong_type));
    let mut extra = records.clone();
    extra[0].canonical_state =
        r#"{"age":{"type":"int","value":0},"extra":{"type":"int","value":0}}"#.into();
    families.push(("extra canonical key", extra));
    let mut wrong_projection = records.clone();
    wrong_projection[0].model_state = encoded_model_record(
        13_606,
        130_000,
        "minecraft:cactus",
        ModelFamily::Cuboid,
        &[(ModelStateField::Growth, 1)],
    )
    .model_state;
    families.push(("model-state disagreement", wrong_projection));
    let mut extra_projection = records.clone();
    extra_projection[0].model_state = encoded_model_record(
        13_606,
        130_000,
        "minecraft:cactus",
        ModelFamily::Cuboid,
        &[
            (ModelStateField::Growth, 0),
            (ModelStateField::Orientation, 0),
        ],
    )
    .model_state;
    families.push(("extra model-state field", extra_projection));
    let mut wrong_id = records.clone();
    wrong_id[0].sequential_id = 13_605;
    families.push(("wrong ID", wrong_id));
    let mut wrong_family = records.clone();
    wrong_family[0].model_family = ModelFamily::Crop;
    families.push(("wrong family", wrong_family));
    let mut wrong_role = records.clone();
    wrong_role[0].contributor_role = ContributorRole::LiquidAdditional;
    families.push(("wrong role", wrong_role));
    let mut wrong_flags = records.clone();
    wrong_flags[0].flags = BlockFlags::CUBE_GEOMETRY;
    families.push(("wrong flags", wrong_flags));
    let mut wrong_coverage = records.clone();
    wrong_coverage[0].face_coverage = 1;
    families.push(("wrong coverage", wrong_coverage));
    let mut wrong_shape = records.clone();
    wrong_shape[0].collision_seed.shape_id = 1;
    families.push(("wrong shape", wrong_shape));
    let mut wrong_confidence = records.clone();
    wrong_confidence[0].collision_seed.confidence = CollisionConfidence::ReviewedVisibleBounds;
    families.push(("wrong confidence", wrong_confidence));
    let mut wrong_bounds = records.clone();
    wrong_bounds[0].collision_seed.boxes[0].min_x += 1;
    families.push(("wrong bounds", wrong_bounds));
    let mut duplicate = records.clone();
    duplicate[15].canonical_state = duplicate[14].canonical_state.clone();
    duplicate[15].model_state = duplicate[14].model_state;
    families.push(("duplicate age", duplicate));

    for (label, family) in families {
        let compiled =
            compile_pack(directory.path(), &family).expect("compile rejected cactus family");
        assert!(
            family.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "invalid family `{label}` leaked a supported visual"
        );
    }
}

#[test]
fn compiler_cactus_rejects_nonexact_pack_routes_even_when_materials_are_interned() {
    let records = cactus_records();
    let cases = [
        (
            "scalar",
            r#"{"cactus":{"textures":"cactus_side"}}"#,
            r#"{"texture_data":{"cactus_side":{"textures":"textures/blocks/cactus_side"},"cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},"cactus_top":{"textures":"textures/blocks/cactus_top"}}}"#,
            "[]",
        ),
        (
            "alias",
            r#"{"cactus":{"textures":{"down":"bottom_alias","side":"side_alias","up":"top_alias"}}}"#,
            r#"{"texture_data":{"bottom_alias":{"textures":"textures/blocks/cactus_bottom"},"side_alias":{"textures":"textures/blocks/cactus_side"},"top_alias":{"textures":"textures/blocks/cactus_top"}}}"#,
            "[]",
        ),
        (
            "variant",
            r#"{"cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top"}}}"#,
            r#"{"texture_data":{"cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},"cactus_side":{"textures":["textures/blocks/cactus_side"]},"cactus_top":{"textures":"textures/blocks/cactus_top"}}}"#,
            "[]",
        ),
        (
            "tint",
            r#"{"cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top"}}}"#,
            r##"{"texture_data":{"cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},"cactus_side":{"textures":{"path":"textures/blocks/cactus_side","overlay_color":"#ffffff"}},"cactus_top":{"textures":"textures/blocks/cactus_top"}}}"##,
            "[]",
        ),
        (
            "flipbook",
            r#"{"cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top"}}}"#,
            r#"{"texture_data":{"cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},"cactus_side":{"textures":"textures/blocks/cactus_side"},"cactus_top":{"textures":"textures/blocks/cactus_top"}}}"#,
            r#"[{"flipbook_texture":"textures/blocks/cactus_side","atlas_tile":"cactus_side"}]"#,
        ),
        (
            "unknown face-map key",
            r#"{"cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top","sied":"cactus_side"}}}"#,
            r#"{"texture_data":{"cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},"cactus_side":{"textures":"textures/blocks/cactus_side"},"cactus_top":{"textures":"textures/blocks/cactus_top"}}}"#,
            "[]",
        ),
    ];
    for (label, blocks, terrain, flipbooks) in cases {
        let directory = tempfile::tempdir().expect("create malformed cactus pack");
        write_pack(directory.path(), blocks, terrain, flipbooks);
        for (name, seed) in [
            ("cactus_bottom", 11),
            ("cactus_side", 71),
            ("cactus_top", 131),
        ] {
            write_png(
                directory.path(),
                &format!("textures/blocks/{name}"),
                TILE_SIZE,
                TILE_SIZE,
                &cactus_pixels(seed),
            );
        }
        let compiled =
            compile_pack(directory.path(), &records).expect("compile malformed cactus pack");
        assert!(
            records.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "nonexact route `{label}` was accepted"
        );
    }

    let bypass = tempfile::tempdir().expect("create cactus descriptor bypass");
    write_pack(
        bypass.path(),
        r#"{
            "cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_side","west":"cactus_side"}},
            "cactus_descriptor_bypass":{"textures":"cactus_side"}
        }"#,
        r#"{"texture_data":{"cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},"cactus_side":{"textures":"textures/blocks/cactus_side"},"cactus_top":{"textures":"textures/blocks/cactus_top"}}}"#,
        "[]",
    );
    for (name, seed) in [
        ("cactus_bottom", 11),
        ("cactus_side", 71),
        ("cactus_top", 131),
    ] {
        write_png(
            bypass.path(),
            &format!("textures/blocks/{name}"),
            TILE_SIZE,
            TILE_SIZE,
            &cactus_pixels(seed),
        );
    }
    let mut with_bypass = records.clone();
    with_bypass.push(model_record(
        20_000,
        200_000,
        "minecraft:cactus_descriptor_bypass",
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
fn compiler_real_pinned_pack_admits_all_exact_cactus_records() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .expect("set PINNED_VANILLA_PACK to the ignored pinned vanilla resource pack");
    let records = cactus_records();
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned cactus family");
    assert!(records.iter().all(|record| {
        compiled.visuals[record.sequential_id as usize].kind == VisualKind::Model
    }));
    assert_eq!(compiled.model_templates.len(), 1);
    assert_eq!(compiled.model_quads.len(), 6);
}
