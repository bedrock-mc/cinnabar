use super::support::*;

#[test]
fn pack_reader_strips_leading_comments_and_selects_first_terrain_variant() {
    let directory = tempfile::tempdir().expect("create fixture");
    let blocks = r#"{
        "format_version": [1, 1, 0],
        "stone": { "textures": "stone" },
        "column": { "textures": {
            "up": "column_top", "down": "column_bottom", "side": "column_side"
        }},
        "six": { "textures": {
            "west": "six_w", "east": "six_e", "down": "six_d",
            "up": "six_u", "north": "six_n", "south": "six_s"
        }}
    }"#;
    let terrain = r##"// generated header
        // a second complete leading comment line
        {
            "texture_data": {
                "stone": { "textures": "textures/blocks/stone" },
                "column_top": { "textures": {
                    "path": "textures/blocks/column_top", "overlay_color": "#ffffffff"
                }},
                "column_bottom": { "textures": [
                    "textures/blocks/column_bottom_first",
                    { "path": "textures/blocks/column_bottom_second" }
                ]},
                "column_side": { "textures": "textures/blocks/column_side" },
                "six_w": { "textures": "textures/blocks/six_w" },
                "six_e": { "textures": "textures/blocks/six_e" },
                "six_d": { "textures": "textures/blocks/six_d" },
                "six_u": { "textures": "textures/blocks/six_u" },
                "six_n": { "textures": "textures/blocks/six_n" },
                "six_s": { "textures": "textures/blocks/six_s" },
                "water": { "textures": "textures/blocks/water" }
            }
        }"##;
    let flipbooks = r#"// generated header
        [
            {
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                "ticks_per_frame": 2
            }
        ]"#;
    write_pack(directory.path(), blocks, terrain, flipbooks);

    let pack = read_pack(directory.path()).expect("valid synthetic pack");

    assert_eq!(pack.terrain.get("stone"), Some("textures/blocks/stone"));
    assert_eq!(
        pack.terrain.get("column_top"),
        Some("textures/blocks/column_top")
    );
    assert_eq!(
        pack.terrain.get("column_bottom"),
        Some("textures/blocks/column_bottom_first")
    );
    assert_eq!(pack.flipbooks.len(), 1);
    assert_eq!(&*pack.flipbooks[0].atlas_tile, "water");
    assert_eq!(&*pack.flipbooks[0].texture_path, "textures/blocks/water");

    for face in BlockFace::ALL {
        assert_key(
            resolve_texture_key(&pack.blocks, &record("minecraft:stone", "{}"), face),
            "stone",
            false,
        );
    }

    assert!(
        resolve_texture_key(&pack.blocks, &record("custom:stone", "{}"), BlockFace::Up)
            .key
            .is_none(),
        "only the exact minecraft: namespace is stripped"
    );

    let column = record("minecraft:column", "{}");
    for face in [
        BlockFace::West,
        BlockFace::East,
        BlockFace::North,
        BlockFace::South,
    ] {
        assert_key(
            resolve_texture_key(&pack.blocks, &column, face),
            "column_side",
            false,
        );
    }
    assert_key(
        resolve_texture_key(&pack.blocks, &column, BlockFace::Down),
        "column_bottom",
        false,
    );
    assert_key(
        resolve_texture_key(&pack.blocks, &column, BlockFace::Up),
        "column_top",
        false,
    );

    let explicit = [
        (BlockFace::West, "six_w"),
        (BlockFace::East, "six_e"),
        (BlockFace::Down, "six_d"),
        (BlockFace::Up, "six_u"),
        (BlockFace::North, "six_n"),
        (BlockFace::South, "six_s"),
    ];
    for (face, expected) in explicit {
        assert_key(
            resolve_texture_key(&pack.blocks, &record("minecraft:six", "{}"), face),
            expected,
            false,
        );
    }
}

#[test]
fn pack_reader_skips_untextured_and_carried_only_block_entries() {
    let directory = tempfile::tempdir().expect("create fixture");
    let blocks = r#"{
        "air": { "sound": "air" },
        "light_block": { "carried_textures": "stone" },
        "stone": { "textures": "stone" }
    }"#;
    write_pack(directory.path(), blocks, MINIMAL_TERRAIN, EMPTY_FLIPBOOKS);

    let pack = read_pack(directory.path()).expect("untextured entries are valid");

    for name in ["minecraft:air", "minecraft:light_block"] {
        assert!(
            resolve_texture_key(&pack.blocks, &record(name, "{}"), BlockFace::Up)
                .key
                .is_none(),
            "{name} must resolve to the diagnostic texture"
        );
    }
    assert_key(
        resolve_texture_key(
            &pack.blocks,
            &record("minecraft:stone", "{}"),
            BlockFace::Up,
        ),
        "stone",
        false,
    );
}

