use std::{fs, path::Path};

use assets::{
    AssetError, BlockFace, BlockFlags, MAX_FLIPBOOK_FRAMES, MAX_FLIPBOOKS, RegistryRecord,
    TextureKey, read_pack, read_registry, resolve_texture_key,
};
use tempfile::TempDir;

const MINIMAL_BLOCKS: &str = r#"{
    "format_version": [1, 1, 0],
    "stone": { "textures": "stone" }
}"#;
const MINIMAL_TERRAIN: &str = r#"{
    "texture_data": {
        "stone": { "textures": "textures/blocks/stone" }
    }
}"#;
const EMPTY_FLIPBOOKS: &str = "[]";
type RegistryFixture<'a> = (u32, u32, u8, &'a [u8], &'a [u8]);

fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture directory");
    }
    fs::write(path, contents).expect("write fixture");
}

fn write_pack(root: &Path, blocks: &str, terrain: &str, flipbooks: &str) {
    write_file(root.join("blocks.json"), blocks);
    write_file(root.join("textures/terrain_texture.json"), terrain);
    write_file(root.join("textures/flipbook_textures.json"), flipbooks);
}

fn minimal_pack() -> TempDir {
    let directory = tempfile::tempdir().expect("create pack fixture");
    write_pack(
        directory.path(),
        MINIMAL_BLOCKS,
        MINIMAL_TERRAIN,
        EMPTY_FLIPBOOKS,
    );
    directory
}

fn pack_with_flipbooks(flipbooks: &str) -> TempDir {
    let directory = tempfile::tempdir().expect("create flipbook fixture");
    write_pack(
        directory.path(),
        MINIMAL_BLOCKS,
        r#"{
            "texture_data": {
                "stone": { "textures": "textures/blocks/stone" },
                "water": { "textures": "textures/blocks/water" },
                "lava": { "textures": "textures/blocks/lava" }
            }
        }"#,
        flipbooks,
    );
    directory
}

fn registry_bytes(records: &[RegistryFixture<'_>]) -> Vec<u8> {
    let mut bytes = b"BREG1002".to_vec();
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for &(sequential_id, network_hash, flags, name, state) in records {
        bytes.extend_from_slice(&sequential_id.to_le_bytes());
        bytes.extend_from_slice(&network_hash.to_le_bytes());
        bytes.push(flags);
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&(state.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(state);
    }
    bytes
}

fn record(name: &str, canonical_state: &str) -> RegistryRecord {
    RegistryRecord {
        sequential_id: 7,
        network_hash: 0x8000_0007,
        name: name.into(),
        canonical_state: canonical_state.into(),
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
    }
}

fn assert_key(actual: TextureKey, expected_key: &str, rotate_uv: bool) {
    assert_eq!(actual.key.as_deref(), Some(expected_key));
    assert_eq!(actual.rotate_uv, rotate_uv);
}

#[test]
fn block_faces_match_the_packed_renderer_discriminants() {
    let faces = [
        BlockFace::West,
        BlockFace::East,
        BlockFace::Down,
        BlockFace::Up,
        BlockFace::North,
        BlockFace::South,
    ];

    assert_eq!(faces.map(|face| face as u8), [0, 1, 2, 3, 4, 5]);
}

#[test]
fn registry_reader_decodes_dragonfly_records_and_flags() {
    let bytes = registry_bytes(&[
        (0, 0xdbf4_4120, 1, b"minecraft:air", b"{}"),
        (
            1,
            0x9123_4567,
            6,
            b"minecraft:stone",
            br#"{"stone_type":"stone"}"#,
        ),
    ]);

    let records = read_registry(&bytes).expect("valid registry");

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].sequential_id, 0);
    assert_eq!(records[0].network_hash, 0xdbf4_4120);
    assert_eq!(&*records[0].name, "minecraft:air");
    assert_eq!(&*records[0].canonical_state, "{}");
    assert_eq!(records[0].flags, BlockFlags::AIR);
    assert_eq!(
        records[1].flags,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
    );
}

#[test]
fn block_flag_semantics_accept_only_independent_valid_combinations() {
    for valid in [
        BlockFlags::empty(),
        BlockFlags::AIR,
        BlockFlags::CUBE_GEOMETRY,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
    ] {
        assert!(valid.has_valid_semantics(), "rejected {valid:?}");
    }

    for invalid in [
        BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY,
        BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::LEAF_MODEL,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL,
    ] {
        assert!(!invalid.has_valid_semantics(), "accepted {invalid:?}");
    }
}

#[test]
fn registry_reader_rejects_unknown_and_invalid_semantic_flags() {
    for raw in [0x10, 0x03, 0x04, 0x08, 0x0e] {
        let bytes = registry_bytes(&[(3, 11, raw, b"minecraft:test", b"{}")]);
        assert!(matches!(
            read_registry(&bytes),
            Err(AssetError::InvalidRegistryFlags(actual)) if actual == raw
        ));
    }
}

