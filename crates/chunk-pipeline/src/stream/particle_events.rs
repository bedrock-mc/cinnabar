use super::WorldStream;

impl WorldStream {
    /// Normalizes particle block ids through the authoritative session palette.
    pub(super) fn remap_particle_block_ids(
        &self,
        event: &mut client_world::ingestion::ParticleEvent,
    ) {
        self.authority.remap_particle_block_ids(event);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use assets::{RuntimeAssets, SequentialIdRemap};
    use protocol::{LevelParticleEvent, ParticleEvent, WorldBootstrap, WorldEvent};

    use crate::WorldStream;

    /// Sends terrain events through the ordered commit path with shifted wire ids.
    fn terrain_events(hashes: bool) -> Vec<i32> {
        let mut stream = WorldStream::new_with_assets(
            WorldBootstrap {
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                dimension: 0,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: hashes,
            },
            Arc::new(RuntimeAssets::diagnostic()),
            [0.0; 3],
            None,
        );
        stream.set_sequential_id_remap(SequentialIdRemap::new([(3, 2, 10)]));
        for (index, (event_id, data)) in [
            (2001, 7),
            (2021, 7),
            (0x4000 | 19, 7),
            (2014, (5 << 24) | 7),
            (2002, 7),
        ]
        .into_iter()
        .enumerate()
        {
            stream
                .submit(
                    index as u64 + 1,
                    WorldEvent::Particle(ParticleEvent::Level(LevelParticleEvent {
                        event_id,
                        position: [0.0; 3],
                        data,
                    })),
                )
                .unwrap();
        }
        stream
            .take_committed_particles()
            .into_iter()
            .map(|committed| match committed.event {
                ParticleEvent::Level(event) => event.data,
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn terrain_particle_events_use_the_same_internal_ids_as_chunks() {
        assert_eq!(terrain_events(false), [5, 5, 5, (5 << 24) | 5, 7]);
    }

    #[test]
    fn hashed_terrain_events_preserve_hashes_and_crack_face_bits() {
        assert_eq!(terrain_events(true), [7, 7, 7, (5 << 24) | 7, 7]);
    }
}
