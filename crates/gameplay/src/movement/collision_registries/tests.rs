use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use super::{PhysicsCollisionRegistries, PhysicsCollisionRegistryError};

#[path = "tests/bamboo.rs"]
mod bamboo;

#[path = "tests/custom_shapes.rs"]
mod custom_shapes;

/// Reads the checkout's shared target manifest for registry fixtures.
fn active_content_registry_protocol() -> u32 {
    serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../../../assets/bedrock-target.json"
    ))
    .unwrap()["wire_protocol"]
        .as_u64()
        .unwrap() as u32
}

const BREG_V1001: &[u8] = include_bytes!("../../../../assets/data/block-registry-v1001.bin");
const BREG_V2193: &[u8] = include_bytes!("../../../../assets/data/block-registry-v2193.bin");

/// Minimal byte-valid PREG stamped for one protocol and bound to one BREG
/// digest; shape mirrors the committed movement fixtures so no new
/// artifact is required.
fn synthetic_preg(protocol: u32, breg: &[u8], records: &[assets::RegistryRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PREG1001");
    bytes.extend_from_slice(&protocol.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(records.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(breg));
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.push(u8::try_from(record.collision_seed.boxes.len()).unwrap());
        bytes.push(if record.collision_seed.boxes.is_empty() {
            assets::BlockPhysicsFlags::PASSABLE.bits()
        } else {
            0
        });
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&60_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        for shape in &record.collision_seed.boxes {
            for coordinate in [
                shape.min_x,
                shape.min_y,
                shape.min_z,
                shape.max_x,
                shape.max_y,
                shape.max_z,
            ] {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
    }
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    bytes
}

fn bind(
    breg: &[u8],
    preg: &[u8],
    expected_protocol: u32,
) -> Result<PhysicsCollisionRegistries, PhysicsCollisionRegistryError> {
    PhysicsCollisionRegistries::bind_coherent_assets(
        breg,
        preg,
        Path::new("installed/physics/block-physics.bin"),
        Path::new("installed/assets/world.mcbea"),
        expected_protocol,
    )
}

fn custom_block(name: &str, state_count: u32) -> protocol::CustomBlock {
    protocol::CustomBlock {
        state_physics: Default::default(),
        name: name.into(),
        tags: Default::default(),
        state_count,
        collides: true,
        collision_boxes: None,
        selection: Default::default(),
        visual: Default::default(),
    }
}

#[test]
fn camera_block_tags_follow_both_identity_spaces_and_session_reset() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut block = custom_block("test:aim_target", 1);
    block.tags = Arc::from([Arc::from("test:target")]);
    let hash = block.hashed_states()[0].hash;
    let definitions = protocol::CustomBlocks {
        blocks: Arc::from([block]),
        ..Default::default()
    };
    let (range, _) = registries
        .begin_session_custom_blocks(&definitions)
        .unwrap();
    registries
        .begin_session_hashed_custom_blocks(&definitions)
        .unwrap();
    for (mode, id) in [
        (assets::NetworkIdMode::Sequential, range.start),
        (assets::NetworkIdMode::Hashed, hash),
    ] {
        assert_eq!(
            registries.block_tags(mode, id),
            definitions.blocks[0].tags.as_ref()
        );
    }
    registries
        .begin_session_custom_blocks(&Default::default())
        .unwrap();
    registries
        .begin_session_hashed_custom_blocks(&Default::default())
        .unwrap();
    assert!(
        registries
            .block_tags(assets::NetworkIdMode::Sequential, range.start)
            .is_empty()
    );
    assert!(
        registries
            .block_tags(assets::NetworkIdMode::Hashed, hash)
            .is_empty()
    );
}

/// Non-colliding plants must still be targets for the first punch.
#[test]
fn noncolliding_plants_have_vanilla_pick_shapes_in_both_id_spaces() {
    let protocol = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, protocol).unwrap();
    let registries = bind(
        BREG_V2193,
        &synthetic_preg(protocol, BREG_V2193, &records),
        protocol,
    )
    .unwrap();
    for name in [
        "minecraft:short_grass",
        "minecraft:tall_grass",
        "minecraft:dandelion",
        "minecraft:poppy",
        "minecraft:oak_sapling",
        "minecraft:deadbush",
        "minecraft:torch",
        "minecraft:soul_torch",
        "minecraft:redstone_torch",
        "minecraft:unlit_redstone_torch",
        "minecraft:short_dry_grass",
        "minecraft:brown_mushroom",
        "minecraft:red_mushroom",
        "minecraft:nether_sprouts",
        "minecraft:cactus_flower",
        "minecraft:reeds",
        "minecraft:wheat",
        "minecraft:carrots",
        "minecraft:potatoes",
        "minecraft:beetroot",
        "minecraft:nether_wart",
    ] {
        let matched = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert!(!matched.is_empty(), "{name} must exist in the carrier");
        for record in matched {
            assert!(record.collision_seed.boxes.is_empty(), "{name} is passable");
            for (mode, id) in [
                (assets::NetworkIdMode::Sequential, record.sequential_id),
                (assets::NetworkIdMode::Hashed, record.network_hash),
            ] {
                assert!(
                    !registries
                        .registry(mode)
                        .selection_shapes(id)
                        .unwrap()
                        .is_empty(),
                    "{name} has a visual pick shape despite no movement collision"
                );
                let air_record = records
                    .iter()
                    .find(|record| record.name.as_ref() == "minecraft:air")
                    .unwrap();
                let air = match mode {
                    assets::NetworkIdMode::Sequential => air_record.sequential_id,
                    assets::NetworkIdMode::Hashed => air_record.network_hash,
                };
                let mut store = world::ChunkStore::new();
                // The pick ray inspects a halo for connected/protruding shapes.
                for x in -1..=1 {
                    for z in -1..=1 {
                        store
                            .mark_chunk_loaded(world::ChunkKey::new(0, x, z))
                            .unwrap();
                    }
                }
                let key = world::SubChunkKey::new(0, 0, 0, 0);
                store
                    .update_block(key, world::BlockUpdate::new(0, 0, 2, 0, id), air)
                    .unwrap();
                let shape = registries.registry(mode).selection_shapes(id).unwrap()[0];
                let origin = sim::Vec3::new(
                    (shape.min.x + shape.max.x) * 0.5,
                    (shape.min.y + shape.max.y) * 0.5,
                    0.5,
                );
                let hit = sim::PaletteWorld::new(&store, registries.registry(mode), 0)
                    .block_interaction_ray_current(origin, sim::Vec3::new(0.0, 0.0, 1.0), 3.0)
                    .unwrap()
                    .expect("a first punch can pick the plant");
                assert_eq!((hit.block_pos, hit.runtime_id), ([0, 0, 2], id));
            }
        }
    }
}

/// Remote clients register vanilla definitions only when the server supplies them.
#[test]
fn remote_palette_omits_unsupplied_vanilla_definitions() {
    let records =
        assets::read_registry_for_protocol(BREG_V2193, active_content_registry_protocol()).unwrap();
    let preg = synthetic_preg(active_content_registry_protocol(), BREG_V2193, &records);
    let mut registries = bind(BREG_V2193, &preg, active_content_registry_protocol()).unwrap();
    let (_, remap) = registries
        .begin_session_custom_blocks(&protocol::CustomBlocks::default())
        .unwrap();
    for (wire, name) in [
        (15_844, "minecraft:air"),
        (3_219, "minecraft:stone"),
        (9_650, "minecraft:polished_blackstone_bricks"),
        (19_595, "minecraft:brown_terracotta"),
        (17_951, "minecraft:gray_concrete"),
    ] {
        let resolved = records
            .iter()
            .find(|record| record.sequential_id == remap.to_internal(wire))
            .unwrap();
        assert_eq!(resolved.name.as_ref(), name, "wire block {wire}");
        assert_eq!(remap.to_wire(resolved.sequential_id), wire);
    }
}

#[test]
fn session_custom_blocks_append_after_vanilla_ids() {
    let records =
        assets::read_registry_for_protocol(BREG_V2193, active_content_registry_protocol()).unwrap();
    let preg = synthetic_preg(active_content_registry_protocol(), BREG_V2193, &records);
    let mut registries = bind(BREG_V2193, &preg, active_content_registry_protocol()).unwrap();
    let first = u32::try_from(records.len()).unwrap();
    let vanilla_blocks = assets::server_defined_blocks()
        .iter()
        .map(|definition| Arc::from(definition.name))
        .collect::<Arc<[Arc<str>]>>();
    let appended = protocol::CustomBlocks {
        blocks: vec![
            custom_block("lifeboat:lucky_block_9nnvjzz", 1),
            custom_block("lifeboat:coal_ore_generator_a451ess", 4),
        ]
        .into(),
        vanilla_blocks: Arc::clone(&vanilla_blocks),
        skipped: 0,
    };
    let (range, remap) = registries.begin_session_custom_blocks(&appended).unwrap();
    assert_eq!(range, first..first + 5);
    assert!(remap.is_identity(), "customs after vanilla keep wire ids");
    assert_eq!(
        registries
            .begin_session_custom_blocks(&protocol::CustomBlocks::default())
            .map(|(range, _)| range),
        Some(first..first)
    );
    let interleaved = protocol::CustomBlocks {
        blocks: vec![custom_block("minecraft:stone", 1)].into(),
        vanilla_blocks: Arc::clone(&vanilla_blocks),
        skipped: 0,
    };
    assert_eq!(registries.begin_session_custom_blocks(&interleaved), None);
    // A name sorting among vanilla takes the wire ids from the first vanilla state after
    // it, so every later vanilla wire id is one more than the carrier's.
    let among = protocol::CustomBlocks {
        blocks: vec![custom_block("benergistics:controller", 1)].into(),
        vanilla_blocks,
        skipped: 0,
    };
    let (range, remap) = registries.begin_session_custom_blocks(&among).unwrap();
    assert_eq!(range, first..first + 1);
    let key = (
        protocol::block_name_sort_key("benergistics:controller"),
        "benergistics:controller",
    );
    let state = |name: &str| {
        records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let wire = records
        .iter()
        .filter(|record| {
            (
                protocol::block_name_sort_key(&record.name),
                record.name.as_ref(),
            ) > key
        })
        .map(|record| record.sequential_id)
        .min()
        .unwrap();
    assert_eq!(remap.to_internal(wire), first);
    let (air, dirt) = (state("minecraft:air"), state("minecraft:dirt"));
    assert!(dirt < wire && wire <= air);
    assert_eq!(remap.to_internal(dirt), dirt);
    assert_eq!(remap.to_internal(air + 1), air);
    assert_eq!(remap.to_internal(first), first - 1);
}

#[test]
fn remote_palette_rebuilds_admission_and_custom_order_each_session() {
    let protocol = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, protocol).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(protocol, BREG_V2193, &records),
        protocol,
    )
    .unwrap();
    let first = records.len() as u32;
    let supplied = assets::server_defined_blocks();
    for admitted_count in [0, 1, supplied.len(), 0] {
        let custom = protocol::CustomBlocks {
            blocks: vec![
                custom_block("benergistics:controller", 2),
                custom_block("lifeboat:lucky_block_9nnvjzz", 1),
            ]
            .into(),
            vanilla_blocks: supplied[..admitted_count]
                .iter()
                .map(|block| Arc::from(block.name))
                .collect(),
            skipped: 0,
        };
        let (range, remap) = registries.begin_session_custom_blocks(&custom).unwrap();
        assert_eq!(range, first..first + 3);
        let omitted = supplied[admitted_count..]
            .iter()
            .map(|block| block.state_count)
            .sum::<u32>();
        let wire_count = first + 3 - omitted;
        let actual = (0..wire_count)
            .filter_map(|wire| {
                let internal = remap.to_internal(wire);
                assert_eq!(remap.to_wire(internal), wire);
                registries
                    .block_identifier(assets::NetworkIdMode::Sequential, internal)
                    .filter(|name| *name != super::RESERVED_RECORD_NAME)
                    .map(|name| (protocol::block_name_sort_key(name), name))
            })
            .collect::<Vec<_>>();
        assert!(actual.windows(2).all(|pair| pair[0] <= pair[1]));
        for block in &supplied[admitted_count..] {
            assert_eq!(remap.to_wire(block.first_internal_id), u32::MAX);
        }
    }
}