#[test]
fn registry_reader_rejects_old_schema_magic() {
    let mut bytes = registry_bytes(&[]);
    bytes[..8].copy_from_slice(b"BREG1001");
    assert!(matches!(
        read_registry(&bytes),
        Err(AssetError::InvalidRegistryMagic)
    ));
}

#[test]
fn registry_reader_rejects_duplicate_sequential_ids() {
    let bytes = registry_bytes(&[
        (3, 11, 0, b"minecraft:first", b"{}"),
        (3, 12, 0, b"minecraft:second", b"{}"),
    ]);

    assert!(matches!(
        read_registry(&bytes),
        Err(AssetError::DuplicateSequentialId(3))
    ));
}

#[test]
fn registry_reader_rejects_duplicate_network_hashes() {
    let bytes = registry_bytes(&[
        (3, 11, 0, b"minecraft:first", b"{}"),
        (4, 11, 0, b"minecraft:second", b"{}"),
    ]);

    assert!(matches!(
        read_registry(&bytes),
        Err(AssetError::DuplicateNetworkHash(11))
    ));
}

#[test]
fn registry_reader_rejects_oversized_counts_before_record_allocation() {
    let mut bytes = b"BREG1002".to_vec();
    bytes.extend_from_slice(&65_537_u32.to_le_bytes());

    assert!(matches!(
        read_registry(&bytes),
        Err(AssetError::TooManyRegistryRecords {
            count: 65_537,
            max: 65_536
        })
    ));
}

#[test]
fn registry_reader_rejects_truncated_records() {
    let mut bytes = registry_bytes(&[(3, 11, 0, b"minecraft:stone", b"{}")]);
    bytes.pop();

    assert!(matches!(
        read_registry(&bytes),
        Err(AssetError::UnexpectedEof { .. })
    ));
}

#[test]
fn registry_reader_rejects_invalid_utf8() {
    let bytes = registry_bytes(&[(3, 11, 0, &[0xff], b"{}")]);

    assert!(matches!(
        read_registry(&bytes),
        Err(AssetError::InvalidRegistryUtf8 { field: "name", .. })
    ));
}

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

    let x = record("minecraft:column", r#"{"pillar_axis":"x"}"#);
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

    let y = record("minecraft:column", r#"{"pillar_axis":"y"}"#);
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

    let z = record("minecraft:column", r#"{"axis":"z"}"#);
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

#[test]
fn flipbook_preserves_complete_metadata_defaults_and_order() {
    let directory = pack_with_flipbooks(
        r#"[
            {
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                "ticks_per_frame": 3,
                "frames": [7, 2, 9],
                "atlas_index": 4,
                "atlas_tile_variant": 6,
                "replicate": 2,
                "blend_frames": true
            },
            {
                "flipbook_texture": "textures/blocks/lava",
                "atlas_tile": "lava"
            }
        ]"#,
    );

    let pack = read_pack(directory.path()).expect("valid complete flipbook metadata");

    assert_eq!(pack.flipbooks.len(), 2);
    let water = &pack.flipbooks[0];
    assert_eq!(&*water.texture_path, "textures/blocks/water");
    assert_eq!(&*water.atlas_tile, "water");
    assert_eq!(water.ticks_per_frame, 3);
    assert_eq!(&*water.frames, &[7, 2, 9]);
    assert_eq!(water.atlas_index, 4);
    assert_eq!(water.atlas_tile_variant, 6);
    assert_eq!(water.replicate, 2);
    assert!(water.blend_frames);

    let lava = &pack.flipbooks[1];
    assert_eq!(&*lava.texture_path, "textures/blocks/lava");
    assert_eq!(&*lava.atlas_tile, "lava");
    assert_eq!(lava.ticks_per_frame, 1);
    assert!(lava.frames.is_empty());
    assert_eq!(lava.atlas_index, 0);
    assert_eq!(lava.atlas_tile_variant, 0);
    assert_eq!(lava.replicate, 1);
    assert!(!lava.blend_frames);
}

#[test]
fn flipbook_rejects_zero_timing_and_replication() {
    for (field, extra) in [
        ("ticks_per_frame", r#", "ticks_per_frame": 0"#),
        ("replicate", r#", "replicate": 0"#),
    ] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water"{extra}
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::ZeroFlipbookValue {
                index: 0,
                field: actual,
            }) if actual == field
        ));
    }
}

#[test]
fn flipbook_rejects_negative_and_non_integer_frame_values() {
    for invalid in ["-1", "1.5", r#""zero""#] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                "frames": [0, {invalid}]
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::InvalidFlipbookInteger {
                index: 0,
                field: "frames",
                element: Some(1),
            })
        ));
    }
}

