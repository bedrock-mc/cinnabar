use super::*;

/// A valid received current remains reportable even outside the simulation budget.
#[test]
fn sync_regression_reports_large_accepted_speed() {
    let mut speed = super::super::super::LocalMovementSpeedAuthority::default();
    speed.begin_session(1, 0);
    assert!(speed.apply(
        1,
        1,
        0,
        client_world::MovementSpeedAttribute::unmodified(0.1)
    ));
    speed.apply(
        1,
        2,
        0,
        client_world::MovementSpeedAttribute::unmodified(33.0),
    );
    let mut actor = local_actor();
    actor
        .0
        .attributes
        .get_mut(Attribute::Movement.wire_name())
        .unwrap()
        .current = 33.0;
    let mut physics = due_physics();
    send_movement_prediction_sync(
        &mut physics,
        &actor,
        None,
        speed.reported_current(),
        None,
        |packet| {
            let attributes =
                protocol::test_support::packets::prediction_sync_attributes(&packet).unwrap();
            assert_eq!(attributes[0], 33.0);
            true
        },
    );
}

/// Explicit sizes remain accepted even when they equal a familiar pose height.
#[test]
fn sync_regression_preserves_explicit_pose_sized_dimensions() {
    for scale in [1.0, 2.0] {
        for height in [0.6, 1.5] {
            let mut physics = due_physics();
            physics.advance(
                std::time::Duration::from_millis(50),
                sim::MovementInput::default(),
                &OpenWorld,
            );
            let mut actor = local_actor();
            for (key, value) in [
                (ActorDataKey::Scale, scale),
                (ActorDataKey::Width, 0.6),
                (ActorDataKey::Height, height),
            ] {
                actor
                    .0
                    .metadata
                    .insert(key.wire_value(), ActorMetadataValue::Float(value));
            }
            actor.0.explicit_dimensions_revision = Some(20);
            send_movement_prediction_sync(&mut physics, &actor, None, None, None, |packet| {
                assert_eq!(
                    protocol::test_support::packets::prediction_sync_bounding_box(&packet),
                    Some([scale, 0.6, height])
                );
                true
            });
        }
    }
}

/// Inactive local use does not clear an accepted server use flag.
#[test]
fn sync_regression_preserves_server_item_use() {
    let mut actor = local_actor();
    actor.0.metadata.insert(
        ActorDataKey::Flags.wire_value(),
        ActorMetadataValue::Flags(1 << ActorFlag::UsingItem.wire_value()),
    );
    let mut physics = due_physics();
    send_movement_prediction_sync(&mut physics, &actor, None, None, None, |packet| {
        assert!(flag_set(
            &protocol::test_support::packets::prediction_sync_flags(&packet).unwrap(),
            ActorFlag::UsingItem
        ));
        true
    });
}

/// A server flag write remains observable before another local tick runs.
#[test]
fn sync_keeps_server_pose_writes_between_ticks() {
    for (flag, update) in [
        (
            ActorFlag::Gliding,
            client_world::MovementFlagUpdate {
                gliding: Some(true),
                ..Default::default()
            },
        ),
        (
            ActorFlag::Swimming,
            client_world::MovementFlagUpdate {
                swimming: Some(true),
                ..Default::default()
            },
        ),
        (
            ActorFlag::Crawling,
            client_world::MovementFlagUpdate {
                crawling: Some(true),
                ..Default::default()
            },
        ),
    ] {
        let mut actor = local_actor();
        let mut physics = due_physics();
        physics.advance(
            std::time::Duration::from_millis(50),
            sim::MovementInput::default(),
            &OpenWorld,
        );
        let (word, mask) = flag_slot(flag);
        let key = [
            ActorDataKey::Flags,
            ActorDataKey::FlagsTwo,
            ActorDataKey::FlagsThird,
        ][word];
        actor
            .0
            .metadata
            .insert(key.wire_value(), ActorMetadataValue::FlagsExtended(mask));
        physics.apply_server_movement_flags(0, update);
        let mut sent = None;
        send_movement_prediction_sync(&mut physics, &actor, None, None, None, |packet| {
            sent = Some(packet);
            true
        });
        let words = protocol::test_support::packets::prediction_sync_flags(
            &sent.expect("due correction sync"),
        )
        .expect("prediction sync packet");
        assert!(flag_set(&words, flag), "server-set {flag:?} was lost");
    }
}