/// Hashed custom states register under their hashes and are dropped by the next session.
#[test]
fn session_hashed_custom_blocks_register_and_reset() {
    let records =
        assets::read_registry_for_protocol(BREG_V2193, active_content_registry_protocol()).unwrap();
    let preg = synthetic_preg(active_content_registry_protocol(), BREG_V2193, &records);
    let mut registries = bind(BREG_V2193, &preg, active_content_registry_protocol()).unwrap();
    let custom = protocol::CustomBlocks {
        blocks: vec![custom_block("test:hashed", 1)].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let hash = custom.blocks[0].hashed_states()[0].hash;
    assert_eq!(
        registries.begin_session_hashed_custom_blocks(&custom),
        Some(1)
    );
    assert!(registries.hashed.contains_runtime_id(hash));
    assert_eq!(
        registries.begin_session_hashed_custom_blocks(&protocol::CustomBlocks::default()),
        Some(0)
    );
    assert!(!registries.hashed.contains_runtime_id(hash));
}

/// The live LBSG aliasing mechanism: a byte-valid PREG whose stamped
/// header protocol disagrees with the active authority must fail closed
/// through the dedicated typed error naming both protocols and both
/// artifact paths. Before this gate existed the same input surfaced only
/// the generic legacy detail string ("protocol is not 1001") with no path
/// attribution.
#[test]
fn valid_wrong_protocol_preg_fails_with_typed_cross_carrier_mismatch() {
    let legacy_records = assets::read_registry(BREG_V1001).unwrap();
    let preg_v1001 = synthetic_preg(1001, BREG_V1001, &legacy_records);
    let error = bind(BREG_V2193, &preg_v1001, active_content_registry_protocol())
        .expect_err("a flipped physics registry must fail startup");

    let PhysicsCollisionRegistryError::ProtocolMismatch {
        expected_protocol,
        actual_protocol,
        physics_registry_path,
        world_carrier_path,
    } = &error
    else {
        panic!("expected ProtocolMismatch, got {error:?}");
    };
    assert_eq!(*expected_protocol, 2193);
    assert_eq!(*actual_protocol, 1001);
    assert_eq!(
        physics_registry_path,
        &PathBuf::from("installed/physics/block-physics.bin")
    );
    assert_eq!(
        world_carrier_path,
        &PathBuf::from("installed/assets/world.mcbea")
    );
    let message = format!("{error}");
    assert!(
        message.contains("1001") && message.contains("2193"),
        "{message}"
    );
    assert!(message.contains("block-physics.bin"), "{message}");
    assert!(message.contains("world.mcbea"), "{message}");
}

/// Accepted production path: both carriers carry the active protocol.
#[test]
fn coherent_active_protocol_pair_binds_completely() {
    let registries = bind(
        BREG_V2193,
        &synthetic_preg(
            2193,
            BREG_V2193,
            &assets::read_registry_for_protocol(BREG_V2193, 2193).expect("checked-in v2193 BREG"),
        ),
        active_content_registry_protocol(),
    )
    .expect("the pinned protocol pair must bind");

    assert!(registries.is_complete());
    assert!(registries.available_record_count() > 0);
}

/// Consolidation witness: driving the seam with a mutated authority value
/// flips its acceptance decision on identical bytes, so both gates'
/// expectations provably hang off the one shared knob instead of local
/// constants.
#[test]
fn mutating_the_authority_flips_the_binding_decision() {
    let accepted = bind(
        BREG_V1001,
        &synthetic_preg(
            1001,
            BREG_V1001,
            &assets::read_registry_for_protocol(BREG_V1001, 1001).expect("checked-in v1001 BREG"),
        ),
        1001,
    )
    .is_ok();
    let rejected = bind(
        BREG_V1001,
        &synthetic_preg(
            1001,
            BREG_V1001,
            &assets::read_registry_for_protocol(BREG_V1001, 1001).expect("checked-in v1001 BREG"),
        ),
        2193,
    )
    .is_err();

    assert!(accepted && rejected);
}

#[test]
fn held_placement_intention_uses_the_held_block_family_in_both_id_spaces() {
    let protocol = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, protocol).unwrap();
    let preg = synthetic_preg(protocol, BREG_V2193, &records);
    let registry = bind(BREG_V2193, &preg, protocol).unwrap();
    for (name, expected) in [
        ("minecraft:stone", true),
        ("minecraft:oak_leaves", true),
        ("minecraft:oak_stairs", true),
        ("minecraft:oak_slab", true),
        ("minecraft:glass_pane", true),
        ("minecraft:oak_fence", true),
        ("minecraft:cobblestone_wall", true),
        ("minecraft:white_carpet", true),
        ("minecraft:soul_sand", true),
        ("minecraft:mud", true),
        ("minecraft:barrier", true),
        ("minecraft:chiseled_bookshelf", true),
        ("minecraft:fence_gate", false),
        ("minecraft:wooden_door", false),
        ("minecraft:trapdoor", false),
        ("minecraft:chest", false),
        ("minecraft:air", false),
    ] {
        let record = records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap_or_else(|| panic!("missing registered held block {name}"));
        assert_eq!(
            registry
                .block_has_build_intention(assets::NetworkIdMode::Sequential, record.sequential_id),
            expected,
            "{name}"
        );
        assert_eq!(
            registry.block_has_build_intention(assets::NetworkIdMode::Hashed, record.network_hash),
            expected,
            "{name}"
        );
    }
    assert!(!registry.block_has_build_intention(assets::NetworkIdMode::Sequential, u32::MAX));
}
