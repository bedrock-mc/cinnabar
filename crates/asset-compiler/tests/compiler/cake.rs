use super::support::*;

fn cake_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:cake")
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        record
            .model_state
            .get(ModelStateField::Growth)
            .expect("cake bite counter")
    });
    assert_eq!(records.len(), 7);
    records
}

fn cake_pixels(seed: u8, top_or_bottom: bool) -> Vec<[u8; 4]> {
    (0..TILE_SIZE)
        .flat_map(|y| {
            (0..TILE_SIZE).map(move |x| {
                let visible = if top_or_bottom {
                    (1..=14).contains(&x) && (1..=14).contains(&y)
                } else {
                    (1..=14).contains(&x) && y >= 8
                };
                [
                    seed.wrapping_add(x as u8 * 3),
                    seed.wrapping_add(y as u8 * 5),
                    seed.wrapping_add((x as u8 ^ y as u8) * 7),
                    if visible { 255 } else { 0 },
                ]
            })
        })
        .collect()
}

fn write_cake_pack(root: &Path) {
    write_pack(
        root,
        r#"{"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}"#,
        r#"{"texture_data":{
            "cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},
            "cake_side":{"textures":["textures/blocks/cake_side","textures/blocks/cake_side"]},
            "cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},
            "cake_west":{"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]}
        }}"#,
        "[]",
    );
    for (name, seed, top_or_bottom) in [
        ("cake_bottom", 11, true),
        ("cake_side", 71, false),
        ("cake_top", 131, true),
        ("cake_inner", 191, false),
    ] {
        write_png(
            root,
            &format!("textures/blocks/{name}"),
            TILE_SIZE,
            TILE_SIZE,
            &cake_pixels(seed, top_or_bottom),
        );
    }
}