#[test]
fn explicit_legacy_block_aliases_preserve_face_keys_and_unknowns_stay_diagnostic() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "grass": {"textures": {
                "down": "grass_bottom", "side": "grass_side", "up": "grass_top"
            }},
            "seaLantern": {"textures": "sea_lantern"}
        }"#,
        r#"{"texture_data": {
            "grass_bottom": {"textures": "textures/blocks/grass_bottom"},
            "grass_side": {"textures": "textures/blocks/grass_side"},
            "grass_top": {"textures": "textures/blocks/grass_top"},
            "sea_lantern": {"textures": "textures/blocks/sea_lantern"}
        }}"#,
        EMPTY_FLIPBOOKS,
    );
    let pack = read_pack(directory.path()).expect("valid legacy-name pack");
    let grass = record("minecraft:grass_block", "null");

    assert_key(
        resolve_texture_key(&pack.blocks, &grass, BlockFace::Down),
        "grass_bottom",
        false,
    );
    assert_key(
        resolve_texture_key(&pack.blocks, &grass, BlockFace::Up),
        "grass_top",
        false,
    );
    for face in [
        BlockFace::West,
        BlockFace::East,
        BlockFace::North,
        BlockFace::South,
    ] {
        assert_key(
            resolve_texture_key(&pack.blocks, &grass, face),
            "grass_side",
            false,
        );
    }

    let sea_lantern = record("minecraft:sea_lantern", "null");
    for face in BlockFace::ALL {
        assert_key(
            resolve_texture_key(&pack.blocks, &sea_lantern, face),
            "sea_lantern",
            false,
        );
    }

    let invisible = record("minecraft:invisible_bedrock", "null");
    for face in BlockFace::ALL {
        assert!(
            resolve_texture_key(&pack.blocks, &invisible, face)
                .key
                .is_none(),
            "unlisted blocks must not acquire a legacy alias"
        );
    }
}

#[test]
fn all_hard_glass_panes_alias_exact_normal_body_and_edge_keys() {
    let directory = tempfile::tempdir().expect("create hard pane alias fixture");
    let colours = [
        "black",
        "blue",
        "brown",
        "cyan",
        "gray",
        "green",
        "light_blue",
        "light_gray",
        "lime",
        "magenta",
        "orange",
        "pink",
        "purple",
        "red",
        "white",
        "yellow",
    ];
    let mut blocks = serde_json::Map::new();
    let mut terrain = serde_json::Map::new();
    blocks.insert(
        "glass_pane".into(),
        serde_json::json!({"textures":{"side":"glass","east":"glass_pane_top"}}),
    );
    for key in ["glass", "glass_pane_top"] {
        terrain.insert(
            key.into(),
            serde_json::json!({"textures":format!("textures/blocks/{key}")}),
        );
    }
    for colour in colours {
        let body = format!("{colour}_stained_glass");
        let edge = format!("{colour}_stained_glass_pane_top");
        blocks.insert(
            format!("{colour}_stained_glass_pane"),
            serde_json::json!({"textures":{
                "side":body,
                "east":edge
            }}),
        );
        for key in [body, edge] {
            terrain.insert(
                key.clone(),
                serde_json::json!({"textures":format!("textures/blocks/{key}")}),
            );
        }
    }
    write_pack(
        directory.path(),
        &serde_json::Value::Object(blocks).to_string(),
        &serde_json::json!({"texture_data":terrain}).to_string(),
        EMPTY_FLIPBOOKS,
    );
    let pack = read_pack(directory.path()).expect("read hard pane alias fixture");
    for (hard, body, edge) in std::iter::once((
        "hard_glass_pane".to_owned(),
        "glass".to_owned(),
        "glass_pane_top".to_owned(),
    ))
    .chain(colours.into_iter().map(|colour| {
        (
            format!("hard_{colour}_stained_glass_pane"),
            format!("{colour}_stained_glass"),
            format!("{colour}_stained_glass_pane_top"),
        )
    })) {
        let record = record(&format!("minecraft:{hard}"), "{}");
        assert_key(
            resolve_texture_key(&pack.blocks, &record, BlockFace::North),
            &body,
            false,
        );
        assert_key(
            resolve_texture_key(&pack.blocks, &record, BlockFace::East),
            &edge,
            false,
        );
    }
}

