use std::{fs, path::Path};

use assets::{
    AssetError, BlockFace, BlockFlags, CollisionConfidence, ContributorRole, MAX_FLIPBOOK_FRAMES,
    MAX_FLIPBOOKS, ModelFamily, ModelState, ModelStateField, RegistryProvenance, RegistryRecord,
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

#[test]
fn exact_side_caps_and_static_terrain_accessors_fail_closed() {
    let valid = tempfile::tempdir().expect("valid cactus pack");
    write_pack(
        valid.path(),
        r#"{
            "format_version":[1,1,0],
            "cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top"}}
        }"#,
        r#"{"texture_data":{
            "cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},
            "cactus_side":{"textures":"textures/blocks/cactus_side"},
            "cactus_top":{"textures":"textures/blocks/cactus_top"}
        }}"#,
        EMPTY_FLIPBOOKS,
    );
    let pack = read_pack(valid.path()).expect("read valid cactus pack");
    assert_eq!(
        pack.blocks.get_exact_side_caps("cactus"),
        Some(["cactus_side", "cactus_bottom", "cactus_top"])
    );
    assert_eq!(
        pack.terrain.get_exact_static_no_tint("cactus_bottom"),
        Some("textures/blocks/cactus_bottom")
    );

    for (name, route) in [
        ("scalar", r#""cactus_side""#),
        (
            "explicit horizontal",
            r#"{"down":"cactus_bottom","up":"cactus_top","side":"cactus_side","west":"cactus_side"}"#,
        ),
        (
            "missing side",
            r#"{"down":"cactus_bottom","up":"cactus_top"}"#,
        ),
        (
            "missing down",
            r#"{"side":"cactus_side","up":"cactus_top"}"#,
        ),
        (
            "missing up",
            r#"{"down":"cactus_bottom","side":"cactus_side"}"#,
        ),
        (
            "unknown typo key",
            r#"{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top","sied":"cactus_side"}"#,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid cactus pack");
        write_pack(
            directory.path(),
            &format!(r#"{{"format_version":[1,1,0],"cactus":{{"textures":{route}}}}}"#),
            r#"{"texture_data":{"cactus_side":{"textures":"textures/blocks/cactus_side"},"cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},"cactus_top":{"textures":"textures/blocks/cactus_top"}}}"#,
            EMPTY_FLIPBOOKS,
        );
        let pack = read_pack(directory.path())
            .unwrap_or_else(|error| panic!("read {name} cactus fixture: {error}"));
        assert_eq!(pack.blocks.get_exact_side_caps("cactus"), None, "{name}");
    }

    for (name, terrain_value) in [
        (
            "array",
            r#"["textures/blocks/cactus_side","textures/blocks/cactus_side_2"]"#,
        ),
        (
            "tinted",
            r##"{"path":"textures/blocks/cactus_side","overlay_color":"#00ff00"}"##,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid cactus terrain");
        write_pack(
            directory.path(),
            r#"{"format_version":[1,1,0],"cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top"}}}"#,
            &format!(
                r#"{{"texture_data":{{"cactus_side":{{"textures":{terrain_value}}},"cactus_bottom":{{"textures":"textures/blocks/cactus_bottom"}},"cactus_top":{{"textures":"textures/blocks/cactus_top"}}}}}}"#
            ),
            EMPTY_FLIPBOOKS,
        );
        let pack = read_pack(directory.path())
            .unwrap_or_else(|error| panic!("read {name} terrain fixture: {error}"));
        assert_eq!(
            pack.terrain.get_exact_static_no_tint("cactus_side"),
            None,
            "{name}"
        );
    }
}

#[test]
fn exact_cake_faces_and_untinted_pairs_fail_closed() {
    const TERRAIN: &str = r#"{"texture_data":{
        "cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},
        "cake_side":{"textures":["textures/blocks/cake_side","textures/blocks/cake_side"]},
        "cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},
        "cake_west":{"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]}
    }}"#;
    let valid = tempfile::tempdir().expect("valid cake pack");
    write_pack(
        valid.path(),
        r#"{"format_version":[1,1,0],"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}"#,
        TERRAIN,
        EMPTY_FLIPBOOKS,
    );
    let pack = read_pack(valid.path()).expect("read valid cake pack");
    assert_eq!(
        pack.blocks.get_exact_cake_faces(),
        Some([
            "cake_west",
            "cake_side",
            "cake_bottom",
            "cake_top",
            "cake_side",
            "cake_side"
        ])
    );
    assert_eq!(
        pack.terrain.get_exact_pair_no_tint("cake_west"),
        Some(["textures/blocks/cake_side", "textures/blocks/cake_inner"])
    );

    for (name, route) in [
        ("scalar", r#""cake_side""#),
        (
            "side fallback",
            r#"{"down":"cake_bottom","side":"cake_side","up":"cake_top","west":"cake_west"}"#,
        ),
        (
            "missing face",
            r#"{"down":"cake_bottom","east":"cake_side","north":"cake_side","up":"cake_top","west":"cake_west"}"#,
        ),
        (
            "wrong route",
            r#"{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_side"}"#,
        ),
        (
            "unknown key",
            r#"{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west","sied":"cake_side"}"#,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid cake pack");
        write_pack(
            directory.path(),
            &format!(r#"{{"cake":{{"textures":{route}}}}}"#),
            TERRAIN,
            EMPTY_FLIPBOOKS,
        );
        let pack = read_pack(directory.path())
            .unwrap_or_else(|error| panic!("read {name} cake fixture: {error}"));
        assert_eq!(pack.blocks.get_exact_cake_faces(), None, "{name}");
    }

    for (name, value) in [
        ("static", r#""textures/blocks/cake_side""#),
        ("singleton", r#"["textures/blocks/cake_side"]"#),
        (
            "three",
            r#"["textures/blocks/cake_side","textures/blocks/cake_inner","textures/blocks/cake_inner"]"#,
        ),
        ("empty", r#"["","textures/blocks/cake_inner"]"#),
        (
            "tinted",
            r##"[{"path":"textures/blocks/cake_side","overlay_color":"#ffffff"},"textures/blocks/cake_inner"]"##,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid cake terrain");
        write_pack(
            directory.path(),
            r#"{"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}"#,
            &format!(r#"{{"texture_data":{{"cake_west":{{"textures":{value}}}}}}}"#),
            EMPTY_FLIPBOOKS,
        );
        if let Ok(pack) = read_pack(directory.path()) {
            assert_eq!(
                pack.terrain.get_exact_pair_no_tint("cake_west"),
                None,
                "{name}"
            );
        }
    }
}

#[test]
fn exact_farmland_routes_and_inverse_moisture_selector_fail_closed() {
    let valid = tempfile::tempdir().expect("valid farmland pack");
    write_pack(
        valid.path(),
        r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
        r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}}}"#,
        EMPTY_FLIPBOOKS,
    );
    let pack = read_pack(valid.path()).expect("read valid farmland pack");
    assert_eq!(
        pack.blocks.get_exact_side_caps("farmland"),
        Some(["farmland_side", "farmland_side", "farmland"])
    );
    assert_eq!(
        pack.terrain.get_exact_static_no_tint("farmland_side"),
        Some("textures/blocks/dirt")
    );
    assert_eq!(
        pack.terrain.get_exact_farmland_side(),
        Some("textures/blocks/dirt")
    );
    assert_eq!(
        pack.terrain.get_exact_farmland_top(0),
        Some(("textures/blocks/farmland_dry", 1))
    );
    for amount in 1..=7 {
        assert_eq!(
            pack.terrain.get_exact_farmland_top(amount),
            Some(("textures/blocks/farmland_wet", 0)),
            "amount {amount}"
        );
    }
    assert_eq!(pack.terrain.get_exact_farmland_top(8), None);

    for (label, route) in [
        ("scalar", r#""farmland_side""#),
        (
            "override",
            r#"{"down":"farmland_side","side":"farmland_side","up":"farmland","north":"farmland_side"}"#,
        ),
        (
            "missing side",
            r#"{"down":"farmland_side","up":"farmland"}"#,
        ),
        (
            "wrong top key",
            r#"{"down":"farmland_side","side":"farmland_side","up":"farmland_wet"}"#,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid farmland route");
        write_pack(
            directory.path(),
            &format!(r#"{{"farmland":{{"textures":{route}}}}}"#),
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}}}"#,
            EMPTY_FLIPBOOKS,
        );
        if let Ok(pack) = read_pack(directory.path()) {
            assert_ne!(
                pack.blocks.get_exact_side_caps("farmland"),
                Some(["farmland_side", "farmland_side", "farmland"]),
                "{label}"
            );
        }
    }

    for (label, value) in [
        ("static", r#""textures/blocks/farmland_wet""#),
        ("singleton", r#"["textures/blocks/farmland_wet"]"#),
        (
            "three",
            r#"["textures/blocks/farmland_wet","textures/blocks/farmland_dry","textures/blocks/farmland_wet"]"#,
        ),
        (
            "wrong order",
            r#"["textures/blocks/farmland_dry","textures/blocks/farmland_wet"]"#,
        ),
        ("empty", r#"["","textures/blocks/farmland_dry"]"#),
        (
            "tinted",
            r##"[{"path":"textures/blocks/farmland_wet","overlay_color":"#ffffff"},"textures/blocks/farmland_dry"]"##,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid farmland terrain");
        write_pack(
            directory.path(),
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            &format!(
                r#"{{"texture_data":{{"farmland_side":{{"textures":"textures/blocks/dirt"}},"farmland":{{"textures":{value}}}}}}}"#
            ),
            EMPTY_FLIPBOOKS,
        );
        if let Ok(pack) = read_pack(directory.path()) {
            assert_eq!(pack.terrain.get_exact_farmland_top(0), None, "{label}");
        }
    }

    for (label, terrain) in [
        (
            "top carried metadata",
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"],"carried_textures":"textures/blocks/farmland_dry"}}}"#,
        ),
        (
            "variant alias metadata",
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":[{"path":"textures/blocks/farmland_wet","alias":"wet"},"textures/blocks/farmland_dry"]}}}"#,
        ),
        (
            "side carried metadata",
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt","carried_textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}}}"#,
        ),
    ] {
        let directory = tempfile::tempdir().expect("farmland metadata adversary");
        write_pack(
            directory.path(),
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            terrain,
            EMPTY_FLIPBOOKS,
        );
        let pack = read_pack(directory.path())
            .unwrap_or_else(|error| panic!("read {label} farmland pack: {error}"));
        assert!(
            pack.terrain.get_exact_farmland_top(0).is_none()
                || pack.terrain.get_exact_farmland_side().is_none(),
            "{label}"
        );
    }
}

fn registry_bytes(records: &[RegistryFixture<'_>]) -> Vec<u8> {
    let mut bytes = b"BREG1003".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for &(sequential_id, network_hash, flags, name, state) in records {
        bytes.extend_from_slice(&sequential_id.to_le_bytes());
        bytes.extend_from_slice(&network_hash.to_le_bytes());
        bytes.push(flags);
        bytes.push(if flags & 1 != 0 { 1 } else { 0 });
        bytes.push(if flags & 1 != 0 { 2 } else { 0 });
        bytes.push(0);
        bytes.push(if flags & 4 != 0 { 0x3f } else { 0 });
        bytes.push(0);
        bytes.push(1 << 1);
        bytes.push(0);
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&(state.len() as u32).to_le_bytes());
        for _ in 0..8 {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
        }
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
        model_family: ModelFamily::Cube,
        contributor_role: ContributorRole::Primary,
        model_state: ModelState::default(),
        face_coverage: 0x3f,
        collision_seed: Default::default(),
        provenance: RegistryProvenance::DRAGONFLY,
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
    assert_eq!(records[0].model_family, ModelFamily::Air);
    assert_eq!(records[0].contributor_role, ContributorRole::Air);
    assert_eq!(records[0].face_coverage, 0);
    assert_eq!(
        records[0].collision_seed.confidence,
        CollisionConfidence::None
    );
    assert_eq!(records[0].provenance, RegistryProvenance::DRAGONFLY);
    assert_eq!(
        records[1].flags,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
    );
}

#[test]
fn registry_reader_exposes_typed_model_state() {
    let mut bytes = registry_bytes(&[(
        7,
        11,
        0,
        b"minecraft:wheat",
        br#"{"growth":{"type":"int","value":7}}"#,
    )]);
    let record_start = 8 + 7 * 4;
    bytes[record_start + 11] = 1 << ((ModelStateField::Growth as u8) - 1);
    let values_start = record_start + 24;
    bytes[values_start + 5 * 4..values_start + 6 * 4].copy_from_slice(&7_u32.to_le_bytes());
    let records = read_registry(&bytes).expect("typed BREG1003 registry");
    assert_eq!(records[0].model_state.get(ModelStateField::Growth), Some(7));
}

#[test]
fn registry_reader_decodes_fixed_collision_seed() {
    let mut bytes = registry_bytes(&[(9, 12, 0, b"minecraft:test", b"{}")]);
    let record_start = 8 + 7 * 4;
    bytes[record_start + 13] = CollisionConfidence::CollisionOnly as u8;
    bytes[record_start + 15] = 1;
    bytes[record_start + 16..record_start + 18].copy_from_slice(&7_u16.to_le_bytes());
    let coordinates = [
        -6_250_000_i32,
        0,
        2_500_000,
        100_000_000,
        95_000_005,
        90_000_000,
    ];
    let mut encoded_box = Vec::with_capacity(24);
    for coordinate in coordinates {
        encoded_box.extend_from_slice(&coordinate.to_le_bytes());
    }
    bytes.splice(record_start + 56..record_start + 56, encoded_box);

    let records = read_registry(&bytes).expect("collision BREG1003 registry");
    assert_eq!(records[0].collision_seed.shape_id, 7);
    assert_eq!(
        records[0].collision_seed.confidence,
        CollisionConfidence::CollisionOnly
    );
    assert_eq!(records[0].collision_seed.boxes.len(), 1);
    assert_eq!(records[0].collision_seed.boxes[0].min_x, -6_250_000);
    assert_eq!(records[0].collision_seed.boxes[0].max_y, 95_000_005);
}

#[test]
fn registry_reader_rejects_unknown_breg1003_enums() {
    let mut bytes = registry_bytes(&[(9, 12, 0, b"minecraft:test", b"{}")]);
    bytes[8 + 7 * 4 + 9] = 0xff;
    assert!(matches!(
        read_registry(&bytes),
        Err(AssetError::InvalidRegistryFlags(0xff))
    ));
}

#[test]
fn registry_reader_rejects_false_valentine_name_overlap_metadata() {
    let first_name = b"minecraft:first";
    let second_name = b"minecraft:second";
    let mut bytes = registry_bytes(&[
        (1, 11, 0, first_name, b"{}"),
        (2, 12, 0, second_name, b"{}"),
    ]);
    bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&2_u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&1_u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&0_u32.to_le_bytes());
    let first_start = 36;
    let second_start = first_start + 56 + first_name.len() + 2;
    bytes[first_start + 14] = 0x0f;
    bytes[second_start + 14] = 0x0f;
    assert!(matches!(
        read_registry(&bytes),
        Err(AssetError::InvalidRegistryFlags(0xff))
    ));
}

#[test]
fn registry_reader_decodes_checked_in_full_source_bijection() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/block-registry-v1001.bin");
    let bytes = fs::read(path).expect("read checked-in BREG1003");
    let records = read_registry(&bytes).expect("decode checked-in BREG1003");
    assert_eq!(records.len(), 16_913);
    let names = records
        .iter()
        .map(|record| &record.name)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(names.len(), 1_356);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.provenance.contains(RegistryProvenance::VALENTINE))
            .count(),
        15_845
    );
    let canonical =
        RegistryProvenance::PMMP | RegistryProvenance::DRAGONFLY | RegistryProvenance::PRISMARINE;
    assert!(
        records
            .iter()
            .all(|record| record.provenance.contains(canonical))
    );

    let family = |name: &str| {
        records
            .iter()
            .find(|record| &*record.name == name)
            .unwrap_or_else(|| panic!("missing {name}"))
            .model_family
    };
    assert_eq!(family("minecraft:short_grass"), ModelFamily::Cross);
    assert_eq!(family("minecraft:wheat"), ModelFamily::Crop);
    assert_eq!(family("minecraft:water"), ModelFamily::Liquid);
    assert_eq!(family("minecraft:oak_stairs"), ModelFamily::Stair);
    assert_eq!(family("minecraft:cobblestone_wall"), ModelFamily::Wall);
    assert_eq!(family("minecraft:iron_bars"), ModelFamily::Pane);
    assert_eq!(family("minecraft:seagrass"), ModelFamily::Aquatic);
    assert_eq!(family("minecraft:cocoa"), ModelFamily::Cocoa);
    assert_eq!(family("minecraft:vine"), ModelFamily::Vine);
    assert_eq!(family("minecraft:glow_lichen"), ModelFamily::GlowLichen);
    assert_eq!(family("minecraft:sculk_vein"), ModelFamily::SculkVein);
    assert_eq!(
        family("minecraft:chiseled_bookshelf"),
        ModelFamily::ChiseledBookshelf
    );
    assert_ne!(family("minecraft:chorus_flower"), ModelFamily::Cross);
    assert_eq!(family("minecraft:soul_sand"), ModelFamily::Cuboid);
    assert_eq!(family("minecraft:barrier"), ModelFamily::Invisible);
    let soul_sand = records
        .iter()
        .find(|record| &*record.name == "minecraft:soul_sand")
        .unwrap();
    assert!(!soul_sand.flags.contains(BlockFlags::CUBE_GEOMETRY));
    assert_eq!(soul_sand.face_coverage, 0);
}

#[test]
fn checked_in_registry_has_one_canonical_air_network_identity() {
    let records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode checked-in BREG1003");
    let air = records
        .iter()
        .filter(|record| {
            record.flags.contains(BlockFlags::AIR)
                && record.contributor_role == ContributorRole::Air
                && record.model_family == ModelFamily::Air
        })
        .collect::<Vec<_>>();

    assert_eq!(air.len(), 1);
    assert_eq!(&*air[0].name, "minecraft:air");
    assert_eq!(air[0].sequential_id, 13_094);
    assert_eq!(air[0].network_hash, 0xdbf4_4120);
}

#[test]
fn registry_reader_decodes_all_pressure_plate_pressed_selectors() {
    const PRESSED: u32 = 1 << 1;
    let bytes = include_bytes!("../data/block-registry-v1001.bin");
    let records = read_registry(bytes).expect("decode checked-in BREG1003");
    let plates = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::PressurePlate)
        .collect::<Vec<_>>();
    assert_eq!(plates.len(), 256);
    let mut counts = std::collections::BTreeMap::<&str, [usize; 2]>::new();
    for record in plates {
        assert_eq!(record.model_state.mask(), 1 << 7, "{}", record.name);
        let state: serde_json::Value =
            serde_json::from_str(&record.canonical_state).expect("canonical pressure-plate state");
        let signal = state["redstone_signal"]["value"]
            .as_u64()
            .expect("integer redstone_signal");
        assert!(signal <= 15);
        let expected = if signal == 0 { 0 } else { PRESSED };
        assert_eq!(
            record.model_state.get(ModelStateField::Flags),
            Some(expected),
            "{}",
            record.canonical_state
        );
        counts.entry(record.name.as_ref()).or_default()[usize::from(signal != 0)] += 1;
    }
    assert_eq!(counts.len(), 16);
    assert!(counts.values().all(|count| *count == [1, 15]));
}

#[test]
fn block_flag_semantics_accept_only_independent_valid_combinations() {
    for valid in [
        BlockFlags::empty(),
        BlockFlags::AIR,
        BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::CUBE_GEOMETRY,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
    ] {
        assert!(valid.has_valid_semantics(), "rejected {valid:?}");
    }

    for invalid in [
        BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY,
        BlockFlags::AIR | BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::LEAF_MODEL,
        BlockFlags::LEAF_MODEL | BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL,
    ] {
        assert!(!invalid.has_valid_semantics(), "accepted {invalid:?}");
    }
}

#[test]
fn registry_reader_rejects_unknown_and_invalid_semantic_flags() {
    let accepted = registry_bytes(&[(3, 11, 0x04, b"minecraft:test", b"{}")]);
    assert_eq!(
        read_registry(&accepted).expect("standalone full-face occluder")[0].flags,
        BlockFlags::OCCLUDES_FULL_FACE
    );

    for raw in [0x10, 0x03, 0x05, 0x08, 0x0c, 0x0e] {
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
    bytes[..8].copy_from_slice(b"BREG1002");
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
    let mut bytes = b"BREG1003".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&65_537_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
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
