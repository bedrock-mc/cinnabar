use assets::{
    BlobProvenance, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, Material,
    ModelQuad, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, NetworkIdMode, TextureArray,
    TextureMip, TexturePage, TextureRef, VisualKind, encode_blob,
};

use super::{BackingBlockIdentity, BlockEntityVisualRoute, adjudicate_block_entity_visual};

/// Complete synthetic identity for blobs that are decoded directly and never
/// startup-validated. These bytes never match a real pinned expectation.
const FIXTURE_PROVENANCE: BlobProvenance = BlobProvenance {
    source_manifest_sha256: [0xA5; 32],
    block_registry_sha256: [0x5A; 32],
    light_registry_sha256: [0x33; 32],
    biome_registry_sha256: [0x3C; 32],
};

fn block_entity_nbt(id: Option<&str>, fields: &[(u8, &str, u8)]) -> world::BlockEntityNbt {
    let mut bytes = vec![10, 0];
    if let Some(id) = id {
        bytes.extend([8, 2, b'i', b'd', id.len() as u8]);
        bytes.extend_from_slice(id.as_bytes());
    }
    for &(tag, name, value) in fields {
        bytes.extend([tag, name.len() as u8]);
        bytes.extend_from_slice(name.as_bytes());
        match tag {
            1 => bytes.push(value),
            3 => bytes.push(value << 1),
            _ => panic!("unsupported test tag"),
        }
    }
    bytes.push(0);
    world::BlockEntityNbt::decode_prefix(&bytes).unwrap().0
}

fn backing(sequential_id: u32, network_hash: u32) -> BackingBlockIdentity {
    let kind = if sequential_id == 846
        || matches!(
            sequential_id,
            1_936
                | 2_699..=2_702
                | 7_069..=7_080
                | 8_516
                | 13_947..=13_950
                | 14_587..=14_590
                | 15_143..=15_146
                | 15_321..=15_324
                | 15_688..=15_691
        ) {
        VisualKind::Cube
    } else if matches!(
        sequential_id,
        13..=28
            | 837..=842
            | 1_992..=1_997
            | 2_018..=2_023
            | 5_393..=5_398
            | 6_438..=6_453
            | 6_883..=6_888
            | 8_510..=8_515
            | 9_120..=9_125
            | 9_209..=9_214
            | 10_237..=10_252
            | 11_064..=11_079
            | 12_171..=12_186
            | 12_620..=12_635
            | 13_126..=13_141
            | 13_336..=13_347
            | 13_849..=13_864
            | 14_513..=14_528
            | 14_533..=14_548
            | 14_691..=14_722
            | 14_941..=14_946
            | 15_347..=15_352
    ) {
        VisualKind::Model
    } else {
        VisualKind::Diagnostic
    };
    let runtime = runtime_with_block_identity(sequential_id, network_hash, kind);
    BackingBlockIdentity::from_runtime(network_hash, NetworkIdMode::Hashed, &runtime)
}

fn runtime_with_block_identity(
    sequential_id: u32,
    network_hash: u32,
    kind: VisualKind,
) -> assets::RuntimeAssets {
    let flags = if kind == VisualKind::Cube {
        BlockFlags::CUBE_GEOMETRY
    } else {
        BlockFlags::empty()
    };
    let visual = BlockVisual {
        support: if kind == VisualKind::Diagnostic {
            assets::VisualSupport::Diagnostic
        } else {
            assets::VisualSupport::Exact
        },
        faces: [0; 6],
        flags,
        kind,
        contributor_role: assets::ContributorRole::Primary,
        model_template: if kind == VisualKind::Model {
            0
        } else {
            NO_MODEL_TEMPLATE
        },
        animation: NO_ANIMATION,
        variant: 0,
    };
    let mips = [16_u32, 8, 4, 2, 1]
        .map(|size| TextureMip {
            size,
            rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
        })
        .into();
    let count = sequential_id as usize + 1;
    let blob = encode_blob(&CompiledAssets {
        visuals: vec![visual; count].into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); count].into_boxed_slice(),
        hashed: vec![(network_hash, sequential_id)].into_boxed_slice(),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
        }]
        .into_boxed_slice(),
        model_templates: if kind == VisualKind::Model {
            vec![ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: 0,
            }]
            .into_boxed_slice()
        } else {
            Box::new([])
        },
        model_quads: if kind == VisualKind::Model {
            vec![ModelQuad {
                positions: [[0; 3]; 4],
                uvs: [[0; 2]; 4],
                material: 0,
                flags: 0,
            }]
            .into_boxed_slice()
        } else {
            Box::new([])
        },
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: FIXTURE_PROVENANCE,
    })
    .unwrap();
    assets::RuntimeAssets::decode(&blob).unwrap()
}

