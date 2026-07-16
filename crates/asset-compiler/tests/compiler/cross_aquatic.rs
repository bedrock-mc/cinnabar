use super::support::*;

#[test]
fn compiler_compiles_exact_terrestrial_cross_alias_tint_and_crop_variants() {
    let directory = tempfile::tempdir().expect("create terrestrial cross fixture");
    write_pack(
        directory.path(),
        r#"{
            "short_grass":{"textures":"short_grass"},
            "fern":{"textures":"fern"},
            "yellow_flower":{"textures":"yellow_flower"},
            "sapling":{"textures":"sapling"},
            "wheat":{"textures":"wheat"},
            "carrots":{"textures":"carrots"},
            "melon_stem":{"textures":"melon_stem"}
        }"#,
        r#"{"texture_data":{
            "short_grass":{"textures":"textures/blocks/short_grass"},
            "fern":{"textures":"textures/blocks/fern"},
            "yellow_flower":{"textures":"textures/blocks/dandelion"},
            "sapling":{"textures":["textures/blocks/oak","textures/blocks/spruce"]},
            "wheat":{"textures":[
                "textures/blocks/wheat0","textures/blocks/wheat1",
                "textures/blocks/wheat2","textures/blocks/wheat3",
                "textures/blocks/wheat4","textures/blocks/wheat5",
                "textures/blocks/wheat6","textures/blocks/wheat7"
            ]},
            "carrots":{"textures":[
                "textures/blocks/carrots0","textures/blocks/carrots1",
                "textures/blocks/carrots2","textures/blocks/carrots3"
            ]},
            "melon_stem":{"textures":[
                "textures/blocks/melon_disconnected","textures/blocks/melon_connected"
            ]}
        }}"#,
        "[]",
    );
    for (index, path) in [
        "short_grass",
        "fern",
        "dandelion",
        "oak",
        "spruce",
        "wheat0",
        "wheat1",
        "wheat2",
        "wheat3",
        "wheat4",
        "wheat5",
        "wheat6",
        "wheat7",
        "carrots0",
        "carrots1",
        "carrots2",
        "carrots3",
        "melon_disconnected",
        "melon_connected",
    ]
    .into_iter()
    .enumerate()
    {
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 17, 31, 255]),
        );
    }
    let records = [
        model_record(0, 100, "minecraft:short_grass", "{}", ModelFamily::Cross),
        model_record(1, 101, "minecraft:fern", "{}", ModelFamily::Cross),
        model_record(2, 102, "minecraft:dandelion", "{}", ModelFamily::Cross),
        model_record(
            3,
            103,
            "minecraft:oak_sapling",
            r#"{"age_bit":{"type":"byte","value":1}}"#,
            ModelFamily::Cross,
        ),
        model_record(
            4,
            104,
            "minecraft:wheat",
            r#"{"growth":{"type":"int","value":0}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            5,
            105,
            "minecraft:wheat",
            r#"{"growth":{"type":"int","value":7}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            6,
            106,
            "minecraft:carrots",
            r#"{"growth":{"type":"int","value":5}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            7,
            107,
            "minecraft:melon_stem",
            r#"{"facing_direction":{"type":"int","value":0},"growth":{"type":"int","value":7}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            8,
            108,
            "minecraft:melon_stem",
            r#"{"facing_direction":{"type":"int","value":2},"growth":{"type":"int","value":7}}"#,
            ModelFamily::Crop,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile crossed plants");
    assert!(compiled.visuals.iter().all(|visual| {
        visual.kind == VisualKind::Cross && visual.model_template != assets::NO_MODEL_TEMPLATE
    }));
    assert_eq!(
        compiled
            .visuals
            .iter()
            .map(|visual| visual.variant)
            .collect::<Vec<_>>(),
        [0, 0, 0, 0, 0, 7, 2, 0, 1]
    );
    for (index, visual) in compiled.visuals.iter().enumerate() {
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(
            template.quad_count, 2,
            "visual {index} did not use one crossed pair"
        );
        assert!(template.quad_count <= 32);
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert!(
            quads
                .iter()
                .all(|quad| quad.flags == MODEL_QUAD_FLAG_TWO_SIDED)
        );
        assert!(quads.iter().all(|quad| {
            compiled.materials[quad.material as usize].flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0
        }));
    }
    for index in [0_usize, 1] {
        let template = compiled.model_templates[compiled.visuals[index].model_template as usize];
        let material = compiled.model_quads[template.quad_start as usize].material as usize;
        assert_eq!(
            compiled.materials[material].flags & MATERIAL_FLAG_TINT_MASK,
            MATERIAL_FLAG_GRASS_TINT,
            "grass and fern must use the biome grass tint class"
        );
    }
    for index in 2..compiled.visuals.len() {
        let template = compiled.model_templates[compiled.visuals[index].model_template as usize];
        let material = compiled.model_quads[template.quad_start as usize].material as usize;
        assert_eq!(
            compiled.materials[material].flags & MATERIAL_FLAG_TINT_MASK,
            0
        );
    }
    assert_ne!(
        compiled.visuals[4].model_template,
        compiled.visuals[5].model_template
    );
    assert_ne!(
        compiled.visuals[4].model_template,
        compiled.visuals[6].model_template
    );
}

#[test]
fn compiler_compiles_exact_animated_seagrass_pairs_without_biome_tint() {
    let directory = tempfile::tempdir().expect("create seagrass fixture");
    write_pack(
        directory.path(),
        r#"{
            "seagrass":{"textures":{
                "up":"seagrass_short",
                "down":"seagrass_tall_bot_a",
                "south":"seagrass_tall_bot_b",
                "east":"seagrass_tall_top_a",
                "west":"seagrass_tall_top_b"
            }}
        }"#,
        r#"{"texture_data":{
            "seagrass_short":{"textures":"textures/blocks/seagrass"},
            "seagrass_tall_bot_a":{"textures":"textures/blocks/seagrass_bottom_a"},
            "seagrass_tall_bot_b":{"textures":"textures/blocks/seagrass_bottom_b"},
            "seagrass_tall_top_a":{"textures":"textures/blocks/seagrass_top_a"},
            "seagrass_tall_top_b":{"textures":"textures/blocks/seagrass_top_b"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/seagrass","atlas_tile":"seagrass_short","ticks_per_frame":4},
            {"flipbook_texture":"textures/blocks/seagrass_bottom_a","atlas_tile":"seagrass_tall_bot_a","ticks_per_frame":3},
            {"flipbook_texture":"textures/blocks/seagrass_bottom_b","atlas_tile":"seagrass_tall_bot_b","ticks_per_frame":3},
            {"flipbook_texture":"textures/blocks/seagrass_top_a","atlas_tile":"seagrass_tall_top_a","ticks_per_frame":3},
            {"flipbook_texture":"textures/blocks/seagrass_top_b","atlas_tile":"seagrass_tall_top_b","ticks_per_frame":3}
        ]"#,
    );
    for (index, path) in [
        "seagrass",
        "seagrass_bottom_a",
        "seagrass_bottom_b",
        "seagrass_top_a",
        "seagrass_top_b",
    ]
    .into_iter()
    .enumerate()
    {
        let mut strip = solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 40, 80, 255]);
        strip.extend(solid(TILE_SIZE, TILE_SIZE, [index as u8 + 11, 50, 90, 255]));
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 2,
            &strip,
        );
    }
    let records = [
        model_record(
            0,
            200,
            "minecraft:seagrass",
            r#"{"sea_grass_type":{"type":"string","value":"default"}}"#,
            ModelFamily::Aquatic,
        ),
        model_record(
            1,
            201,
            "minecraft:seagrass",
            r#"{"sea_grass_type":{"type":"string","value":"double_bot"}}"#,
            ModelFamily::Aquatic,
        ),
        model_record(
            2,
            202,
            "minecraft:seagrass",
            r#"{"sea_grass_type":{"type":"string","value":"double_top"}}"#,
            ModelFamily::Aquatic,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile animated seagrass");
    assert_eq!(compiled.visuals.len(), 3);
    let expected_ticks = [[4, 4], [3, 3], [3, 3]];
    for (index, visual) in compiled.visuals.iter().enumerate() {
        assert_eq!(visual.kind, VisualKind::Cross);
        assert_ne!(visual.model_template, assets::NO_MODEL_TEMPLATE);
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.quad_count, 2);
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        for (quad, ticks) in quads.iter().zip(expected_ticks[index]) {
            assert_eq!(quad.flags, MODEL_QUAD_FLAG_TWO_SIDED);
            let material = compiled.materials[quad.material as usize];
            assert_eq!(
                material.flags & MATERIAL_FLAG_ALPHA_CUTOUT,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(material.flags & MATERIAL_FLAG_TINT_MASK, 0);
            assert_ne!(material.animation, assets::NO_ANIMATION);
            assert_eq!(
                compiled.animations[material.animation as usize].ticks_per_frame,
                ticks
            );
        }
        assert!(visual.flags.is_empty());
    }
    let materials_for = |index: usize| {
        let template = compiled.model_templates[compiled.visuals[index].model_template as usize];
        compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
            .iter()
            .map(|quad| quad.material)
            .collect::<Vec<_>>()
    };
    let short = materials_for(0);
    assert_eq!(short[0], short[1]);
    assert_ne!(materials_for(1)[0], materials_for(1)[1]);
    assert_ne!(materials_for(2)[0], materials_for(2)[1]);
}

#[test]
fn compiler_compiles_all_kelp_ages_as_six_animated_body_and_head_faces() {
    let directory = tempfile::tempdir().expect("create kelp fixture");
    write_pack(
        directory.path(),
        r#"{"kelp":{"textures":{
            "down":"kelp_d","east":"kelp_top","north":"kelp_a",
            "south":"kelp_b","up":"kelp_c","west":"kelp_top_bulb"
        }}}"#,
        r#"{"texture_data":{
            "kelp_a":{"textures":"textures/blocks/kelp_a"},
            "kelp_b":{"textures":"textures/blocks/kelp_b"},
            "kelp_c":{"textures":"textures/blocks/kelp_c"},
            "kelp_d":{"textures":"textures/blocks/kelp_d"},
            "kelp_top":{"textures":"textures/blocks/kelp_top"},
            "kelp_top_bulb":{"textures":"textures/blocks/kelp_top_bulb"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/kelp_a","atlas_tile":"kelp_a","ticks_per_frame":4,"frames":[0,1,2,3,4,5]},
            {"flipbook_texture":"textures/blocks/kelp_b","atlas_tile":"kelp_b","ticks_per_frame":4,"frames":[1,2,3,4,5,0]},
            {"flipbook_texture":"textures/blocks/kelp_c","atlas_tile":"kelp_c","ticks_per_frame":4,"frames":[2,3,4,5,0,1]},
            {"flipbook_texture":"textures/blocks/kelp_d","atlas_tile":"kelp_d","ticks_per_frame":4,"frames":[3,4,5,0,1,2]},
            {"flipbook_texture":"textures/blocks/kelp_top","atlas_tile":"kelp_top","ticks_per_frame":4,"frames":[4,5,0,1,2,3]},
            {"flipbook_texture":"textures/blocks/kelp_top_bulb","atlas_tile":"kelp_top_bulb","ticks_per_frame":4,"frames":[5,0,1,2,3,4]}
        ]"#,
    );
    for (texture_index, path) in [
        "kelp_a",
        "kelp_b",
        "kelp_c",
        "kelp_d",
        "kelp_top",
        "kelp_top_bulb",
    ]
    .into_iter()
    .enumerate()
    {
        let strip = (0..6)
            .flat_map(|frame| {
                solid(
                    TILE_SIZE,
                    TILE_SIZE,
                    [texture_index as u8 + 1, frame as u8 + 20, 90, 255],
                )
            })
            .collect::<Vec<_>>();
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 6,
            &strip,
        );
    }
    let records = (0..26)
        .map(|age| {
            model_record(
                age,
                300 + age,
                "minecraft:kelp",
                &format!(r#"{{"kelp_age":{{"type":"int","value":{age}}}}}"#),
                ModelFamily::Aquatic,
            )
        })
        .collect::<Vec<_>>();

    let compiled = compile_pack(directory.path(), &records).expect("compile all kelp ages");
    assert_eq!(compiled.visuals.len(), 26);
    assert!(compiled.visuals.iter().all(|visual| {
        visual.kind == VisualKind::Model
            && visual.model_template == compiled.visuals[0].model_template
            && visual.flags.is_empty()
    }));
    let template = compiled.model_templates[compiled.visuals[0].model_template as usize];
    assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_KELP);
    assert_eq!(template.quad_count, 6);
    let quads = &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
    assert_eq!(
        quads.iter().map(|quad| quad.material).collect::<Vec<_>>(),
        [
            compiled.visuals[0].faces[BlockFace::North as usize],
            compiled.visuals[0].faces[BlockFace::South as usize],
            compiled.visuals[0].faces[BlockFace::Up as usize],
            compiled.visuals[0].faces[BlockFace::Down as usize],
            compiled.visuals[0].faces[BlockFace::East as usize],
            compiled.visuals[0].faces[BlockFace::West as usize],
        ]
    );
    assert_eq!(
        quads.iter().map(|quad| quad.flags).collect::<Vec<_>>(),
        [
            0,
            0,
            0,
            0,
            MODEL_QUAD_FLAG_TWO_SIDED,
            MODEL_QUAD_FLAG_TWO_SIDED
        ]
    );
    assert_eq!(
        quads[2].positions,
        [
            quads[0].positions[1],
            quads[0].positions[0],
            quads[0].positions[3],
            quads[0].positions[2]
        ]
    );
    assert_eq!(
        quads[3].positions,
        [
            quads[1].positions[1],
            quads[1].positions[0],
            quads[1].positions[3],
            quads[1].positions[2]
        ]
    );
    let normal = |quad: &assets::ModelQuad| {
        let a = [
            i64::from(quad.positions[1][0]) - i64::from(quad.positions[0][0]),
            i64::from(quad.positions[1][1]) - i64::from(quad.positions[0][1]),
            i64::from(quad.positions[1][2]) - i64::from(quad.positions[0][2]),
        ];
        let b = [
            i64::from(quad.positions[2][0]) - i64::from(quad.positions[0][0]),
            i64::from(quad.positions[2][1]) - i64::from(quad.positions[0][1]),
            i64::from(quad.positions[2][2]) - i64::from(quad.positions[0][2]),
        ];
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    for (forward, reverse) in [(0, 2), (1, 3)] {
        let forward = normal(&quads[forward]);
        let reverse = normal(&quads[reverse]);
        assert!(
            forward
                .into_iter()
                .zip(reverse)
                .map(|(left, right)| left * right)
                .sum::<i64>()
                < 0,
            "kelp body windings must face opposite directions"
        );
    }
    let animations = quads
        .iter()
        .map(|quad| compiled.materials[quad.material as usize])
        .map(|material| {
            assert_eq!(
                material.flags & MATERIAL_FLAG_ALPHA_CUTOUT,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(material.flags & MATERIAL_FLAG_TINT_MASK, 0);
            assert_ne!(material.animation, assets::NO_ANIMATION);
            let animation = compiled.animations[material.animation as usize];
            assert_eq!(animation.ticks_per_frame, 4);
            compiled.animation_frames[animation.frame_start as usize
                ..(animation.frame_start + animation.frame_count) as usize]
                .to_vec()
        })
        .collect::<Vec<_>>();
    assert_eq!(animations.len(), 6);
    for left in 0..animations.len() {
        for right in left + 1..animations.len() {
            assert_ne!(animations[left], animations[right]);
        }
    }
}
