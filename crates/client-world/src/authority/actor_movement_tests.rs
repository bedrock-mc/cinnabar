use super::*;
use assets::RuntimeAssets;
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use std::sync::Arc;

/// Spawns a remote actor at the origin with an optional resident all-air column.
fn authority(player: bool, loaded: bool) -> WorldAuthority {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    if loaded {
        authority.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    }
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: if player {
                    ActorKind::Player {
                        uuid: [2; 16],
                        username: "Remote".into(),
                    }
                } else {
                    ActorKind::Entity {
                        identifier: "minecraft:pig".into(),
                    }
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: [].into(),
                attributes: [].into(),
                properties: [].into(),
                links: [].into(),
            })),
            Some(1),
        )
        .unwrap();
    authority
}

/// Builds a partial MoveActor update without changing its vertical native origin.
fn movement(x: Option<f32>, forced: bool) -> ActorMoveEvent {
    ActorMoveEvent {
        dimension: 0,
        runtime_id: 2,
        position: [x, None, None],
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: None,
        yaw: None,
        head_yaw: None,
        on_ground: None,
        teleported: forced,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    }
}

#[test]
fn forced_actor_move_interpolates_below_half_a_block_and_snaps_at_the_boundary() {
    for player in [false, true] {
        for x in [f32::from_bits(0.5_f32.to_bits() - 1), 0.5] {
            let mut authority = authority(player, true);
            authority
                .apply_ordered_event(
                    WorldEvent::Actor(ActorEvent::Move(movement(Some(x), true))),
                    Some(2),
                )
                .unwrap();
            if x < 0.5 {
                assert_eq!(authority.actor(2).unwrap().position[0], 0.0);
                authority.advance_actor_interpolation_ticks(1);
                assert_eq!(authority.actor(2).unwrap().position[0], x * (1.0 / 3.0));
            } else {
                assert_eq!(authority.actor(2).unwrap().position[0], x);
            }
        }
    }
}

#[test]
fn ordinary_moves_snap_only_when_the_target_column_is_absent() {
    for loaded in [false, true] {
        let mut authority = authority(false, loaded);
        authority
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Move(movement(Some(1.0), false))),
                Some(2),
            )
            .unwrap();
        assert_eq!(
            authority.actor(2).unwrap().position[0],
            if loaded { 0.0 } else { 1.0 }
        );
    }
}

#[test]
fn partial_forced_updates_measure_from_the_latest_decoded_target() {
    let mut authority = authority(false, true);
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Move(movement(Some(1.0), false))),
            Some(2),
        )
        .unwrap();
    assert_eq!(authority.actor(2).unwrap().position[0], 0.0);
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Move(movement(None, true))),
            Some(3),
        )
        .unwrap();
    assert_eq!(authority.actor(2).unwrap().position[0], 1.0);
}

#[test]
fn move_player_teleports_keep_their_own_rules() {
    let mut authority = authority(true, true);
    let mut movement = movement(Some(0.1), true);
    movement.position_origin = ActorPositionOrigin::Feet;
    movement.player_mode = Some(protocol::MovePlayerMode::Teleport);
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Move(movement)), Some(2))
        .unwrap();
    assert_eq!(authority.actor(2).unwrap().position[0], 0.1);
}

#[test]
fn pending_target_column_keeps_actor_interpolation_independent_of_decode_timing() {
    use crate::ingestion::{
        CommitStep, LaneContext, OrderedCommitState, PreparedWorldEvent, classify,
    };
    for ready in [false, true] {
        let mut authority = authority(false, false);
        let chunk = WorldEvent::LevelChunk(protocol::LevelChunkEvent {
            dimension: 0,
            x: 0,
            z: 0,
            mode: protocol::LevelChunkMode::Inline { count: 0 },
            payload: Vec::new(),
        });
        let move_event = WorldEvent::Actor(ActorEvent::Move(movement(Some(1.0), false)));
        let context = LaneContext {
            local_runtime_id: 1,
            local_unique_id: 1,
            dimension: 0,
        };
        let mut queue = OrderedCommitState::new(2);
        queue.admit(2, classify(&chunk, context), 0).unwrap();
        queue.admit(3, classify(&move_event, context), 0).unwrap();
        queue
            .insert_ready(3, PreparedWorldEvent::Immediate(move_event.clone()))
            .unwrap();
        if ready {
            queue
                .insert_ready(2, PreparedWorldEvent::Immediate(chunk.clone()))
                .unwrap();
        } else {
            assert!(queue.next_commit().is_none());
            queue
                .insert_ready(2, PreparedWorldEvent::Immediate(chunk.clone()))
                .unwrap();
        }
        let Some(CommitStep::Apply { sequence: 2, .. }) = queue.next_commit() else {
            panic!("the target column must commit before the actor move");
        };
        authority.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
        assert!(queue.finish_commit(2));
        let Some(CommitStep::Apply { sequence: 3, .. }) = queue.next_commit() else {
            panic!("the move is ready after its target column");
        };
        authority.apply_ordered_event(move_event, Some(3)).unwrap();
        assert!(queue.finish_commit(3));
        assert_eq!(authority.actor(2).unwrap().position[0], 0.0);
        authority.advance_actor_interpolation_ticks(1);
        assert_eq!(authority.actor(2).unwrap().position[0], 1.0 / 3.0);
    }
}
