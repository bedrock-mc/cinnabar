use super::support::*;

fn write_flowerbed_pack(root: &Path, include_stem: bool) {
    write_pack(
        root,
        r#"{
            "wildflowers":{"textures":"wildflowers"},
            "pink_petals":{"textures":"pink_petals"}
        }"#,
        if include_stem {
            r#"{"texture_data":{
                "wildflowers":{"textures":[
                    "textures/blocks/wildflowers",
                    "textures/blocks/wildflowers_stem"
                ]},
                "pink_petals":{"textures":[
                    "textures/blocks/pink_petals",
                    "textures/blocks/pink_petals_stem"
                ]}
            }}"#
        } else {
            r#"{"texture_data":{
                "wildflowers":{"textures":["textures/blocks/wildflowers"]},
                "pink_petals":{"textures":["textures/blocks/pink_petals"]}
            }}"#
        },
        "[]",
    );
    for (index, path) in [
        "wildflowers",
        "wildflowers_stem",
        "pink_petals",
        "pink_petals_stem",
    ]
    .into_iter()
    .enumerate()
    {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 41, 79, 0]),
        );
    }
}

fn flowerbed_geometry_digest(quads: &[assets::ModelQuad]) -> String {
    let flower_material = quads
        .first()
        .expect("flowerbed template has quads")
        .material;
    let mut digest = Sha256::new();
    for quad in quads {
        for position in quad.positions {
            for coordinate in position {
                digest.update(coordinate.to_le_bytes());
            }
        }
        for uv in quad.uvs {
            for coordinate in uv {
                digest.update(coordinate.to_le_bytes());
            }
        }
        digest.update([u8::from(quad.material != flower_material)]);
        digest.update(quad.flags.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

#[test]
fn compiler_flowerbed_positions_and_uvs_match_pinned_layout_hashes() {
    let directory = tempfile::tempdir().expect("create flowerbed digest fixture");
    write_flowerbed_pack(directory.path(), true);
    let records = (0..4)
        .flat_map(|growth| {
            (0..4).map(move |orientation| {
                let sequential_id = growth * 4 + orientation;
                generated_flowerbed_record(
                    sequential_id,
                    9_000 + sequential_id,
                    "minecraft:wildflowers",
                    growth,
                    orientation,
                )
            })
        })
        .collect::<Vec<_>>();
    let compiled = compile_pack(directory.path(), &records).expect("compile flowerbed digests");
    let actual = compiled
        .visuals
        .iter()
        .map(|visual| {
            let template = compiled.model_templates[visual.model_template as usize];
            flowerbed_geometry_digest(
                &compiled.model_quads[template.quad_start as usize
                    ..(template.quad_start + template.quad_count) as usize],
            )
        })
        .collect::<Vec<_>>();
    let expected = [
        [
            "0535cc209cf5d041dac03f4b705b506e4dcbcf78631b3c19f08c29529c0372e1",
            "fe9ae6e63ab41e8a54a1d478d493ac7f27b6a65e6342e2e9897a0b2481277c5c",
            "a7b1c4a16e06435a244f86d6f363592604ac1e3004f159df82c8126eaab60b69",
            "137445d77e0b871726f1715da34ea13afc85f8e368b3eabe47aa73c226d139ff",
        ],
        [
            "7ab55b6772d41dda2a461a3a6283e15f80d2f37624b259be8f01e806764d592d",
            "4d7a49f3e00ddb42a2e4d1457c90d97c1cdc9530315fa4f84c7b4eb375470b03",
            "02732e5274ee362636178813a3757a7ecb172b64a4bc53a04543ade5dd825984",
            "17fa180ccb7197b23a4812f4aa9bf5836ff1f216c90b543e61e10b280bd31e8c",
        ],
        [
            "5ef044de509676b39536764fbe07a8dcff229c395f4c9a1e359f252491f2c206",
            "722e9e3b0baa2de6565fdd5784e9cd88573bd8d211759492f3c285680864ea64",
            "6c69274f1235f83290629448199c67bd4d50768906815b3d191f8c928ecb85f6",
            "b2876159d61f4efcfc7050cbe3d68a381b7c4f6d3231e1dbe2ec9578680223ca",
        ],
        [
            "0ad8b575a87c6d1b1b6acb04b77cdb9c7db62321e38af3d157c2af8d84b6b134",
            "6e86adaf45e3916de0372636dcc6ebd1dc93b8c97675c466235766e8027b4950",
            "18a7cddfe2d57f62c2fdd29ed8a0edf883c13e7b9d79ce084093685c55e82574",
            "9c35fc675c95aeca270cb20b6b68eea6e1f366a6785003b3b0af3dbab92663d5",
        ],
    ];
    for layout in 0..4 {
        for orientation in 0..4 {
            assert_eq!(
                actual[layout * 4 + orientation],
                expected[layout][orientation],
                "layout={layout} orientation={orientation}"
            );
        }
    }
}

#[test]
fn compiler_compiles_normal_flowerbeds_as_additive_near_ground_two_material_models() {
    let directory = tempfile::tempdir().expect("create flowerbed fixture");
    write_flowerbed_pack(directory.path(), true);
    let mut records = Vec::new();
    for name in ["minecraft:wildflowers", "minecraft:pink_petals"] {
        for growth in 0..=7 {
            let sequential_id = records.len() as u32;
            records.push(generated_flowerbed_record(
                sequential_id,
                10_000 + sequential_id,
                name,
                growth,
                2,
            ));
        }
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile flowerbeds");
    for name_index in 0..2 {
        for (growth, expected_flower_quads) in [1, 2, 3, 4, 4, 4, 4, 4].into_iter().enumerate() {
            let visual = compiled.visuals[name_index * 8 + growth];
            assert_eq!(visual.kind, VisualKind::Model, "growth={growth}");
            assert_ne!(visual.model_template, assets::NO_MODEL_TEMPLATE);
            let template = compiled.model_templates[visual.model_template as usize];
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            let flower_material = quads[0].material;
            assert_eq!(
                quads
                    .iter()
                    .filter(|quad| quad.material == flower_material)
                    .count(),
                expected_flower_quads,
                "growth={growth} additive patch count"
            );
            assert!(
                quads
                    .iter()
                    .flat_map(|quad| quad.positions)
                    .all(|position| position[1] < 64),
                "growth={growth} exceeded near-ground bound"
            );
            assert_eq!(
                quads
                    .iter()
                    .map(|quad| quad.material)
                    .collect::<HashSet<_>>()
                    .len(),
                2,
                "growth={growth} material count"
            );
            assert!(
                quads
                    .iter()
                    .all(|quad| quad.flags == MODEL_QUAD_FLAG_TWO_SIDED)
            );
        }
    }
}

#[test]
fn compiler_rotates_north_baseline_flowerbeds_by_pinned_cardinal_authority() {
    let directory = tempfile::tempdir().expect("create flowerbed rotation fixture");
    write_flowerbed_pack(directory.path(), true);
    let records = (0..4)
        .map(|orientation| {
            generated_flowerbed_record(
                orientation,
                11_000 + orientation,
                "minecraft:wildflowers",
                0,
                orientation,
            )
        })
        .collect::<Vec<_>>();
    let compiled = compile_pack(directory.path(), &records).expect("compile rotated flowerbeds");
    // Pinned wildflowers.json at be56c809: north has no Y rotation;
    // east=90, south=180, west=270. BREG encodes S=0, W=1, N=2, E=3.
    let authority = [
        (0, "south", 180),
        (1, "west", 270),
        (2, "north", 0),
        (3, "east", 90),
    ];
    let expected_flower_positions = [
        [
            [256, 48, 256],
            [128, 48, 256],
            [128, 48, 128],
            [256, 48, 128],
        ],
        [[0, 48, 256], [0, 48, 128], [128, 48, 128], [128, 48, 256]],
        [[0, 48, 0], [128, 48, 0], [128, 48, 128], [0, 48, 128]],
        [[256, 48, 0], [256, 48, 128], [128, 48, 128], [128, 48, 0]],
    ];
    let expected_stem_positions = [
        [[179, 0, 237], [190, 0, 226], [190, 48, 226], [179, 48, 237]],
        [[19, 0, 179], [30, 0, 190], [30, 48, 190], [19, 48, 179]],
        [[77, 0, 19], [66, 0, 30], [66, 48, 30], [77, 48, 19]],
        [[237, 0, 77], [226, 0, 66], [226, 48, 66], [237, 48, 77]],
    ];
    for (orientation, direction, degrees) in authority {
        let visual = compiled.visuals[orientation];
        let template = compiled.model_templates[visual.model_template as usize];
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert_eq!(
            quads[0].positions, expected_flower_positions[orientation],
            "BREG {direction}={orientation} must apply Y={degrees}"
        );
        assert_eq!(
            quads[1].positions, expected_stem_positions[orientation],
            "BREG {direction}={orientation} stem must apply Y={degrees}"
        );
        assert_eq!(quads[0].uvs, [[0, 0], [2048, 0], [2048, 2048], [0, 2048]]);
        assert_eq!(
            quads[1].uvs,
            [[0, 1792], [256, 1792], [256, 1024], [0, 1024]]
        );
    }
}

#[test]
fn compiler_flowerbed_templates_are_bounded_deduplicated_and_blob_stable() {
    let directory = tempfile::tempdir().expect("create flowerbed matrix fixture");
    write_flowerbed_pack(directory.path(), true);
    let mut records = Vec::new();
    for name in ["minecraft:wildflowers", "minecraft:pink_petals"] {
        for growth in 0..8 {
            for orientation in 0..4 {
                let sequential_id = records.len() as u32;
                records.push(generated_flowerbed_record(
                    sequential_id,
                    12_000 + sequential_id,
                    name,
                    growth,
                    orientation,
                ));
            }
        }
    }
    let duplicate_id = records.len() as u32;
    records.push(generated_flowerbed_record(
        duplicate_id,
        12_000 + duplicate_id,
        "minecraft:wildflowers",
        2,
        2,
    ));

    let compiled = compile_pack(directory.path(), &records).expect("compile flowerbed matrix");
    assert_eq!(compiled.materials.len(), 5, "diagnostic plus four textures");
    assert_eq!(compiled.model_templates.len(), 32);
    assert_eq!(compiled.model_quads.len(), 432);
    assert_eq!(
        compiled.visuals[duplicate_id as usize].model_template, compiled.visuals[10].model_template,
        "identical material/growth/orientation identity must deduplicate"
    );
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Model),
        "all 64 normal flowerbed states must route to models"
    );
    for name_index in 0..2 {
        for orientation in 0..4 {
            let full_layout =
                compiled.visuals[name_index * 32 + 3 * 4 + orientation].model_template;
            for growth in 4..8 {
                assert_eq!(
                    compiled.visuals[name_index * 32 + growth * 4 + orientation].model_template,
                    full_layout,
                    "growth={growth} must alias the measured full layout for block={name_index} orientation={orientation}"
                );
            }
        }
    }
    for (index, expected_quads) in [7, 10, 17, 20].into_iter().enumerate() {
        let visual = compiled.visuals[index * 4];
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.quad_count, expected_quads);
        assert!(template.quad_count <= 32);
    }

    let bytes = encode_blob(&compiled).expect("encode compiled flowerbed templates");
    let runtime = RuntimeAssets::decode(&bytes).expect("decode compiled flowerbed templates");
    assert_eq!(runtime.model_templates(), compiled.model_templates.as_ref());
    assert_eq!(runtime.model_quads(), compiled.model_quads.as_ref());
}

#[test]
fn compiler_keeps_flowerbeds_diagnostic_without_exact_second_terrain_variant() {
    let directory = tempfile::tempdir().expect("create incomplete flowerbed fixture");
    write_flowerbed_pack(directory.path(), false);
    let records = [generated_flowerbed_record(
        0,
        13_000,
        "minecraft:pink_petals",
        3,
        0,
    )];

    let compiled = compile_pack(directory.path(), &records).expect("compile incomplete flowerbed");
    assert_eq!(compiled.visuals[0].kind, VisualKind::Diagnostic);
    assert_eq!(
        compiled.visuals[0].model_template,
        assets::NO_MODEL_TEMPLATE
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
fn compiler_flowerbed_exact_pair_does_not_require_command_only_records() {
    let directory = tempfile::tempdir().expect("create exact-pair flowerbed fixture");
    write_flowerbed_pack(directory.path(), true);
    let records = (0..4)
        .map(|growth| {
            generated_flowerbed_record(growth, 13_100 + growth, "minecraft:wildflowers", growth, 2)
        })
        .collect::<Vec<_>>();

    let compiled = compile_pack(directory.path(), &records).expect("compile exact-pair flowerbed");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Model)
    );
    assert_eq!(compiled.model_templates.len(), 4);
}

#[test]
fn compiler_keeps_flowerbeds_diagnostic_for_an_overlong_terrain_variant_array() {
    let directory = tempfile::tempdir().expect("create malformed flowerbed fixture");
    write_pack(
        directory.path(),
        r#"{"pink_petals":{"textures":"pink_petals"}}"#,
        r#"{"texture_data":{
            "pink_petals":{"textures":[
                "textures/blocks/pink_petals",
                "textures/blocks/pink_petals_stem",
                "textures/blocks/pink_petals_unexpected"
            ]}
        }}"#,
        "[]",
    );
    for (index, path) in ["pink_petals", "pink_petals_stem", "pink_petals_unexpected"]
        .into_iter()
        .enumerate()
    {
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 43, 83, 0]),
        );
    }
    let records = (0..4)
        .map(|growth| {
            generated_flowerbed_record(growth, 14_000 + growth, "minecraft:pink_petals", growth, 0)
        })
        .collect::<Vec<_>>();

    let compiled = compile_pack(directory.path(), &records).expect("compile malformed flowerbed");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}
