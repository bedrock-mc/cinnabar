//! External placement cannot carry contact evidence from the previous location.

use super::*;

/// A wall produces real contacts; the separate destination is climbable without a wall.
struct ClimbableDestination(VersionedWall);

impl CollisionWorld for ClimbableDestination {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.0.collision_boxes(query)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = self.0.block_physics(block)?;
        if block[2] >= 4 {
            sample.layers[0].flags = sim::BlockPhysicsFlags::CLIMBABLE;
        }
        Ok(sample)
    }
}

/// Exercises live travel or a rewind that reconstructs the wall contact before placement.
fn actor_move_from_wall_to_unobstructed_climbable(replay: bool) {
    let world = ClimbableDestination(VersionedWall(1));
    let (mut physics, frame) = collided_prediction(&world);
    let before = physics.state().unwrap().clone();
    assert!(before.velocity.y <= 0.0);
    let destination = [0.0, 3.0 + protocol::PLAYER_NETWORK_OFFSET, 8.0];
    physics.apply_server_actor_position(destination, Some(false));
    assert_eq!(physics.state().unwrap().velocity, before.velocity);
    assert!(!physics.state().unwrap().on_ground);
    assert_eq!(physics.state().unwrap().tick, before.tick);
    let next = physics.advance(Duration::from_millis(50), Default::default(), &world);
    assert!(next.blocked.is_none(), "{:?}", next.blocked);
    assert_eq!(next.samples.len(), 1);
    if replay {
        let mut ticker = ticker_with_samples(frame.samples.iter().chain(&next.samples).cloned());
        let anchor = &frame.samples[0];
        let mut corrected = anchor.position;
        corrected[0] += 0.01;
        assert!(matches!(
            crate::movement::reconcile_prediction_correction(
                &mut ticker,
                &mut physics,
                corrected,
                anchor.tick,
                anchor.grounded_after_tick,
                anchor.velocity,
                &world,
            )
            .unwrap(),
            Some(PhysicsCorrectionOutcome::Replayed { .. })
        ));
        assert!(ticker.physics_is_authorized());
    }
    let sample = physics.sample_at(before.tick + 1).unwrap();
    assert!(!sample.horizontal_collision);
    assert!(
        sample.movement[1] <= 0.0,
        "the old wall contact invented an ascent after placement: {:?}",
        sample.movement
    );
    assert!(sample.velocity[1] < 0.0);
    assert!(sample.position[1] <= destination[1]);
    assert_eq!(physics.state().unwrap().tick, before.tick + 1);
}

#[test]
fn actor_move_live_travel_discards_previous_position_wall_contacts() {
    actor_move_from_wall_to_unobstructed_climbable(false);
}

#[test]
fn actor_move_replay_discards_reconstructed_previous_position_wall_contacts() {
    actor_move_from_wall_to_unobstructed_climbable(true);
}
