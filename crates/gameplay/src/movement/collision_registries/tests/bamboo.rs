use super::{BREG_V2193, active_content_registry_protocol, bind, synthetic_preg};

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