/// A locally controlled mount preserves the passenger's cached Moving flag.
#[test]
fn sync_regression_controlled_mount_retains_moving() {
    let mut physics = due_physics();
    physics.advance(
        std::time::Duration::from_millis(100),
        sim::MovementInput::default(),
        &OpenWorld,
    );
    assert!(physics.prediction_sync_flags().moving == Some(true));
    let frame = physics.advance_with_context(
        std::time::Duration::from_millis(50),
        sim::MovementInput::default(),
        crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                ride: Some(crate::movement::RideKind::Boat),
                vehicle_controlled_locally: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(frame.completed_ticks, 1);
    assert!(
        physics.prediction_sync_flags().moving == Some(true),
        "mount replaced cached Moving"
    );
}

/// Reads a due packet while leaving it pending for another observation.
fn sync_packet(physics: &mut LocalPhysicsController, actor: &LocalActor) -> protocol::Packet {
    let mut sent = None;
    send_movement_prediction_sync(physics, actor, None, None, None, |packet| {
        sent = Some(packet);
        false
    });
    sent.expect("pending correction sync")
}

/// Server poses remain independent, and sparse later clears replace only supplied bits.
#[test]
fn sync_preserves_independent_server_pose_combinations() {
    let mut physics = due_physics();
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    let actor = local_actor();
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            gliding: Some(true),
            swimming: Some(true),
            crawling: Some(true),
            ..Default::default()
        },
    );
    let packet = sync_packet(&mut physics, &actor);
    let words = protocol::test_support::packets::prediction_sync_flags(&packet).unwrap();
    for flag in [ActorFlag::Gliding, ActorFlag::Swimming, ActorFlag::Crawling] {
        assert!(flag_set(&words, flag));
    }
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            swimming: Some(false),
            ..Default::default()
        },
    );
    let words =
        protocol::test_support::packets::prediction_sync_flags(&sync_packet(&mut physics, &actor))
            .unwrap();
    assert!(flag_set(&words, ActorFlag::Gliding));
    assert!(!flag_set(&words, ActorFlag::Swimming));
    assert!(flag_set(&words, ActorFlag::Crawling));
}

/// A server collision or movement write survives until its local producer next runs.
#[test]
fn sync_orders_server_collision_and_movement_against_ticks() {
    let mut physics = due_physics();
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            moving: Some(false),
            has_collision: Some(false),
            ..Default::default()
        },
    );
    let actor = local_actor();
    let words =
        protocol::test_support::packets::prediction_sync_flags(&sync_packet(&mut physics, &actor))
            .unwrap();
    assert!(!flag_set(&words, ActorFlag::Moving));
    assert!(!flag_set(&words, ActorFlag::HasCollision));
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    let words =
        protocol::test_support::packets::prediction_sync_flags(&sync_packet(&mut physics, &actor))
            .unwrap();
    assert!(flag_set(&words, ActorFlag::Moving));
    assert!(flag_set(&words, ActorFlag::HasCollision));
}

/// A later pose transition replaces an explicit size, independently of numerical equality.
#[test]
fn sync_replaces_explicit_dimensions_on_a_later_local_resize() {
    let mut physics = due_physics();
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    let mut actor = local_actor();
    actor.0.metadata.insert(
        ActorDataKey::Height.wire_value(),
        ActorMetadataValue::Float(0.6),
    );
    actor.0.explicit_dimensions_revision = Some(20);
    physics.note_server_dimensions(Some(20));
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    assert_eq!(
        protocol::test_support::packets::prediction_sync_bounding_box(&sync_packet(
            &mut physics,
            &actor
        )),
        Some([1.0, 0.6, 0.6])
    );
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(
        protocol::test_support::packets::prediction_sync_bounding_box(&sync_packet(
            &mut physics,
            &actor
        )),
        Some([
            1.0,
            0.6,
            sim::MovementMode::Walking.hitbox_height(true) as f32
        ])
    );
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    assert_eq!(
        protocol::test_support::packets::prediction_sync_bounding_box(&sync_packet(
            &mut physics,
            &actor
        )),
        Some([1.0, 0.6, 1.8])
    );
}

/// Controlled passengers keep both cached values and later server writes through replay and snaps.
#[test]
fn sync_mount_cache_survives_server_updates_and_corrections() {
    for moving in [false, true] {
        let mut physics = due_physics();
        let first = physics
            .advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                &OpenWorld,
            )
            .samples
            .remove(0);
        let context = crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                ride: Some(crate::movement::RideKind::Boat),
                vehicle_controlled_locally: true,
                ..Default::default()
            },
            ..Default::default()
        };
        physics.apply_server_movement_flags(
            0,
            client_world::MovementFlagUpdate {
                moving: Some(moving),
                ..Default::default()
            },
        );
        for _ in 0..2 {
            physics.advance_with_context(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                context,
                &OpenWorld,
            );
            assert_eq!(physics.prediction_sync_flags().moving, Some(moving));
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
        assert_eq!(physics.prediction_sync_flags().moving, Some(moving));
        physics.apply_server_movement_flags(
            0,
            client_world::MovementFlagUpdate {
                moving: Some(!moving),
                ..Default::default()
            },
        );
        physics.reanchor_network_position([0.5, 72.0, 0.5], 200, false);
        assert_eq!(physics.prediction_sync_flags().moving, Some(!moving));
        physics.advance_with_context(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            context,
            &OpenWorld,
        );
        assert_eq!(physics.prediction_sync_flags().moving, Some(!moving));
        physics.advance(
            simulation_clock::TICK_DURATION * 2,
            sim::MovementInput::default(),
            &OpenWorld,
        );
        assert_eq!(physics.prediction_sync_flags().moving, Some(true));
    }
}

