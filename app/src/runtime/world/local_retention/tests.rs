use bevy::{ecs::system::RunSystemOnce, prelude::*};
use client_presentation::local_player_camera_receipt::{
    PublishedCamera, begin_camera_publication_attempt,
};
use protocol::{WorldBootstrap, WorldEvent};
use semantic_input::PerspectiveMode;

use super::*;

/// Creates a live stream and completed physics without a server movement echo.
fn fixture() -> (WorldStream, LocalPhysicsController) {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(2)).unwrap();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([320.0, 70.0, 0.0], 10, false);
    (stream, physics)
}

/// Publishes a test receipt through the same per-frame attempt lifecycle.
fn publication(owner: CameraOwner, tick: u64) -> CameraPublicationAttempt {
    let mut world = World::new();
    world.init_resource::<CameraPublicationAttempt>();
    world
        .run_system_once(begin_camera_publication_attempt)
        .unwrap();
    let mut publication = world.remove_resource::<CameraPublicationAttempt>().unwrap();
    publication.finish(
        PublishedCamera {
            owner,
            entity: Entity::PLACEHOLDER,
            transform: Transform::from_translation(Vec3::new(-1600.0, 70.0, 0.0)),
            perspective: PerspectiveMode::FirstPerson,
            tick,
            frame_generation: 0,
        },
        1,
    );
    publication
}

#[test]
fn retention_uses_physics_and_requires_the_current_publication_owner() {
    let (mut stream, physics) = fixture();
    let owner = CameraOwner::current(&stream, 4);
    assert!(!retain_completed_player_terrain(
        &mut stream,
        &physics,
        None,
        4
    ));
    for stale in [
        CameraOwner {
            session: owner.session + 1,
            ..owner
        },
        CameraOwner {
            stream: owner.stream + 1,
            ..owner
        },
        CameraOwner {
            dimension: 1,
            ..owner
        },
        CameraOwner {
            epoch: owner.epoch + 1,
            ..owner
        },
        CameraOwner {
            sequence: owner.sequence + 1,
            ..owner
        },
    ] {
        let receipt = publication(stale, 10);
        assert!(!retain_completed_player_terrain(
            &mut stream,
            &physics,
            Some(&receipt),
            4
        ));
    }
    let wrong_tick = publication(owner, 9);
    assert!(!retain_completed_player_terrain(
        &mut stream,
        &physics,
        Some(&wrong_tick),
        4
    ));
    let current = publication(owner, 10);
    assert!(retain_completed_player_terrain(
        &mut stream,
        &physics,
        Some(&current),
        4
    ));
    assert!(!stream.retain_for_local_player(
        owner.stream,
        owner.dimension,
        owner.epoch,
        [320.5, 70.0, 0.0]
    ));
    assert_eq!(stream.resolved_server_position().position, [0.5, 70.0, 0.5]);
}

/// Server moves must still recenter terrain when no local physics will publish a position.
#[test]
fn inactive_physics_retains_around_the_committed_server_position() {
    let (mut stream, physics) = fixture();
    let owner = CameraOwner::current(&stream, 4);
    assert!(retain_completed_player_terrain(
        &mut stream,
        &physics,
        Some(&publication(owner, 10)),
        4
    ));
    stream
        .submit(
            2,
            WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position: [1600.5, 70.0, 0.5],
                ..Default::default()
            }),
        )
        .unwrap();
    stream.take_committed_controls();
    let inactive = LocalPhysicsController::default();
    assert!(retain_completed_player_terrain(
        &mut stream,
        &inactive,
        None,
        4
    ));
    assert!(!retain_completed_player_terrain(
        &mut stream,
        &inactive,
        None,
        4
    ));
}
