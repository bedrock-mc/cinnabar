//! Current terrain work at the State query boundary, independent of visibility probes.

use bevy::{diagnostic::FrameCount, prelude::World};
use client_runtime::publication::PublicationController;
use render_scene::ChunkRenderQueue;
use serde::Serialize;

use crate::runtime::world::ClientWorld;

/// Live work gauges, without a claim about future server data or GPU presentation.
#[derive(Serialize)]
pub(super) struct WorldWork {
    frame_count: u32,
    session_generation: u64,
    admitted_world_events: usize,
    queued_decode_jobs: usize,
    in_flight_decode_jobs: usize,
    completed_decode_results: usize,
    pending_light_jobs: usize,
    in_flight_light_jobs: usize,
    terminal_light_failures: usize,
    pending_mesh_jobs: usize,
    in_flight_mesh_jobs: usize,
    stream_pending_mesh_changes: usize,
    unacknowledged_meshes: usize,
    upload_queue_items: usize,
    upload_queue_bytes: u64,
    render_session_reset_pending: bool,
    allowance_live_permits: usize,
    allowance_live_payload_bytes: u64,
}

/// Reads the current owners on demand; absent owners never become a zero-work sample.
pub(super) fn snapshot(world: &World) -> Option<WorldWork> {
    let stream = world.get_resource::<ClientWorld>()?.stream.as_ref()?;
    let frame = world.get_resource::<FrameCount>()?;
    let queue = world.get_resource::<ChunkRenderQueue>()?;
    let allowance = world.get_resource::<PublicationController>()?.allowance();
    let stats = stream.stats();
    Some(WorldWork {
        frame_count: frame.0,
        session_generation: stream.authority().actor_session_id(),
        admitted_world_events: stats.admitted_world_events,
        queued_decode_jobs: stats.queued_decode_jobs,
        in_flight_decode_jobs: stats.in_flight_decode_jobs,
        completed_decode_results: stats.completed_decode_results,
        pending_light_jobs: stats.pending_light_jobs,
        in_flight_light_jobs: stats.in_flight_light_jobs,
        terminal_light_failures: stats.terminal_light_failures,
        pending_mesh_jobs: stats.pending_mesh_jobs,
        in_flight_mesh_jobs: stats.in_flight_mesh_jobs,
        stream_pending_mesh_changes: stream.pending_mesh_change_count(),
        unacknowledged_meshes: stream.unacknowledged_mesh_count(),
        upload_queue_items: queue.retained_len(),
        upload_queue_bytes: queue.pending_bytes(),
        render_session_reset_pending: queue.session_reset_pending,
        allowance_live_permits: allowance.live_permits(),
        allowance_live_payload_bytes: allowance.live_payload_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use client_runtime::publication::PublicationFrameWork;
    use render_scene::VisibilityDiagnosticsInput;

    use super::*;

    /// Creates an asset-independent stream with a genuine fresh session identity.
    fn stream() -> chunk_pipeline::WorldStream {
        chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        })
    }

    /// Installs the actual work owners without a renderer or a visibility probe.
    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(ClientWorld {
            stream: Some(stream()),
            ..Default::default()
        });
        world.insert_resource(FrameCount(1));
        world.init_resource::<ChunkRenderQueue>();
        world.init_resource::<PublicationController>();
        world
    }

    #[test]
    fn absent_and_retired_owners_are_not_quiet_samples() {
        assert!(snapshot(&World::new()).is_none());
        let mut world = world();
        let first = snapshot(&world).unwrap();

        world.resource_mut::<ClientWorld>().stream = None;
        assert!(snapshot(&world).is_none());

        world.resource_mut::<ClientWorld>().stream = Some(stream());
        world.resource_mut::<FrameCount>().0 += 1;
        let replacement = snapshot(&world).unwrap();
        assert_ne!(replacement.session_generation, first.session_generation);
        assert!(replacement.frame_count > first.frame_count);

        world.remove_resource::<ChunkRenderQueue>();
        assert!(snapshot(&world).is_none());
    }

    #[test]
    fn live_work_remains_observable_after_startup_probes_stop() {
        let mut world = world();
        let mut visibility = VisibilityDiagnosticsInput::new(false);
        visibility.set_startup_probe_enabled(true);
        visibility.set_startup_probe_enabled(false);
        world.insert_resource(visibility);
        world
            .resource_mut::<PublicationController>()
            .finish_frame(PublicationFrameWork {
                upload_queue_items: 99,
                allowance_live_permits: 99,
                ..Default::default()
            });
        let initial = snapshot(&world).unwrap();
        assert_eq!(initial.upload_queue_items, 0);
        assert_eq!(initial.allowance_live_permits, 0);

        let allowance = world.resource::<PublicationController>().allowance();
        allowance.begin_frame(1, 1, 64, 0, 1);
        let permit = allowance.try_admit_payload(64).unwrap();
        world
            .resource_mut::<ChunkRenderQueue>()
            .try_remove(world::SubChunkKey::new(0, 0, 0, 0))
            .unwrap();
        let busy = snapshot(&world).unwrap();
        assert!(!world.resource::<VisibilityDiagnosticsInput>().enabled());
        assert_eq!(busy.upload_queue_items, 1);
        assert_eq!(busy.allowance_live_permits, 1);
        assert_eq!(busy.allowance_live_payload_bytes, 64);

        drop(permit);
        world.resource_mut::<ChunkRenderQueue>().reset_session();
        let reset = snapshot(&world).unwrap();
        assert_eq!(reset.upload_queue_items, 0);
        assert!(reset.render_session_reset_pending);
        assert_eq!(reset.allowance_live_permits, 0);
        assert_eq!(reset.allowance_live_payload_bytes, 0);
    }
}
