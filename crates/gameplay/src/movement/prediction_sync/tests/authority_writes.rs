use super::*;
use crate::committed_control::CommittedGameplayState;
use std::sync::Arc;

/// An unsupported prediction model must not hide a newer actor current.
#[test]
fn sync_reports_unsupported_received_speed() {
    let normal = protocol::ActorAttribute {
        name: Arc::from(Attribute::Movement.wire_name()),
        min: 0.0,
        max: 100.0,
        current: 0.1,
        default: Some(0.1),
        modifiers: Arc::from([]),
    };
    let mut zero_factor = normal.clone();
    zero_factor.current = 0.0;
    zero_factor.modifiers = Arc::from([protocol::ActorAttributeModifier {
        id: Arc::from(protocol_ids::SPRINT_SPEED_MODIFIER_ID),
        name: Arc::from("sprint"),
        amount: -1.0,
        operation: 2,
        operand: 2,
        serializable: false,
    }]);
    let mut missing_default = normal.clone();
    missing_default.current = 0.04;
    missing_default.default = None;
    let mut signed_bounds = normal.clone();
    signed_bounds.min = -1.0;
    signed_bounds.current = 0.05;
    for received in [zero_factor, missing_default, signed_bounds] {
        assert!(client_world::MovementSpeedAttribute::from_attribute(&received).is_none());
        for duplicate in [false, true] {
            let mut world = local_actor_authority();
            let mut physics = due_physics();
            let mut movement = crate::movement::MovementTicker::default();
            let mut effects = crate::movement::LocalMovementEffectTimeline::default();
            let mut speed = crate::movement::LocalMovementSpeedAuthority::default();
            speed.begin_session(1, 0);
            world.take_committed_controls();
            for (sequence, attributes) in [
                (2, vec![normal.clone()]),
                (
                    3,
                    if duplicate {
                        vec![normal.clone(), received.clone()]
                    } else {
                        vec![received.clone()]
                    },
                ),
            ] {
                world
                    .apply_ordered_event(
                        protocol::WorldEvent::Actor(protocol::ActorEvent::Attributes(
                            protocol::ActorAttributesUpdateEvent {
                                dimension: 0,
                                runtime_id: 7,
                                attributes: attributes.into(),
                                tick: 0,
                            },
                        )),
                        Some(sequence),
                    )
                    .unwrap();
                for control in world.take_committed_controls() {
                    CommittedGameplayState {
                        movement: &mut movement,
                        physics: &mut physics,
                        effects: &mut effects,
                        speed: &mut speed,
                        session_generation: 1,
                        dimension: 0,
                        dimension_transfer_active: false,
                    }
                    .apply(control, &OpenWorld, |_| {});
                }
            }
            assert_eq!(
                world.actor(7).unwrap().attributes[Attribute::Movement.wire_name()].current,
                received.current
            );
            for sprinting in [false, true, false] {
                speed.set_sprinting(sprinting);
                let actor = LocalActor(world.actor(7).unwrap().clone());
                send_movement_prediction_sync(
                    &mut physics,
                    &actor,
                    None,
                    speed.reported_current(),
                    None,
                    |packet| {
                        assert_eq!(
                            protocol::test_support::packets::prediction_sync_attributes(&packet)
                                .unwrap()[0],
                            received.current,
                            "an unsupported update leaves reporting with the actor"
                        );
                        false
                    },
                );
            }
        }
    }
}

/// A completed local pose producer clears an accepted pose even without a travel-mode edge.
#[test]
fn sync_local_tick_replaces_server_pose() {
    let mut physics = due_physics();
    let actor = local_actor();
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            gliding: Some(true),
            swimming: Some(true),
            crawling: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(physics.mode(), sim::MovementMode::Walking);
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    assert_eq!(physics.mode(), sim::MovementMode::Walking);
    send_movement_prediction_sync(&mut physics, &actor, None, None, None, |packet| {
        let words = protocol::test_support::packets::prediction_sync_flags(&packet).unwrap();
        for flag in [ActorFlag::Gliding, ActorFlag::Swimming, ActorFlag::Crawling] {
            assert!(!flag_set(&words, flag), "local producer left {flag:?} set");
        }
        true
    });
}

/// Replay keeps the order between completed pose writes and a newer unstamped server write.
#[test]
fn sync_pose_writes_keep_their_order_through_replay() {
    let mut physics = due_physics();
    let first = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &OpenWorld,
        )
        .samples
        .remove(0);
    let poses = client_world::MovementFlagUpdate {
        gliding: Some(true),
        swimming: Some(true),
        crawling: Some(true),
        ..Default::default()
    };
    physics.apply_server_movement_flags(0, poses);
    physics.advance(
        simulation_clock::TICK_DURATION * 2,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    for newer_server_write in [false, true] {
        if newer_server_write {
            physics.apply_server_movement_flags(0, poses);
        }
        physics
            .apply_correction(
                crate::movement::PhysicsAnchor {
                    network_position: first.position,
                    tick: first.tick,
                    on_ground: false,
                    velocity: Some(first.velocity),
                },
                crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                None,
                &OpenWorld,
            )
            .unwrap();
        let flags = physics.prediction_sync_flags();
        assert_eq!(flags.gliding, Some(newer_server_write));
        assert_eq!(flags.swimming, Some(newer_server_write));
        assert_eq!(flags.crawling, Some(newer_server_write));
    }
}
