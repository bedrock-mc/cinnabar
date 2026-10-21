use super::*;
use protocol::test_support::packets::{ActorAbsoluteFixture, move_actor_absolute_event};
use std::sync::Arc;

/// Couples wire normalization, committed authority and the existing prediction owners.
#[test]
fn ten_server_actor_moves_lift_local_physics_half_a_block_without_teleport_acknowledgement() {
    let (mut movement, mut physics, mut effects, mut speed) = owners();
    let initial = physics.network_position().unwrap();
    let velocity = [0.125, 0.25, -0.0625];
    physics.replace_live_velocity(velocity);
    let mut authority = client_world::WorldAuthority::new(
        protocol::WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: initial,
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        initial,
        None,
    );
    for step in 1..=10 {
        let progress = step as f32 / 10.0;
        let rise = 0.5 * (1.0 - (1.0 - progress).powi(3));
        let position = [initial[0], initial[1] + rise, initial[2]];
        let event = move_actor_absolute_event(ActorAbsoluteFixture {
            runtime_id: 1,
            flags: 3,
            position,
            ..Default::default()
        })
        .unwrap()
        .unwrap();
        authority.apply_ordered_event(event, Some(step)).unwrap();
        let controls = authority.take_committed_controls();
        assert_eq!(controls.len(), 1, "step {step} must reach local physics");
        for control in controls {
            let disposition = CommittedGameplayState {
                movement: &mut movement,
                physics: &mut physics,
                effects: &mut effects,
                speed: &mut speed,
                session_generation: 7,
                dimension: 0,
                dimension_transfer_active: false,
            }
            .apply(control, &NoQueries, |_| {});
            assert_eq!(
                disposition,
                ControlDisposition::Spatial(SpatialReset::Correction)
            );
        }
        assert_eq!(physics.network_position(), Some(position));
        assert_eq!(
            physics.state().unwrap().velocity,
            sim::Vec3::new(0.125, 0.25, -0.0625)
        );
        assert_eq!(movement.completed_tick(), 100);
        assert_eq!(movement.pending_teleport_ack_admitted_ticks(), None);
    }
    assert_eq!(physics.network_position().unwrap()[1] - initial[1], 0.5);
    assert!(movement.physics_is_authorized());
}

#[test]
fn queued_actor_movement_from_an_old_dimension_cannot_reanchor_the_new_one() {
    let (mut movement, mut physics, mut effects, mut speed) = owners();
    let initial = physics.network_position();
    let protocol::WorldEvent::Actor(protocol::ActorEvent::Move(update)) =
        move_actor_absolute_event(ActorAbsoluteFixture {
            runtime_id: 1,
            flags: 3,
            position: [10.0, 80.0, 20.0],
            ..Default::default()
        })
        .unwrap()
        .unwrap()
    else {
        panic!("move")
    };
    let mut observed = false;
    let disposition = CommittedGameplayState {
        movement: &mut movement,
        physics: &mut physics,
        effects: &mut effects,
        speed: &mut speed,
        session_generation: 7,
        dimension: 1,
        dimension_transfer_active: true,
    }
    .apply(
        CommittedControlEvent::LocalActorMove {
            sequence: 1,
            movement: update,
            resolved: ResolvedServerPosition {
                position: [10.0, 80.0, 20.0],
                surface_anchor: None,
            },
        },
        &NoQueries,
        |_| observed = true,
    );
    assert_eq!(disposition, ControlDisposition::Handled);
    assert!(!observed);
    assert_eq!(physics.network_position(), initial);
    assert_eq!(movement.completed_tick(), 100);
}