#[test]
fn compiler_emits_seven_exact_cake_templates_and_four_cutout_materials() {
    let directory = tempfile::tempdir().expect("create cake fixture");
    write_cake_pack(directory.path());
    let mut records = cake_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile exact cake family");

    assert_eq!(
        compiled.materials.len(),
        5,
        "diagnostic plus four cake materials"
    );
    assert_eq!(compiled.model_templates.len(), 7);
    assert_eq!(compiled.model_quads.len(), 42);

    let first = compiled.visuals[14_055];
    let [side_west, side_east, bottom, top, side_north, side_south] = first.faces;
    assert_eq!(side_west, side_east);
    assert_eq!(side_west, side_north);
    assert_eq!(side_west, side_south);
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

    let bitten_west = compiled.visuals[14_056].faces[BlockFace::West as usize];
    assert_ne!(bitten_west, side_west);
    assert_eq!(
        compiled.materials[bitten_west as usize].flags,
        MATERIAL_FLAG_ALPHA_CUTOUT
    );
    assert_eq!(
        compiled.materials[bitten_west as usize].animation,
        assets::NO_ANIMATION
    );

    for (bite, record) in records.iter().enumerate() {
        assert_eq!(record.sequential_id, 14_055 + bite as u32);
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "bite {bite}");
        assert_eq!(visual.flags, BlockFlags::empty(), "bite {bite}");
        assert_eq!(visual.variant, 0, "bite {bite}");
        assert_eq!(
            visual.faces,
            [
                if bite == 0 { side_west } else { bitten_west },
                side_east,
                bottom,
                top,
                side_north,
                side_south
            ]
        );

        let min_x = 16 + 32 * bite as i16;
        let quads = template_quads(&compiled, visual.model_template);
        assert_eq!(quads.len(), 6);
        let expected_positions = [
            [
                [min_x, 0, 16],
                [min_x, 0, 240],
                [min_x, 128, 240],
                [min_x, 128, 16],
            ],
            [[240, 0, 16], [240, 128, 16], [240, 128, 240], [240, 0, 240]],
            [[min_x, 0, 16], [240, 0, 16], [240, 0, 240], [min_x, 0, 240]],
            [
                [min_x, 128, 16],
                [min_x, 128, 240],
                [240, 128, 240],
                [240, 128, 16],
            ],
            [
                [min_x, 0, 16],
                [min_x, 128, 16],
                [240, 128, 16],
                [240, 0, 16],
            ],
            [
                [min_x, 0, 240],
                [240, 0, 240],
                [240, 128, 240],
                [min_x, 128, 240],
            ],
        ];
        let expected_uvs = [
            [[256, 4096], [3840, 4096], [3840, 2048], [256, 2048]],
            [[256, 4096], [256, 2048], [3840, 2048], [3840, 4096]],
            [
                [min_x as u16 * 16, 256],
                [3840, 256],
                [3840, 3840],
                [min_x as u16 * 16, 3840],
            ],
            [
                [min_x as u16 * 16, 256],
                [min_x as u16 * 16, 3840],
                [3840, 3840],
                [3840, 256],
            ],
            [
                [min_x as u16 * 16, 4096],
                [min_x as u16 * 16, 2048],
                [3840, 2048],
                [3840, 4096],
            ],
            [
                [min_x as u16 * 16, 4096],
                [3840, 4096],
                [3840, 2048],
                [min_x as u16 * 16, 2048],
            ],
        ];
        for (index, quad) in quads.iter().enumerate() {
            assert_eq!(
                quad.positions, expected_positions[index],
                "bite {bite} quad {index}"
            );
            assert_eq!(quad.uvs, expected_uvs[index], "bite {bite} quad {index}");
            assert_eq!(
                quad.material, visual.faces[index],
                "bite {bite} quad {index}"
            );
            assert_eq!(
                quad.flags & MODEL_QUAD_FLAG_FACE_MASK,
                [3, 4, 1, 2, 5, 6][index]
            );
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
        }
    }

    let baseline = encode_blob(&compiled).expect("encode cake assets");
    records.reverse();
    let reversed = compile_pack(directory.path(), &records).expect("compile reversed cake family");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_cake_admission_fails_closed_atomically() {
    let directory = tempfile::tempdir().expect("create cake fixture");
    write_cake_pack(directory.path());
    let records = cake_records();
    let mut families = Vec::new();
    let mut missing = records.clone();
    missing.pop();
    families.push(("missing state", missing));
    let mut wrong_type = records.clone();
    wrong_type[0].canonical_state = r#"{"bite_counter":{"type":"byte","value":0}}"#.into();
    families.push(("wrong canonical type", wrong_type));
    let mut extra = records.clone();
    extra[0].canonical_state =
        r#"{"bite_counter":{"type":"int","value":0},"extra":{"type":"int","value":0}}"#.into();
    families.push(("extra canonical key", extra));
    let mut duplicate_outer = records.clone();
    duplicate_outer[0].canonical_state =
        r#"{"bite_counter":{"type":"int","value":0},"bite_counter":{"type":"int","value":0}}"#
            .into();
    families.push(("duplicate outer key", duplicate_outer));
    let mut duplicate_wrapper = records.clone();
    duplicate_wrapper[0].canonical_state =
        r#"{"bite_counter":{"type":"int","type":"int","value":0}}"#.into();
    families.push(("duplicate wrapper key", duplicate_wrapper));
    let mut wrong_projection = records.clone();
    wrong_projection[0].model_state = encoded_model_record(
        14_055,
        140_000,
        "minecraft:cake",
        ModelFamily::Cuboid,
        &[(ModelStateField::Growth, 1)],
    )
    .model_state;
    families.push(("model-state disagreement", wrong_projection));
    let mut extra_projection = records.clone();
    extra_projection[0].model_state = encoded_model_record(
        14_055,
        140_000,
        "minecraft:cake",
        ModelFamily::Cuboid,
        &[
            (ModelStateField::Growth, 0),
            (ModelStateField::Orientation, 0),
        ],
    )
    .model_state;
    families.push(("extra model-state field", extra_projection));
    let mut wrong_id = records.clone();
    wrong_id[0].sequential_id = 14_054;
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
    wrong_shape[0].collision_seed.shape_id += 1;
    families.push(("wrong shape", wrong_shape));
    let mut wrong_confidence = records.clone();
    wrong_confidence[0].collision_seed.confidence = CollisionConfidence::ReviewedVisibleBounds;
    families.push(("wrong confidence", wrong_confidence));
    let mut wrong_bounds = records.clone();
    wrong_bounds[0].collision_seed.boxes[0].min_x += 1;
    families.push(("wrong bounds", wrong_bounds));
    let mut duplicate = records.clone();
    duplicate[6].canonical_state = duplicate[5].canonical_state.clone();
    duplicate[6].model_state = duplicate[5].model_state;
    families.push(("duplicate bite", duplicate));

    for (label, family) in families {
        let compiled =
            compile_pack(directory.path(), &family).expect("compile rejected cake family");
        assert!(
            family.iter().all(
                |record| compiled.visuals[record.sequential_id as usize].kind
                    == VisualKind::Diagnostic
            ),
            "invalid family `{label}` leaked a supported visual"
        );
    }
}