#[test]
fn flipbook_rejects_out_of_range_numeric_metadata() {
    for field in [
        "ticks_per_frame",
        "atlas_index",
        "atlas_tile_variant",
        "replicate",
    ] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                "{field}": 4294967296
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::InvalidFlipbookInteger {
                index: 0,
                field: actual,
                element: None,
            }) if actual == field
        ));
    }
}

#[test]
fn flipbook_rejects_wrong_metadata_types() {
    for (field, extra, expected) in [
        (
            "ticks_per_frame",
            r#""ticks_per_frame": "one""#,
            "unsigned 32-bit integer",
        ),
        (
            "frames",
            r#""frames": {}"#,
            "array of unsigned 32-bit integers",
        ),
        ("blend_frames", r#""blend_frames": 1"#, "boolean"),
    ] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                {extra}
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::InvalidFlipbookFieldType {
                index: 0,
                field: actual,
                expected: actual_expected,
            }) if actual == field && actual_expected == expected
        ));
    }
}

#[test]
fn flipbook_rejects_explicit_null_for_every_optional_field() {
    for (field, expected) in [
        ("ticks_per_frame", "unsigned 32-bit integer"),
        ("frames", "array of unsigned 32-bit integers"),
        ("atlas_index", "unsigned 32-bit integer"),
        ("atlas_tile_variant", "unsigned 32-bit integer"),
        ("replicate", "unsigned 32-bit integer"),
        ("blend_frames", "boolean"),
    ] {
        let flipbooks = format!(
            r#"[{{
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water",
                "{field}": null
            }}]"#
        );
        let directory = pack_with_flipbooks(&flipbooks);

        assert!(matches!(
            read_pack(directory.path()),
            Err(AssetError::InvalidFlipbookFieldType {
                index: 0,
                field: actual,
                expected: actual_expected,
            }) if actual == field && actual_expected == expected
        ));
    }
}

#[test]
fn flipbook_canonicalizes_selector_defaults_before_duplicate_detection() {
    let directory = pack_with_flipbooks(
        r#"[
            {
                "flipbook_texture": "textures/blocks/water",
                "atlas_tile": "water"
            },
            {
                "flipbook_texture": "textures/blocks/lava",
                "atlas_tile": "water",
                "atlas_index": 0,
                "atlas_tile_variant": 0
            }
        ]"#,
    );

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::DuplicateFlipbookSelector {
            ref atlas_tile,
            atlas_index: 0,
            atlas_tile_variant: 0,
        }) if &**atlas_tile == "water"
    ));
}

#[test]
fn flipbook_rejects_excessive_explicit_frame_lists() {
    let frames = std::iter::repeat_n("0", MAX_FLIPBOOK_FRAMES + 1)
        .collect::<Vec<_>>()
        .join(",");
    let flipbooks = format!(
        r#"[{{
            "flipbook_texture": "textures/blocks/water",
            "atlas_tile": "water",
            "frames": [{frames}]
        }}]"#
    );
    let directory = pack_with_flipbooks(&flipbooks);

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::TooManyFlipbookFrames {
            index: 0,
            count,
            max,
        }) if count == MAX_FLIPBOOK_FRAMES + 1 && max == MAX_FLIPBOOK_FRAMES
    ));
}

#[test]
fn flipbook_rejects_excessive_global_list() {
    let entry = r#"{
        "flipbook_texture": "textures/blocks/water",
        "atlas_tile": "water"
    }"#;
    let flipbooks = std::iter::repeat_n(entry, MAX_FLIPBOOKS + 1)
        .collect::<Vec<_>>()
        .join(",");
    let directory = pack_with_flipbooks(&format!("[{flipbooks}]"));

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::TooManyFlipbooks { count, max })
            if count == MAX_FLIPBOOKS + 1 && max == MAX_FLIPBOOKS
    ));
}

#[test]
fn flipbook_rejects_timeline_arithmetic_overflow() {
    let directory = pack_with_flipbooks(
        r#"[{
            "flipbook_texture": "textures/blocks/water",
            "atlas_tile": "water",
            "ticks_per_frame": 4294967295,
            "frames": [0, 1]
        }]"#,
    );

    assert!(matches!(
        read_pack(directory.path()),
        Err(AssetError::FlipbookTimelineOverflow { index: 0 })
    ));
}

#[test]
fn flipbook_replication_is_spatial_not_temporal() {
    let directory = pack_with_flipbooks(
        r#"[{
            "flipbook_texture": "textures/blocks/water",
            "atlas_tile": "water",
            "ticks_per_frame": 2,
            "replicate": 4294967295
        }]"#,
    );

    let pack = read_pack(directory.path()).expect("spatial replication must not overflow timing");
    assert_eq!(pack.flipbooks[0].ticks_per_frame, 2);
    assert!(pack.flipbooks[0].frames.is_empty());
    assert_eq!(pack.flipbooks[0].replicate, u32::MAX);
}