/// An uncontrolled passenger refreshes Moving from displacement even when riding a boat.
#[test]
fn sync_uncontrolled_mount_refreshes_moving() {
    let mut physics = due_physics();
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            moving: Some(true),
            ..Default::default()
        },
    );
    physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                ride: Some(crate::movement::RideKind::Boat),
                ..Default::default()
            },
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(physics.prediction_sync_flags().moving, Some(false));
}

/// Dimensions supplied at spawn keep their ownership through an unchanged first tick.
#[test]
fn sync_preserves_explicit_spawn_dimensions() {
    let mut physics = due_physics();
    let mut actor = local_actor();
    actor.0.metadata.insert(
        ActorDataKey::Height.wire_value(),
        ActorMetadataValue::Float(1.5),
    );
    actor.0.explicit_dimensions_revision = Some(1);
    physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                dimensions_revision: Some(1),
                ..Default::default()
            },
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(
        protocol::test_support::packets::prediction_sync_bounding_box(&sync_packet(
            &mut physics,
            &actor
        )),
        Some([1.0, 0.6, 1.5])
    );
}

/// Horizontal flags supplied at spawn still size a definition-driven player before local adoption.
#[test]
fn sync_sizes_independent_horizontal_metadata_flags() {
    for flag in HORIZONTAL_POSE_FLAGS {
        let mut physics = due_physics();
        physics.advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &OpenWorld,
        );
        let mut actor = local_actor();
        let (word, mask) = flag_slot(flag);
        actor.0.metadata.insert(
            [
                ActorDataKey::Flags,
                ActorDataKey::FlagsTwo,
                ActorDataKey::FlagsThird,
            ][word]
                .wire_value(),
            ActorMetadataValue::FlagsExtended(mask),
        );
        let update = match flag {
            ActorFlag::Gliding => client_world::MovementFlagUpdate {
                gliding: Some(true),
                ..Default::default()
            },
            ActorFlag::Swimming => client_world::MovementFlagUpdate {
                swimming: Some(true),
                ..Default::default()
            },
            ActorFlag::Crawling => client_world::MovementFlagUpdate {
                crawling: Some(true),
                ..Default::default()
            },
            ActorFlag::DamageNearbyMobs => client_world::MovementFlagUpdate {
                spinning: Some(true),
                ..Default::default()
            },
            _ => unreachable!("horizontal flag inventory"),
        };
        physics.apply_server_movement_flags(0, update);
        assert_eq!(
            protocol::test_support::packets::prediction_sync_bounding_box(&sync_packet(
                &mut physics,
                &actor
            )),
            Some([1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_WIDTH as f32])
        );
    }
}

/// Flight descent uses the sneak input lane without requesting a crouching box.
#[test]
fn sync_flight_descent_keeps_the_standing_size() {
    let mut physics = due_physics();
    physics.advance_with_context(
        simulation_clock::TICK_DURATION * 2,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                can_fly: true,
                flight: client_world::game_mode_capabilities::FlightAuthority::Fixed(true),
                ..Default::default()
            },
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(physics.mode(), sim::MovementMode::Flying);
    assert_eq!(
        protocol::test_support::packets::prediction_sync_bounding_box(&sync_packet(
            &mut physics,
            &local_actor()
        )),
        Some([1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32])
    );
}

/// Each accepted pose is reported until its own local trigger ends it.
#[test]
fn sync_combined_pose_writes_follow_independent_local_stops() {
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
            ..Default::default()
        },
    );
    let words =
        protocol::test_support::packets::prediction_sync_flags(&sync_packet(&mut physics, &actor))
            .unwrap();
    assert!(flag_set(&words, ActorFlag::Gliding));
    assert!(flag_set(&words, ActorFlag::Swimming));
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    let packet = sync_packet(&mut physics, &actor);
    let words = protocol::test_support::packets::prediction_sync_flags(&packet).unwrap();
    assert!(!flag_set(&words, ActorFlag::Gliding));
    assert!(!flag_set(&words, ActorFlag::Swimming));
    assert_eq!(
        protocol::test_support::packets::prediction_sync_bounding_box(&packet),
        Some([1.0, 0.6, 1.8])
    );
}

/// An echo of an older capture does not replace a newer live pose in the packet.
#[test]
fn sync_pose_echo_uses_the_effective_live_word() {
    let mut physics = due_physics();
    let actor = local_actor();
    let context = crate::movement::PhysicsSampleContext {
        mode_intent: crate::movement::ModeIntent {
            elytra_ready: true,
            ..Default::default()
        },
        ..Default::default()
    };
    for jumping in [false, true] {
        physics.advance_with_context(
            simulation_clock::TICK_DURATION,
            sim::MovementInput {
                jumping,
                ..Default::default()
            },
            context,
            &OpenWorld,
        );
    }
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    physics.apply_server_movement_flags(
        100,
        client_world::MovementFlagUpdate {
            gliding: Some(false),
            ..Default::default()
        },
    );
    let words =
        protocol::test_support::packets::prediction_sync_flags(&sync_packet(&mut physics, &actor))
            .unwrap();
    assert!(flag_set(&words, ActorFlag::Gliding));
}