#[test]
fn pack_reader_rejects_an_explicit_empty_face_map() {
    let directory = minimal_pack();
    write_file(
        directory.path().join("blocks.json"),
        r#"{"empty": {"textures": {}}}"#,
    );

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::MissingBlockTextureKeys(ref key)) if &**key == "empty"
    ));
}

#[test]
fn pack_reader_rejects_duplicate_top_level_block_names() {
    let directory = minimal_pack();
    let expected_path = directory.path().join("blocks.json");
    write_file(
        &expected_path,
        r#"{
            "stone": {"textures": "stone"},
            "stone": {"textures": "stone"}
        }"#,
    );

    let error = read_pack(directory.path()).expect_err("duplicate block name must fail");
    match &error {
        AssetError::DuplicateBlockKey { path, key } => {
            assert_eq!(path, &expected_path);
            assert_eq!(&**key, "stone");
        }
        other => panic!("unexpected error: {other}"),
    }
    let display = error.to_string();
    assert!(display.contains(expected_path.to_string_lossy().as_ref()));
    assert!(display.contains("stone"));
}

#[test]
fn malformed_block_entries_report_the_block_key() {
    let directory = minimal_pack();
    write_file(
        directory.path().join("blocks.json"),
        r#"{"broken_block": {"textures": 42}}"#,
    );

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::InvalidBlockEntry { ref block, .. }) if &**block == "broken_block"
    ));
}

#[test]
fn pack_reader_rejects_duplicate_terrain_texture_data_keys() {
    let directory = minimal_pack();
    let expected_path = directory.path().join("textures/terrain_texture.json");
    write_file(
        &expected_path,
        r#"{
            "texture_data": {
                "stone": {"textures": "textures/blocks/stone"},
                "stone": {"textures": "textures/blocks/stone"}
            }
        }"#,
    );

    let error = read_pack(directory.path()).expect_err("duplicate terrain key must fail");
    match &error {
        AssetError::DuplicateTerrainTextureKey { path, key } => {
            assert_eq!(path, &expected_path);
            assert_eq!(&**key, "stone");
        }
        other => panic!("unexpected error: {other}"),
    }
    let display = error.to_string();
    assert!(display.contains(expected_path.to_string_lossy().as_ref()));
    assert!(display.contains("stone"));
}

#[test]
fn pillar_axis_permutations_move_caps_and_rotate_horizontal_sides() {
    let directory = tempfile::tempdir().expect("create fixture");
    let blocks = r#"{
        "column": { "textures": {
            "up": "top", "down": "bottom", "side": "side"
        }}
    }"#;
    let terrain = r#"{
        "texture_data": {
            "top": { "textures": "textures/blocks/top" },
            "bottom": { "textures": "textures/blocks/bottom" },
            "side": { "textures": "textures/blocks/side" }
        }
    }"#;
    write_pack(directory.path(), blocks, terrain, EMPTY_FLIPBOOKS);
    let pack = read_pack(directory.path()).expect("valid pillar pack");

    let x = record(
        "minecraft:column",
        r#"{"pillar_axis":{"type":"string","value":"x"}}"#,
    );
    assert_key(
        resolve_texture_key(&pack.blocks, &x, BlockFace::West),
        "bottom",
        false,
    );
    assert_key(
        resolve_texture_key(&pack.blocks, &x, BlockFace::East),
        "top",
        false,
    );
    for face in [
        BlockFace::Down,
        BlockFace::Up,
        BlockFace::North,
        BlockFace::South,
    ] {
        assert_key(resolve_texture_key(&pack.blocks, &x, face), "side", true);
    }

    let y = record(
        "minecraft:column",
        r#"{"pillar_axis":{"type":"string","value":"y"}}"#,
    );
    assert_key(
        resolve_texture_key(&pack.blocks, &y, BlockFace::Down),
        "bottom",
        false,
    );
    assert_key(
        resolve_texture_key(&pack.blocks, &y, BlockFace::Up),
        "top",
        false,
    );
    assert_key(
        resolve_texture_key(&pack.blocks, &y, BlockFace::West),
        "side",
        false,
    );

    let z = record(
        "minecraft:column",
        r#"{"pillar_axis":{"type":"string","value":"z"}}"#,
    );
    assert_key(
        resolve_texture_key(&pack.blocks, &z, BlockFace::North),
        "bottom",
        false,
    );
    assert_key(
        resolve_texture_key(&pack.blocks, &z, BlockFace::South),
        "top",
        false,
    );
    for face in [
        BlockFace::West,
        BlockFace::East,
        BlockFace::Down,
        BlockFace::Up,
    ] {
        assert_key(resolve_texture_key(&pack.blocks, &z, face), "side", true);
    }

    let legacy = record("minecraft:column", r#"{"axis":"z"}"#);
    assert_key(
        resolve_texture_key(&pack.blocks, &legacy, BlockFace::North),
        "bottom",
        false,
    );
}

