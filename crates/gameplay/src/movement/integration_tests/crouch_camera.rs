use super::*;

#[test]
fn crouch_camera_uses_completed_stance_ticks_without_moving_the_network_anchor() {
    let mut physics = LocalPhysicsController::default();
    let network = [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    physics.reanchor_network_position(network, 0, true);
    let standing = physics.render_eye_position().unwrap()[1];
    let input = MovementInput {
        sneaking: true,
        ..MovementInput::default()
    };
    let context = PhysicsSampleContext {
        input: crate::movement::TickInput {
            sneak: semantic_input::ActionPhase {
                held: true,
                ..Default::default()
            },
            sneak_down: true,
            ..Default::default()
        },
        ..PhysicsSampleContext::default()
    };
    let first = physics.advance_with_context(Duration::from_millis(75), input, context, &Floor);
    assert!(first.blocked.is_none());
    assert_eq!(first.completed_ticks, 1);
    assert_eq!(physics.latest_sneak_sprint(), Some((true, false)));
    assert_eq!(first.samples[0].position, network);
    assert!(first.samples[0].processed.sneaking);
    assert!(first.samples[0].input.sneak.held);
    assert!((physics.render_eye_position().unwrap()[1] - (standing - 0.0875)).abs() < 1.0e-6);
    assert_eq!(physics.render_feet_position(), Some([0.0, 1.0, 0.0]));

    let held = physics.advance_with_context(Duration::from_millis(50), input, context, &Floor);
    assert_eq!(held.completed_ticks, 1);
    assert!((physics.render_eye_position().unwrap()[1] - (standing - 0.21875)).abs() < 1.0e-6);
    let release = physics.advance(Duration::from_millis(50), MovementInput::default(), &Floor);
    assert_eq!(release.completed_ticks, 1);
    assert_eq!(physics.latest_sneak_sprint(), Some((false, false)));
    assert!((physics.render_eye_position().unwrap()[1] - (standing - 0.196875)).abs() < 1.0e-6);
    assert_eq!(physics.render_feet_position(), Some([0.0, 1.0, 0.0]));

    physics.reanchor_network_position(network, 4, true);
    assert_eq!(physics.render_eye_position().unwrap()[1], standing);
}

#[test]
fn a_low_ceiling_keeps_the_eye_crouched_without_a_held_sneak_button() {
    struct LowCeiling;
    impl CollisionWorld for LowCeiling {
        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            let mut boxes = Floor.collision_boxes(query)?.value;
            let ceiling = Aabb::new(Vec3::new(-8.0, 2.6, -8.0), Vec3::new(8.0, 3.6, 8.0));
            if ceiling.intersects(query) {
                boxes.push(ceiling);
            }
            Ok(CollisionQuery::synthetic(boxes))
        }
    }
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0], 0, true);
    let standing = physics.render_eye_position().unwrap()[1];
    let frame = physics.advance(
        Duration::from_millis(75),
        MovementInput::default(),
        &LowCeiling,
    );
    assert!(frame.blocked.is_none());
    assert_eq!(frame.completed_ticks, 1);
    assert!(frame.samples[0].processed.forced_sneak);
    assert!(frame.samples[0].processed.sneaking);
    assert!(!frame.samples[0].input.sneak.held);
    assert!((physics.render_eye_position().unwrap()[1] - (standing - 0.0875)).abs() < 1.0e-6);
}

#[test]
fn unavailable_collision_does_not_repeat_an_airborne_crouch_eye_transition() {
    struct MissingTerrain;
    impl CollisionWorld for MissingTerrain {
        fn collision_boxes(
            &self,
            _query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Err(WorldQueryError::UnloadedChunk(world::ChunkKey::new(
                0, 0, 0,
            )))
        }
    }
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0], 0, true);
    let jump = MovementInput {
        jumping: true,
        ..Default::default()
    };
    assert_eq!(
        physics
            .advance(Duration::from_millis(50), jump, &Floor)
            .completed_ticks,
        1
    );
    let crouch = MovementInput {
        sneaking: true,
        ..Default::default()
    };
    let frame = physics.advance(Duration::from_millis(50), crouch, &Floor);
    assert_eq!(frame.completed_ticks, 1);
    assert!(!physics.state().unwrap().on_ground);
    let before = physics.state().unwrap().clone();
    assert_eq!(
        physics
            .advance(Duration::from_millis(50), crouch, &MissingTerrain)
            .completed_ticks,
        0
    );
    let held_eye = physics.render_eye_position().unwrap();
    let held_feet = physics.render_feet_position().unwrap();
    for _ in 0..12 {
        for _ in 0..5 {
            let frame = physics.advance(Duration::from_millis(10), crouch, &MissingTerrain);
            assert_eq!(frame.completed_ticks, 0);
            assert_eq!(physics.state(), Some(&before));
            assert_eq!(physics.render_feet_position(), Some(held_feet));
            assert_eq!(physics.render_eye_position(), Some(held_eye));
        }
    }
    let resumed = physics.advance(Duration::from_millis(50), crouch, &Floor);
    assert_eq!(resumed.completed_ticks, 1);
    assert_eq!(physics.state().unwrap().tick, before.tick + 1);
    assert_eq!(physics.render_eye_position(), Some(held_eye));
}

#[test]
fn unavailable_collision_holds_replayed_correction_and_crouch_release_until_resumed() {
    struct UnknownTerrain;
    impl CollisionWorld for UnknownTerrain {
        fn collision_boxes(
            &self,
            _query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Err(WorldQueryError::UnknownRuntimeId {
                runtime_id: 99,
                block: [0, 1, 0],
            })
        }
    }
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0], 0, true);
    let crouch = MovementInput {
        sneaking: true,
        ..Default::default()
    };
    physics.advance(Duration::from_millis(200), crouch, &Floor);
    let jumping = MovementInput {
        jumping: true,
        ..crouch
    };
    physics.advance(Duration::from_millis(50), jumping, &Floor);
    let latest = physics.sample_at(physics.state().unwrap().tick).unwrap();
    let mut corrected = latest.position;
    corrected[0] += 2.0;
    physics
        .apply_correction(
            crate::movement::PhysicsAnchor {
                network_position: corrected,
                tick: latest.tick,
                on_ground: latest.grounded_after_tick,
                velocity: Some(latest.velocity),
            },
            PhysicsCorrectionMode::ReplayIfRetained,
            None,
            &Floor,
        )
        .unwrap();
    let released = MovementInput::default();
    assert_eq!(
        physics
            .advance(Duration::from_millis(50), released, &Floor)
            .completed_ticks,
        1
    );
    let before = physics.state().unwrap().clone();
    physics.advance(Duration::from_millis(50), released, &UnknownTerrain);
    let feet = physics.render_feet_position();
    let eye = physics.render_eye_position();
    for _ in 0..20 {
        let frame = physics.advance(Duration::from_millis(10), released, &UnknownTerrain);
        assert_eq!(frame.completed_ticks, 0);
        assert_eq!(physics.state(), Some(&before));
        assert_eq!(physics.render_feet_position(), feet);
        assert_eq!(physics.render_eye_position(), eye);
    }
    let resumed = physics.advance(Duration::from_millis(50), released, &Floor);
    assert_eq!(resumed.completed_ticks, 1);
    assert_eq!(physics.render_feet_position(), feet);
    assert_eq!(physics.render_eye_position(), eye);
    physics.advance(Duration::from_millis(25), released, &Floor);
    assert_ne!(physics.render_feet_position(), feet);
    assert_ne!(physics.render_eye_position(), eye);
}