#[test]
fn compiler_cake_rejects_nonexact_sources_and_descriptor_aliases() {
    let records = cake_records();
    let cases = [
        (
            "scalar face map",
            r#"{"cake":{"textures":"cake_side"}}"#,
            r#"{"texture_data":{"cake_side":{"textures":["textures/blocks/cake_side","textures/blocks/cake_side"]},"cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},"cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},"cake_west":{"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]}}}"#,
            "[]",
        ),
        (
            "aliased west key",
            r#"{"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"west_alias"}}}"#,
            r#"{"texture_data":{"cake_side":{"textures":["textures/blocks/cake_side","textures/blocks/cake_side"]},"cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},"cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},"west_alias":{"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]}}}"#,
            "[]",
        ),
        (
            "wrong west order",
            r#"{"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}"#,
            r#"{"texture_data":{"cake_side":{"textures":["textures/blocks/cake_side","textures/blocks/cake_side"]},"cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},"cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},"cake_west":{"textures":["textures/blocks/cake_inner","textures/blocks/cake_side"]}}}"#,
            "[]",
        ),
        (
            "tinted source",
            r#"{"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}"#,
            r##"{"texture_data":{"cake_side":{"textures":[{"path":"textures/blocks/cake_side","overlay_color":"#ffffff"},"textures/blocks/cake_side"]},"cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},"cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},"cake_west":{"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]}}}"##,
            "[]",
        ),
        (
            "flipbook source",
            r#"{"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}"#,
            r#"{"texture_data":{"cake_side":{"textures":["textures/blocks/cake_side","textures/blocks/cake_side"]},"cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},"cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},"cake_west":{"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]}}}"#,
            r#"[{"flipbook_texture":"textures/blocks/cake_inner","atlas_tile":"cake_west"}]"#,
        ),
    ];
    for (label, blocks, terrain, flipbooks) in cases {
        let directory = tempfile::tempdir().expect("create malformed cake pack");
        write_pack(directory.path(), blocks, terrain, flipbooks);
        for (name, seed, top_or_bottom) in [
            ("cake_bottom", 11, true),
            ("cake_side", 71, false),
            ("cake_top", 131, true),
            ("cake_inner", 191, false),
        ] {
            write_png(
                directory.path(),
                &format!("textures/blocks/{name}"),
                TILE_SIZE,
                TILE_SIZE,
                &cake_pixels(seed, top_or_bottom),
            );
        }
        let compiled =
            compile_pack(directory.path(), &records).expect("compile malformed cake pack");
        assert!(
            records.iter().all(
                |record| compiled.visuals[record.sequential_id as usize].kind
                    == VisualKind::Diagnostic
            ),
            "nonexact source `{label}` was accepted"
        );
    }

    for (label, mutation) in [
        ("fully opaque", 0_u8),
        ("nonbinary alpha", 1),
        ("wrong mask", 2),
    ] {
        let directory = tempfile::tempdir().expect("create malformed cake image pack");
        write_cake_pack(directory.path());
        let mut pixels = cake_pixels(71, false);
        match mutation {
            0 => pixels.iter_mut().for_each(|pixel| pixel[3] = 255),
            1 => pixels[8 * TILE_SIZE as usize + 1][3] = 127,
            2 => {
                pixels[8 * TILE_SIZE as usize + 1][3] = 0;
                pixels[0][3] = 255;
            }
            _ => unreachable!(),
        }
        write_png(
            directory.path(),
            "textures/blocks/cake_side",
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
        let compiled =
            compile_pack(directory.path(), &records).expect("compile malformed cake image");
        assert!(
            records.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "nonexact alpha source `{label}` was accepted"
        );
    }
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at the ignored pinned vanilla resource pack"]
fn compiler_real_pinned_pack_admits_all_exact_cake_records_twice() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK").expect("set PINNED_VANILLA_PACK");
    let records = cake_records();
    let first = compile_pack(Path::new(&pack), &records).expect("compile pinned cake family");
    let second =
        compile_pack(Path::new(&pack), &records).expect("compile pinned cake family twice");
    assert_eq!(encode_blob(&first).unwrap(), encode_blob(&second).unwrap());
    assert!(
        records
            .iter()
            .all(|record| first.visuals[record.sequential_id as usize].kind == VisualKind::Model)
    );
    assert_eq!(first.model_templates.len(), 7);
    assert_eq!(first.model_quads.len(), 42);
}