#[test]
fn block_entity_visuals_classify_static_logical_deferred_and_unknown_routes() {
    let static_routes = [
        ("Barrel", 7069, 198_111_737),
        ("BlastFurnace", 15_143, 2_142_573_020),
        ("BlastFurnace", 13_947, 1_464_259_042),
        ("Furnace", 15_688, 3_463_497_305),
        ("Furnace", 14_587, 2_568_407_871),
        ("Smoker", 2_699, 3_435_179_109),
        ("Smoker", 15_321, 2_080_399_355),
    ];
    for (id, sequential_id, network_hash) in static_routes {
        assert!(matches!(
            adjudicate_block_entity_visual(
                &block_entity_nbt(Some(id), &[]),
                backing(sequential_id, network_hash),
            ),
            BlockEntityVisualRoute::ExistingBlockState {
                additional_refs: 0,
                ..
            }
        ));
    }

    assert!(matches!(
        adjudicate_block_entity_visual(
            &block_entity_nbt(Some("Jukebox"), &[]),
            backing(8_516, 1_605_519_270),
        ),
        BlockEntityVisualRoute::LogicalNoAdditionalDraw {
            additional_refs: 0,
            ..
        }
    ));
    assert!(matches!(
        adjudicate_block_entity_visual(
            &block_entity_nbt(None, &[(1, "note", 24), (1, "powered", 1)]),
            backing(1_936, 166_024_317),
        ),
        BlockEntityVisualRoute::LogicalNoAdditionalDraw {
            additional_refs: 0,
            ..
        }
    ));

    for (id, sequential_id, network_hash) in [
        ("Banner", 10_321, 909_112_222),
        ("Beacon", 846, 561_914_719),
        ("Bed", 13_095, 313_457_523),
        ("BrewingStand", 15_128, 1_742_101_107),
        ("Campfire", 10_421, 2_722_749_277),
        ("Chest", 14_039, 741_882_976),
        ("CopperGolemStatue", 2_648, 2_018_082_108),
        ("DecoratedPot", 13_157, 340_115_056),
        ("EnchantTable", 13_163, 1_230_080_101),
        ("EnderChest", 6_870, 1_106_211_301),
        ("GlowItemFrame", 1_047, 209_894_407),
        ("Hopper", 13_514, 3_036_911_681),
        ("ItemFrame", 6_477, 2_475_575_814),
        ("Lectern", 13_559, 3_354_404_216),
        ("Sign", 13, 1_222_845_996),
        ("Skull", 33, 235_396_048),
    ] {
        let source = block_entity_nbt(Some(id), &[(3, "note", 25), (1, "powered", 7)]);
        assert!(matches!(
            adjudicate_block_entity_visual(&source, backing(sequential_id, network_hash)),
            BlockEntityVisualRoute::Deferred { .. }
        ));
        assert!(matches!(
            adjudicate_block_entity_visual(&source, backing(1_936, 166_024_317)),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }

    for (source, identity) in [
        (block_entity_nbt(Some("Barrel"), &[]), backing(42, 84)),
        (
            block_entity_nbt(Some("Jukebox"), &[]),
            backing(1_936, 166_024_317),
        ),
        (
            block_entity_nbt(Some("NotAReviewedSource"), &[]),
            backing(7_069, 198_111_737),
        ),
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(&source, identity),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
}

#[test]
fn block_entity_visuals_deferred_routes_require_exact_backing_identity() {
    let beacon = block_entity_nbt(Some("Beacon"), &[]);
    let runtime = runtime_with_block_identity(846, 561_914_719, VisualKind::Cube);
    for identity in [
        BackingBlockIdentity::from_runtime(0, NetworkIdMode::Hashed, &runtime),
        BackingBlockIdentity::from_runtime(
            561_914_719,
            NetworkIdMode::Hashed,
            &runtime_with_block_identity(846, 561_914_719, VisualKind::Invisible),
        ),
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(&beacon, identity),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
}

#[test]
fn block_entity_visuals_fail_closed_at_every_note_identity_boundary() {
    let valid_backing = backing(1_936, 166_024_317);
    for fields in [
        vec![],
        vec![(1, "note", 0)],
        vec![(1, "powered", 0)],
        vec![(1, "note", 25), (1, "powered", 0)],
        vec![(1, "note", 24), (1, "powered", 2)],
        vec![(3, "note", 24), (1, "powered", 1)],
        vec![(1, "note", 24), (3, "powered", 1)],
        vec![(1, "note", 24), (1, "note", 24), (1, "powered", 1)],
        vec![(1, "note", 24), (1, "powered", 1), (1, "powered", 1)],
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(&block_entity_nbt(None, &fields), valid_backing),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
    assert!(matches!(
        adjudicate_block_entity_visual(
            &block_entity_nbt(None, &[(1, "note", 0), (1, "powered", 0)]),
            backing(42, 84),
        ),
        BlockEntityVisualRoute::Unknown { .. }
    ));
}

#[test]
fn block_entity_visuals_route_digest_is_stable_and_identity_bound() {
    let first = adjudicate_block_entity_visual(
        &block_entity_nbt(Some("Barrel"), &[]),
        backing(7_069, 198_111_737),
    );
    let repeated = adjudicate_block_entity_visual(
        &block_entity_nbt(Some("Barrel"), &[(1, "irrelevant", 9)]),
        backing(7_069, 198_111_737),
    );
    let other_state = adjudicate_block_entity_visual(
        &block_entity_nbt(Some("Barrel"), &[]),
        backing(7_070, 501_043_176),
    );
    assert_eq!(first.route_digest(), repeated.route_digest());
    assert_ne!(first.route_digest(), other_state.route_digest());

    let chest = block_entity_nbt(Some("Chest"), &[]);
    let runtime = runtime_with_block_identity(14_039, 741_882_976, VisualKind::Diagnostic);
    let sequential = adjudicate_block_entity_visual(
        &chest,
        BackingBlockIdentity::from_runtime(14_039, NetworkIdMode::Sequential, &runtime),
    );
    let hashed = adjudicate_block_entity_visual(
        &chest,
        BackingBlockIdentity::from_runtime(741_882_976, NetworkIdMode::Hashed, &runtime),
    );
    assert_eq!(sequential.route_digest(), hashed.route_digest());
}

#[test]
fn block_entity_visuals_route_digest_distinguishes_unresolved_hashed_states() {
    let source = block_entity_nbt(Some("Barrel"), &[]);
    let runtime = runtime_with_block_identity(7_069, 198_111_737, VisualKind::Cube);
    let first = adjudicate_block_entity_visual(
        &source,
        BackingBlockIdentity::from_runtime(1, NetworkIdMode::Hashed, &runtime),
    );
    let second = adjudicate_block_entity_visual(
        &source,
        BackingBlockIdentity::from_runtime(2, NetworkIdMode::Hashed, &runtime),
    );
    assert!(matches!(first, BlockEntityVisualRoute::Unknown { .. }));
    assert!(matches!(second, BlockEntityVisualRoute::Unknown { .. }));
    assert_ne!(first.route_digest(), second.route_digest());
}