#[test]
fn malformed_tagged_pillar_axes_fail_closed_to_diagnostic() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"hay_block":{"textures":{"up":"top","down":"bottom","side":"side"}}}"#,
        r#"{"texture_data":{"top":{"textures":"textures/blocks/top"},"bottom":{"textures":"textures/blocks/bottom"},"side":{"textures":"textures/blocks/side"}}}"#,
        EMPTY_FLIPBOOKS,
    );
    let pack = read_pack(directory.path()).expect("valid pillar pack");

    for malformed in [
        r#"{"pillar_axis":{"type":"string"}}"#,
        r#"{"pillar_axis":{"type":"string","value":"x","extra":0}}"#,
        r#"{"pillar_axis":{"type":"int","value":0}}"#,
        r#"{"pillar_axis":{"type":"string","value":0}}"#,
        r#"{"pillar_axis":{"type":"string","value":"q"}}"#,
        r#"{"pillar_axis":{"type":"string","value":"x"},"axis":{"type":"string","value":"x"}}"#,
    ] {
        let resolved = resolve_texture_key(
            &pack.blocks,
            &record("minecraft:hay_block", malformed),
            BlockFace::West,
        );
        assert_eq!(
            resolved.key, None,
            "malformed state was admitted: {malformed}"
        );
        assert!(!resolved.rotate_uv);
    }
}

#[test]
fn pack_reader_rejects_parent_and_absolute_texture_paths() {
    let directory = minimal_pack();
    for unsafe_path in ["../outside", "/absolute/outside", r"C:\absolute\outside"] {
        let terrain = format!(
            r#"{{"texture_data":{{"stone":{{"textures":{}}}}}}}"#,
            serde_json::to_string(unsafe_path).expect("serialize path")
        );
        write_file(
            directory.path().join("textures/terrain_texture.json"),
            terrain,
        );

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::UnsafeTexturePath { .. })
        ));
    }
}

#[test]
fn pack_reader_rejects_missing_terrain_keys() {
    let directory = minimal_pack();
    write_file(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data": {}}"#,
    );

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::MissingTerrainKey { ref key, .. }) if &**key == "stone"
    ));
}

#[test]
fn pack_reader_rejects_invalid_json_utf8() {
    let directory = minimal_pack();
    write_file(
        directory.path().join("textures/terrain_texture.json"),
        [0xff, 0xfe],
    );

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::InvalidJsonUtf8 { .. })
    ));
}

#[test]
fn pack_reader_rejects_non_leading_json_comments() {
    let directory = minimal_pack();
    write_file(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data": {} // not a complete leading line
        }"#,
    );

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::Json { .. })
    ));
}

#[test]
fn pack_reader_enforces_json_texture_variant_and_path_bounds() {
    let directory = minimal_pack();
    let terrain_path = directory.path().join("textures/terrain_texture.json");

    write_file(&terrain_path, vec![b' '; 16 * 1024 * 1024 + 1]);
    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::JsonTooLarge { .. })
    ));

    let mut keys = serde_json::Map::new();
    for index in 0..8_193 {
        keys.insert(
            format!("key_{index}"),
            serde_json::json!({ "textures": format!("textures/blocks/{index}") }),
        );
    }
    write_file(
        &terrain_path,
        serde_json::to_vec(&serde_json::json!({ "texture_data": keys }))
            .expect("serialize many keys"),
    );
    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::TooManyTextureKeys {
            count: 8_193,
            max: 8_192
        })
    ));

    let variants = (0..257)
        .map(|index| format!("textures/blocks/{index}"))
        .collect::<Vec<_>>();
    write_file(
        &terrain_path,
        serde_json::to_vec(&serde_json::json!({
            "texture_data": { "stone": { "textures": variants } }
        }))
        .expect("serialize variants"),
    );
    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::TooManyTextureVariants {
            count: 257,
            max: 256,
            ..
        })
    ));

    let long_path = format!("textures/blocks/{}", "x".repeat(4_096));
    write_file(
        &terrain_path,
        serde_json::to_vec(&serde_json::json!({
            "texture_data": { "stone": { "textures": long_path } }
        }))
        .expect("serialize long path"),
    );
    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::TexturePathTooLong { max: 4_096, .. })
    ));
}

#[test]
fn missing_pack_files_report_the_exact_source_path() {
    let directory = tempfile::tempdir().expect("create fixture");
    let expected = directory.path().join("blocks.json");

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::Io { ref path, .. }) if path == &expected
    ));
}
