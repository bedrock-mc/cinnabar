use super::*;

/// Reads the reported size without consuming the pending sync.
fn reported_size(physics: &mut LocalPhysicsController, actor: &LocalActor) -> [f32; 3] {
    let mut size = None;
    send_movement_prediction_sync(physics, actor, None, None, None, |packet| {
        size = protocol::test_support::packets::prediction_sync_bounding_box(&packet);
        false
    });
    size.expect("due prediction sync")
}

/// A current-tick correction retains the resize accepted after its captured tick.
#[test]
fn pose_size_regression_current_correction_keeps_live_resize() {
    let mut physics = due_physics();
    let actor = local_actor();
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            gliding: Some(true),
            ..Default::default()
        },
    );
    let sample = physics
        .advance_with_context(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            crate::movement::PhysicsSampleContext {
                mode_intent: crate::movement::ModeIntent {
                    elytra_ready: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &OpenWorld,
        )
        .samples
        .remove(0);
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            gliding: Some(false),
            ..Default::default()
        },
    );
    let standing = [1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32];
    assert_eq!(reported_size(&mut physics, &actor), standing);
    for _ in 0..2 {
        let plan = physics
            .apply_correction(
                crate::movement::PhysicsAnchor {
                    network_position: sample.position,
                    tick: sample.tick,
                    on_ground: false,
                    velocity: Some(sample.velocity),
                },
                crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                None,
                &OpenWorld,
            )
            .unwrap();
        assert!(plan.replayed_samples.is_empty());
        assert_eq!(physics.prediction_sync_flags().gliding, Some(false));
        assert_eq!(reported_size(&mut physics, &actor), standing);
    }
}

/// Zero-tick corrections retain both explicit requests and later local ownership changes.
#[test]
fn pose_size_regression_current_correction_keeps_dimension_owner() {
    for height in [0.6, 1.5] {
        for horizontal in [false, true] {
            let mut physics = due_physics();
            let mut actor = local_actor();
            let height_field = protocol::ActorMetadata {
                key: ActorDataKey::Height.wire_value(),
                value: ActorMetadataValue::Float(height),
            };
            actor.0.explicit_dimensions.record(&height_field, 20);
            actor
                .0
                .metadata
                .insert(height_field.key, height_field.value);
            let sample = physics
                .advance_with_context(
                    simulation_clock::TICK_DURATION,
                    sim::MovementInput::default(),
                    crate::movement::PhysicsSampleContext {
                        mode_intent: crate::movement::ModeIntent {
                            explicit_dimensions: actor.0.explicit_dimensions,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    &OpenWorld,
                )
                .samples
                .remove(0);
            if horizontal {
                physics.apply_server_movement_flags(
                    0,
                    client_world::MovementFlagUpdate {
                        crawling: Some(true),
                        ..Default::default()
                    },
                );
            }
            let expected = [
                1.0,
                sim::PLAYER_WIDTH as f32,
                if horizontal {
                    sim::PLAYER_WIDTH as f32
                } else {
                    height
                },
            ];
            assert_eq!(physics.prediction_sync_explicit_size(), !horizontal);
            assert_eq!(reported_size(&mut physics, &actor), expected);
            for _ in 0..2 {
                physics
                    .apply_correction(
                        crate::movement::PhysicsAnchor {
                            network_position: sample.position,
                            tick: sample.tick,
                            on_ground: false,
                            velocity: Some(sample.velocity),
                        },
                        crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                        None,
                        &OpenWorld,
                    )
                    .unwrap();
                assert_eq!(
                    physics.prediction_sync_explicit_size(),
                    !horizontal,
                    "height={height}, horizontal={horizontal}",
                );
                assert_eq!(reported_size(&mut physics, &actor), expected);
            }
        }
    }
}
