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
            physics.note_server_dimensions(actor.0.explicit_dimensions);
            let sample = physics
                .advance_with_context(
                    simulation_clock::TICK_DURATION,
                    sim::MovementInput::default(),
                    crate::movement::PhysicsSampleContext::default(),
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

/// Server poses and correction replay retain the level's crouching dimensions.
#[test]
fn server_pose_resize_and_replay_keep_the_level_sneak_profile() {
    for version in ["1.19.80", "1.20.10"] {
        let mut physics = due_physics();
        physics.set_base_game_version(protocol::BaseGameVersion::parse(version));
        let actor = local_actor();
        let crouch = sim::MovementInput {
            sneaking: true,
            ..Default::default()
        };
        let anchor = physics
            .advance(simulation_clock::TICK_DURATION, crouch, &OpenWorld)
            .samples
            .remove(0);
        let expected = [
            1.0,
            sim::PLAYER_WIDTH as f32,
            sim::MovementMode::Walking.hitbox_height(true, physics.sneak_profile()) as f32,
        ];
        assert_eq!(reported_size(&mut physics, &actor), expected);
        physics.apply_server_movement_flags(
            0,
            client_world::MovementFlagUpdate {
                gliding: Some(true),
                swimming: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(
            reported_size(&mut physics, &actor),
            [1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_WIDTH as f32]
        );
        physics.advance(simulation_clock::TICK_DURATION, crouch, &OpenWorld);
        assert_eq!(physics.mode(), sim::MovementMode::Walking);
        assert_eq!(reported_size(&mut physics, &actor), expected);
        let eye = physics.render_eye_position();
        for _ in 0..2 {
            let replay = physics
                .apply_correction(
                    crate::movement::PhysicsAnchor {
                        network_position: anchor.position,
                        tick: anchor.tick,
                        on_ground: false,
                        velocity: Some(anchor.velocity),
                    },
                    crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                    None,
                    &OpenWorld,
                )
                .unwrap();
            assert_eq!(replay.replayed_samples.len(), 1);
            assert_eq!(reported_size(&mut physics, &actor), expected);
            assert_eq!(physics.render_eye_position(), eye);
        }
    }
}
