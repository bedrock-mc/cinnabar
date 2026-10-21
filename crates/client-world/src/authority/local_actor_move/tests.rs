use super::*;
use protocol::test_support::packets::{ActorAbsoluteFixture, move_actor_absolute_event};

/// Starts a session without a client-fed rig to exercise early incoming movement too.
fn authority() -> WorldAuthority {
    WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.5, 2.620_01, 0.5],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    )
}

#[test]
fn actor_move_controls_admit_only_marked_local_positions() {
    for runtime_id in [1, 2] {
        for (flags, allowed) in [
            (0, false),
            (1, false),
            (2, true),
            (3, true),
            (4, true),
            (5, true),
            (6, true),
            (7, true),
            (8, false),
            (15, true),
        ] {
            let mut authority = authority();
            let event = move_actor_absolute_event(ActorAbsoluteFixture {
                runtime_id,
                flags,
                position: [3.0, 64.5, 4.0],
                ..Default::default()
            })
            .unwrap()
            .unwrap();
            authority.apply_ordered_event(event, Some(1)).unwrap();
            assert_eq!(
                authority.has_pending_spatial_control(),
                runtime_id == 1 && allowed
            );
            let controls = authority.take_committed_controls();
            if runtime_id == 1 && allowed {
                assert!(
                    matches!(controls.as_slice(), [CommittedControlEvent::LocalActorMove { sequence: 1, resolved, .. }] if resolved.position == [3.0,64.5,4.0])
                );
            } else {
                assert!(controls.is_empty());
                assert_eq!(
                    authority.resolved_server_position().position,
                    [0.5, 2.620_01, 0.5]
                );
            }
        }
    }
}

#[test]
fn partial_marked_moves_merge_the_latest_decoded_target_without_admitting_unmarked_moves() {
    let mut authority = authority();
    let event = move_actor_absolute_event(ActorAbsoluteFixture {
        runtime_id: 1,
        flags: 0,
        position: [10.0, 64.5, 20.0],
        ..Default::default()
    })
    .unwrap()
    .unwrap();
    authority
        .apply_ordered_event(event.clone(), Some(1))
        .unwrap();
    assert!(authority.take_committed_controls().is_empty());
    let WorldEvent::Actor(ActorEvent::Move(mut movement)) = event else {
        panic!("move")
    };
    movement.position = [None, Some(65.0), None];
    movement.force_local_move = true;
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Move(movement)), Some(2))
        .unwrap();
    assert!(
        matches!(authority.take_committed_controls().as_slice(), [CommittedControlEvent::LocalActorMove { resolved, .. }] if resolved.position == [10.0,65.0,20.0])
    );
    movement.dimension = 1;
    movement.position = [Some(999.0); 3];
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Move(movement)), Some(3))
        .unwrap();
    assert!(authority.take_committed_controls().is_empty());
    assert_eq!(
        authority.resolved_server_position().position,
        [10.0, 65.0, 20.0]
    );
}

#[test]
fn dimension_and_move_player_boundaries_replace_the_partial_actor_target() {
    let mut authority = authority();
    let event = move_actor_absolute_event(ActorAbsoluteFixture {
        runtime_id: 1,
        flags: 0,
        position: [10.0, 64.5, 20.0],
        ..Default::default()
    })
    .unwrap()
    .unwrap();
    authority
        .apply_ordered_event(event.clone(), Some(1))
        .unwrap();
    authority.reset_dimension(2, 1);
    authority.resolve_position(2, [30.0, 80.0, 40.0]);
    // A delayed old-dimension update must not seed the new dimension's target.
    authority
        .apply_ordered_event(event.clone(), Some(3))
        .unwrap();
    let WorldEvent::Actor(ActorEvent::Move(mut movement)) = event else {
        panic!("move")
    };
    movement.dimension = 1;
    movement.force_local_move = true;
    movement.position = [None, Some(81.0), None];
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Move(movement)), Some(4))
        .unwrap();
    assert!(
        matches!(authority.take_committed_controls().as_slice(), [CommittedControlEvent::LocalActorMove { resolved, .. }] if resolved.position == [30.0,81.0,40.0])
    );
    authority.apply_player_move(
        5,
        MovePlayerEvent {
            runtime_id: 1,
            position: [50.0, 90.0, 60.0],
            ..Default::default()
        },
    );
    movement.position = [Some(51.0), None, None];
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Move(movement)), Some(6))
        .unwrap();
    assert!(
        matches!(authority.take_committed_controls().as_slice(), [CommittedControlEvent::LocalActorMove { resolved, .. }] if resolved.position == [51.0,90.0,60.0])
    );
}
