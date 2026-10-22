//! Measures production job capture without scheduling or executing workers.

use super::*;

/// Resident packed sections and current light shared by overlapping job neighbourhoods.
pub struct DispatchFixture {
    stream: WorldStream,
    keys: Vec<SubChunkKey>,
}

impl DispatchFixture {
    /// Installs a bounded mixed-palette terrain cohort; setup is outside the timed region.
    pub fn new(sections: usize) -> Self {
        assert!(sections > 0);
        let mut stream = WorldStream::new(client_world::ingestion::WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [8.0; 3],
            world_spawn_position: [8; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let side = (sections.div_ceil(4) as f64).sqrt().ceil() as i32;
        let mut bytes = vec![9, 1, 0, 3];
        for _ in 0..128 {
            bytes.extend_from_slice(&0xaaaa_aaaa_u32.to_le_bytes());
        }
        bytes.extend([4, 0, 2]);
        let section = SubChunk::decode(&bytes, &world::RawBlockIds { air: 0 });
        let keys: Vec<_> = (0..sections)
            .map(|index| {
                let column = (index / 4) as i32;
                SubChunkKey::new(0, column % side, (index % 4) as i32, column / side)
            })
            .collect();
        let halo: BTreeSet<_> = keys
            .iter()
            .flat_map(|key| key.mesh_neighbourhood_dependents())
            .collect();
        for key in halo {
            stream
                .authority
                .commit_sub_chunk(key, section.clone())
                .unwrap();
            stream.resident.insert(key);
            stream.lighting.block_generations.insert(key, 1);
            let mut light = SubChunkLight::uniform(0, 15, 1).unwrap();
            light.set(LightChannel::Block, 8, 8, 8, 7).unwrap();
            stream.lighting.store.insert_resident(key, light);
            stream.lighting.ownership.insert(
                key,
                LightOwnership {
                    block_generation: 1,
                    light_revision: 1,
                },
            );
            stream.lighting.direct_sky.insert(
                key,
                StoredDirectSky {
                    light_revision: 1,
                    mask: Arc::new(DirectSkyMask::Uniform(true)),
                },
            );
        }
        Self { stream, keys }
    }

    /// Captures and releases the same inputs prepared for each dispatched light job.
    pub fn capture_light(&self) {
        for &key in &self.keys {
            std::hint::black_box((
                self.stream.light_block_snapshot(key),
                self.stream.light_prior_snapshot(key),
            ));
        }
    }

    /// Captures and releases the same inputs prepared for each dispatched mesh job.
    pub fn capture_mesh(&self) {
        for &key in &self.keys {
            let center = self.stream.authority.terrain().sub_chunk(key).unwrap();
            let halo = self.stream.mesh_light_halo(key).unwrap();
            std::hint::black_box(self.stream.mesh_snapshot(key, center, halo));
        }
    }
}
