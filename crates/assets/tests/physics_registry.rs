#[path = "../src/error.rs"]
mod error;
pub use error::AssetError;
#[path = "../src/registry.rs"]
mod registry;
pub use registry::{CollisionBox, RegistryRecord, read_registry};
#[path = "../src/physics_registry.rs"]
mod physics_registry;

use physics_registry::{
    BlockPhysicsFlags, BlockPhysicsRecord, PhysicsRegistry, SurfaceResponse, read_physics_registry,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const BREG: &[u8] = include_bytes!("../data/block-registry-v1001.bin");

fn valid_preg(records: &[RegistryRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PREG1001");
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(records.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(BREG));
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        let water = record.name.as_ref() == "minecraft:water"
            || record.name.as_ref() == "minecraft:flowing_water"
            || record.name.as_ref() == "minecraft:bubble_column";
        let lava = record.name.as_ref() == "minecraft:lava"
            || record.name.as_ref() == "minecraft:flowing_lava";
        let fluid = water || lava;
        let boxes = if fluid {
            &[][..]
        } else {
            record.collision_seed.boxes.as_ref()
        };
        let mut flags = BlockPhysicsFlags::empty();
        if boxes.is_empty() {
            flags |= BlockPhysicsFlags::PASSABLE;
        }
        if water {
            flags |= BlockPhysicsFlags::WATER;
        }
        if lava {
            flags |= BlockPhysicsFlags::LAVA;
        }
        let bubble = record.name.as_ref() == "minecraft:bubble_column";
        let response = if bubble && record.canonical_state.contains("\"value\":1") {
            SurfaceResponse::BubbleDown
        } else if bubble {
            SurfaceResponse::BubbleUp
        } else {
            SurfaceResponse::None
        };
        bytes.push(u8::try_from(boxes.len()).unwrap());
        bytes.push(flags.bits());
        bytes.push(response as u8);
        bytes.push(0);
        bytes.extend_from_slice(&60_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&(if fluid { 100_000_000_i32 } else { 0 }).to_le_bytes());
        for collision_box in boxes {
            for coordinate in [
                collision_box.min_x,
                collision_box.min_y,
                collision_box.min_z,
                collision_box.max_x,
                collision_box.max_y,
                collision_box.max_z,
            ] {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
    }
    append_digest(bytes)
}

fn append_digest(mut bytes: Vec<u8>) -> Vec<u8> {
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    bytes
}

fn resign(bytes: &mut Vec<u8>) {
    bytes.truncate(bytes.len() - 32);
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
}

#[test]
fn preg1001_public_decoder_contract_is_available() {
    let _: Option<BlockPhysicsRecord> = None;
    let _: Option<PhysicsRegistry> = None;
    let _ = BlockPhysicsFlags::WATER;
    let _ = SurfaceResponse::None;
    let _ = read_physics_registry;
}

#[test]
fn decodes_exact_breg_bound_identity_and_lookups() {
    let records = read_registry(BREG).unwrap();
    let preg = valid_preg(&records);
    let registry = read_physics_registry(&preg, BREG, &records).unwrap();
    assert_eq!(registry.len(), records.len());
    assert!(!registry.is_empty());
    assert_eq!(registry.sha256(), Sha256::digest(&preg).as_slice());
    assert_eq!(registry.breg_sha256(), Sha256::digest(BREG).as_slice());

    let water_identity = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:water")
        .unwrap();
    let water: &BlockPhysicsRecord = registry
        .by_sequential_id(water_identity.sequential_id)
        .unwrap();
    assert!(water.flags.contains(BlockPhysicsFlags::WATER));
    assert!(water.fluid_height_blocks() > 0.0);
    assert_eq!(water.surface_response, SurfaceResponse::None);
    assert_eq!(registry.by_network_hash(water.network_hash), Some(water));
    assert_eq!(water.friction(), 0.6);
    assert_eq!(water.horizontal_speed_factor(), 1.0);
    assert_eq!(water.vertical_speed_factor(), 1.0);

    let compound_identity = records
        .iter()
        .find(|record| record.collision_seed.boxes.len() > 1)
        .expect("compound collision state");
    let compound = registry
        .by_sequential_id(compound_identity.sequential_id)
        .unwrap();
    assert_eq!(
        compound.boxes.as_ref(),
        compound_identity.collision_seed.boxes.as_ref()
    );

    let ladder_boxes = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:ladder")
        .map(|record| {
            let decoded = registry.by_sequential_id(record.sequential_id).unwrap();
            assert_eq!(decoded.boxes.as_ref(), record.collision_seed.boxes.as_ref());
            decoded.boxes.to_vec()
        })
        .collect::<HashSet<_>>();
    assert!(ladder_boxes.len() >= 4, "ladder orientations collapsed");

    let stone_identity = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .expect("ordinary full solid");
    let stone = registry
        .by_sequential_id(stone_identity.sequential_id)
        .unwrap();
    assert!(!stone.flags.contains(BlockPhysicsFlags::PASSABLE));
    assert_eq!(stone.boxes.len(), 1);
    assert_eq!(stone.boxes[0].min_x, 0);
    assert_eq!(stone.boxes[0].max_x, 100_000_000);
}

#[test]
fn rejects_stale_or_malformed_carriers_without_partial_acceptance() {
    let records = read_registry(BREG).unwrap();
    let valid = valid_preg(&records);
    let first_record = 48;
    for (name, mutate) in [
        ("magic", (0, b'X')),
        ("protocol", (8, 0)),
        ("BREG hash", (16, 0)),
        ("identity", (first_record, 1)),
        ("flags", (first_record + 9, 0x80)),
        ("surface", (first_record + 10, 0xff)),
        ("reserved", (first_record + 11, 1)),
    ] {
        let mut broken = valid.clone();
        broken[mutate.0] = mutate.1;
        resign(&mut broken);
        let error = read_physics_registry(&broken, BREG, &records).unwrap_err();
        assert!(!error.to_string().is_empty(), "{name}");
    }

    let mut corrupt_digest = valid.clone();
    *corrupt_digest.last_mut().unwrap() ^= 1;
    assert!(read_physics_registry(&corrupt_digest, BREG, &records).is_err());

    let mut trailing = valid.clone();
    trailing.truncate(trailing.len() - 32);
    trailing.push(0);
    trailing = append_digest(trailing);
    assert!(read_physics_registry(&trailing, BREG, &records).is_err());

    let mut too_many_boxes = valid.clone();
    too_many_boxes[first_record + 8] = 33;
    resign(&mut too_many_boxes);
    assert!(read_physics_registry(&too_many_boxes, BREG, &records).is_err());

    let mut zero_scalar = valid.clone();
    zero_scalar[first_record + 12..first_record + 16].fill(0);
    resign(&mut zero_scalar);
    assert!(read_physics_registry(&zero_scalar, BREG, &records).is_err());

    let mut inverted_box = valid.clone();
    inverted_box[first_record + 8] = 1;
    let box_bytes = [2_i32, 0, 0, 1, 1, 1]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect::<Vec<_>>();
    inverted_box.splice(first_record + 28..first_record + 28, box_bytes);
    resign(&mut inverted_box);
    assert!(read_physics_registry(&inverted_box, BREG, &records).is_err());

    let mut stale_breg = BREG.to_vec();
    *stale_breg.last_mut().unwrap() ^= 1;
    assert!(read_physics_registry(&valid, &stale_breg, &records).is_err());
}
