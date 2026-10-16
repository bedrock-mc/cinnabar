//! Near terrain is retired as a whole before a new world generation publishes uploads.

use render_scene::{ChunkRenderQueue, ChunkUploadAcknowledgements};

/// Drops queued work and all resident meshes when the stream or dimension lifetime changes.
/// Returns whether other view-local state must also reset before publication.
pub(crate) fn synchronize_terrain_generation(
    rendered: &mut Option<(u64, u64)>,
    active: Option<(u64, u64)>,
    queue: &mut ChunkRenderQueue,
    acknowledgements: &ChunkUploadAcknowledgements,
) -> bool {
    if *rendered == active {
        return false;
    }
    queue.reset_session();
    acknowledgements.clear();
    *rendered = active;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use chunk_pipeline::WorldStream;
    use render_scene::{ChunkRenderInstance, ChunkUploadPriority};

    /// Original solid geometry without any runtime game assets.
    fn mesh() -> meshing::ChunkMesh {
        let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
        meshing::mesh_sub_chunk(
            &meshing::BlockClassifier::new(0),
            &assets::RuntimeAssets::diagnostic(),
            assets::NetworkIdMode::Sequential,
            &meshing::Neighbourhood::empty(),
            &source,
        )
    }

    /// A transfer retires uploaded and queued old terrain together, without a removal budget.
    #[test]
    fn dimension_round_trip_retires_near_terrain_before_reusing_keys() {
        let mut stream = WorldStream::new(protocol::WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(render::ChunkRenderPlugin::new(1));
        let acknowledgements = app
            .world()
            .resource::<ChunkUploadAcknowledgements>()
            .clone();
        let mut rendered = Some(stream.terrain_generation());
        let keys = [
            world::SubChunkKey::new(0, 0, 0, 0),
            world::SubChunkKey::new(0, 1, 0, 0),
        ];
        for key in keys {
            app.world_mut()
                .resource_mut::<ChunkRenderQueue>()
                .try_insert(key, mesh(), ChunkUploadPriority::new(0.0))
                .unwrap();
        }
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&ChunkRenderInstance>()
                .iter(app.world())
                .count(),
            1
        );
        assert_eq!(app.world().resource::<ChunkRenderQueue>().pending_len(), 1);
        for (sequence, dimension) in [(1, 1), (2, 0)] {
            stream
                .submit(
                    sequence,
                    protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                        dimension,
                        ..Default::default()
                    }),
                )
                .unwrap();
        }
        assert!(synchronize_terrain_generation(
            &mut rendered,
            Some(stream.terrain_generation()),
            &mut app.world_mut().resource_mut::<ChunkRenderQueue>(),
            &acknowledgements,
        ));
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&ChunkRenderInstance>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(app.world().resource::<ChunkRenderQueue>().pending_len(), 0);
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_insert(keys[0], mesh(), ChunkUploadPriority::new(0.0))
            .unwrap();
        app.update();
        let mut query = app.world_mut().query::<&ChunkRenderInstance>();
        let current: Vec<_> = query
            .iter(app.world())
            .map(|instance| instance.key)
            .collect();
        assert_eq!(current, [keys[0]]);
        assert!(!synchronize_terrain_generation(
            &mut rendered,
            Some(stream.terrain_generation()),
            &mut app.world_mut().resource_mut::<ChunkRenderQueue>(),
            &acknowledgements,
        ));
    }
}
