use std::{fs, path::Path};

use assets::{
    AssetError, BlockFace, BlockFlags, RegistryRecord, TextureKey, read_pack, read_registry,
    resolve_texture_key,
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

fn registry_bytes(records: &[RegistryFixture<'_>]) -> Vec<u8> {
    let mut bytes = b"BREG1001".to_vec();
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
        flags: BlockFlags::FULL_CUBE,
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
            2,
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
    assert_eq!(records[1].flags, BlockFlags::FULL_CUBE);
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
    let mut bytes = b"BREG1001".to_vec();
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
