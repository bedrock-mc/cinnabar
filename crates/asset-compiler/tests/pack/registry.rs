use super::support::*;

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
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/data/block-registry-v1001.bin");
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
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
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
    let bytes = include_bytes!("../../../assets/data/block-registry-v1001.bin");
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
