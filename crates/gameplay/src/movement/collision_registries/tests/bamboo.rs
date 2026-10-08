use super::{BREG_V2193, active_content_registry_protocol, bind, synthetic_preg};

#[test]
fn bamboo_offset_requires_an_admitted_primary_stalk_state() {
    let protocol = active_content_registry_protocol();
    let originals = assets::read_registry_for_protocol(BREG_V2193, protocol).unwrap();
    let index = originals
        .iter()
        .position(|record| record.name.as_ref() == "minecraft:bamboo")
        .unwrap();
    for rejection in 0..3 {
        let mut record = originals[index].clone();
        record.sequential_id = 0;
        match rejection {
            0 => record.flags = assets::BlockFlags::AIR,
            1 => record.contributor_role = assets::ContributorRole::LiquidAdditional,
            _ => record.canonical_state = "{}".into(),
        }
        let ids = [
            (assets::NetworkIdMode::Sequential, record.sequential_id),
            (assets::NetworkIdMode::Hashed, record.network_hash),
        ];
        let breg = single_record_breg(protocol, &record);
        let records = assets::read_registry_for_protocol(&breg, protocol).unwrap();
        let registries = super::super::PhysicsCollisionRegistries::from_assets(
            &breg,
            &records,
            &synthetic_preg(protocol, &breg, &records),
            protocol,
        )
        .unwrap();
        for (mode, id) in ids {
            assert_eq!(
                registries.registry(mode).block_shape_offset(id, [1, 3, 0]),
                Some(sim::Vec3::new(1.0, 3.0, 0.0)),
                "rejection {rejection}, {mode:?}"
            );
        }
    }
}

/// Encodes one exact registry record so the strict physics carrier binding stays exercised.
fn single_record_breg(protocol: u32, record: &assets::RegistryRecord) -> Vec<u8> {
    let valentine = u32::from(
        record
            .provenance
            .contains(assets::RegistryProvenance::VALENTINE),
    );
    let mut bytes = b"BREG1003".to_vec();
    for value in [
        protocol,
        1,
        1,
        valentine,
        valentine,
        1 - valentine,
        1 - valentine,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [record.sequential_id, record.network_hash] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend([
        record.flags.bits(),
        record.model_family as u8,
        record.contributor_role as u8,
        record.model_state.mask(),
        record.face_coverage,
        record.collision_seed.confidence as u8,
        record.provenance.bits(),
        record.collision_seed.boxes.len() as u8,
    ]);
    bytes.extend_from_slice(&record.collision_seed.shape_id.to_le_bytes());
    bytes.extend_from_slice(&(record.name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&(record.canonical_state.len() as u32).to_le_bytes());
    use assets::ModelStateField::*;
    for field in [
        Orientation,
        Half,
        Open,
        Hinge,
        Connections,
        Growth,
        LiquidDepth,
        Flags,
    ] {
        bytes.extend_from_slice(
            &record
                .model_state
                .get(field)
                .unwrap_or_default()
                .to_le_bytes(),
        );
    }
    for shape in &record.collision_seed.boxes {
        for value in [
            shape.min_x,
            shape.min_y,
            shape.min_z,
            shape.max_x,
            shape.max_y,
            shape.max_z,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(record.name.as_bytes());
    bytes.extend_from_slice(record.canonical_state.as_bytes());
    bytes
}

#[test]
fn bamboo_registry_admits_displaced_picking_in_both_runtime_id_spaces() {
    let protocol = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, protocol).unwrap();
    let registries = bind(
        BREG_V2193,
        &synthetic_preg(protocol, BREG_V2193, &records),
        protocol,
    )
    .unwrap();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:bamboo")
    {
        for (mode, id) in [
            (assets::NetworkIdMode::Sequential, record.sequential_id),
            (assets::NetworkIdMode::Hashed, record.network_hash),
        ] {
            let registry = registries.registry(mode);
            let air_record = records
                .iter()
                .find(|record| record.name.as_ref() == "minecraft:air")
                .unwrap();
            let air = match mode {
                assets::NetworkIdMode::Sequential => air_record.sequential_id,
                assets::NetworkIdMode::Hashed => air_record.network_hash,
            };
            let mut store = world::ChunkStore::new();
            for x in -1..=1 {
                for z in -1..=1 {
                    store
                        .mark_chunk_loaded(world::ChunkKey::new(0, x, z))
                        .unwrap();
                }
            }
            store
                .update_block(
                    world::SubChunkKey::new(0, 0, 0, 0),
                    world::BlockUpdate::new(1, 8, 0, 0, id),
                    air,
                )
                .unwrap();
            let hit = sim::PaletteWorld::new(&store, registry, 0)
                .block_interaction_ray_current(
                    sim::Vec3::new(1.78, 8.5, 2.0),
                    sim::Vec3::new(0.0, 0.0, -1.0),
                    4.0,
                )
                .unwrap()
                .expect("visible displaced bamboo must be pickable");
            assert_eq!((hit.block_pos, hit.runtime_id), ([1, 8, 0], id));
            assert_eq!(
                registry.block_shape_offset(id, [0; 3]),
                Some(sim::Vec3::ZERO),
                "origin-sampled carrier bounds stay unchanged"
            );
        }
    }
}
