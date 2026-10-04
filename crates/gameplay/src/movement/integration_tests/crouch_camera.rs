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
        sneak_button: true,
        ..PhysicsSampleContext::default()
    };
    let first = physics.advance_with_context(Duration::from_millis(75), input, context, &Floor);
    assert!(first.blocked.is_none());
    assert_eq!(first.completed_ticks, 1);
    assert_eq!(physics.latest_sneak_sprint(), Some((true, false)));
    assert_eq!(first.samples[0].position, network);
    assert!(first.samples[0].processed.sneaking);
    assert!(first.samples[0].sneak_button);
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
    assert!(!frame.samples[0].sneak_button);
    assert!((physics.render_eye_position().unwrap()[1] - (standing - 0.0875)).abs() < 1.0e-6);
}
