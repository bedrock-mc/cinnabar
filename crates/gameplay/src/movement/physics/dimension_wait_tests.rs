use super::*;
use crate::movement::{MovementSource, MovementTicker};

struct EmptyWorld;

impl CollisionWorld for EmptyWorld {
    fn collision_boxes(
        &self,
        _query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery {
            value: Vec::new(),
            identity: WorldCollisionIdentity::new(registry(), []).unwrap(),
        })
    }
}

fn registry() -> sim::CollisionRegistryIdentity {
    sim::CollisionRegistry::new().identity()
}

#[test]
fn loading_keeps_destination_stationary_and_input_ticks_contiguous() {
    let position = [4.5, 72.0 + PLAYER_NETWORK_OFFSET, -8.5];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(position, 100, false);
    physics.queue_server_motion([0.3, -0.8, 0.5], 0);
    let mut ticker = MovementTicker::default();
    ticker.reset(9, 100, position);
    ticker.set_source(MovementSource::Physics);
    let context = PhysicsSampleContext {
        raw_move_vector: [1.0, 1.0],
        analogue_move_vector: [1.0, 1.0],
        mode_intent: ModeIntent {
            can_fly: true,
            ..ModeIntent::default()
        },
        ..PhysicsSampleContext::default()
    };
    for _ in 0..20 {
        let frame = physics.advance_dimension_wait(
            Duration::from_millis(25),
            0.0,
            context,
            registry(),
            &mut NoMovementEffects,
        );
        assert!(frame.blocked.is_none());
        assert_eq!(frame.dropped_ticks, 0);
        for sample in frame.samples {
            assert_eq!(sample.position, position);
            assert_eq!(sample.velocity, [0.0; 3]);
            assert_eq!(sample.movement, [0.0; 3]);
            assert!(sample.world_identity.chunks.is_empty());
            ticker.enqueue_completed_physics(sample).unwrap();
        }
        assert_eq!(physics.network_position(), Some(position));
        assert_eq!(physics.state().unwrap().velocity, Vec3::ZERO);
    }
    let snapshots = ticker.pending_snapshots();
    assert_eq!(snapshots.len(), 10);
    for (index, snapshot) in snapshots.iter().enumerate() {
        assert_eq!(snapshot.tick, 101 + index as u64);
        assert_eq!(snapshot.position, position);
        assert_eq!(snapshot.delta, [0.0; 3]);
        assert_eq!(snapshot.move_vector, [0.0; 2]);
        assert_eq!(snapshot.raw_move_vector, [0.0; 2]);
        assert_eq!(snapshot.analogue_move_vector, [0.0; 2]);
    }
    assert_eq!(physics.state().unwrap().tick, 110);
    assert!(physics.history.is_empty());
}

#[test]
fn physics_resumes_immediately_after_loading_ends() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.5, 72.0 + PLAYER_NETWORK_OFFSET, 0.5], 100, false);
    physics.advance_dimension_wait(
        Duration::from_millis(50),
        0.0,
        PhysicsSampleContext::default(),
        registry(),
        &mut NoMovementEffects,
    );
    let resumed = physics.advance(
        Duration::from_millis(50),
        physics_movement_input([0.0, 1.0], 0.0, true, false, false, false, None),
        &EmptyWorld,
    );
    assert!(resumed.blocked.is_none());
    assert_eq!(resumed.samples.len(), 1);
    assert_eq!(resumed.samples[0].tick, 102);
    assert!(
        resumed.samples[0].velocity[1] < 0.0,
        "gravity resumes on the first released tick"
    );
    assert!(resumed.samples[0].movement[2] > 0.0);
    assert!(!physics.dimension_waiting);
    assert_eq!(physics.history.len(), 1);
}

#[test]
fn loading_discards_pre_anchor_time_once_and_preserves_fractional_ticks_on_release() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position_before_advance(
        [4.5, 72.0 + PLAYER_NETWORK_OFFSET, -8.5],
        200,
        false,
    );
    let discard = physics.advance_dimension_wait(
        Duration::from_secs(2),
        0.0,
        PhysicsSampleContext::default(),
        registry(),
        &mut NoMovementEffects,
    );
    assert_eq!(discard.due_ticks, 0);
    assert!(discard.samples.is_empty());
    let partial = physics.advance_dimension_wait(
        Duration::from_millis(25),
        0.0,
        PhysicsSampleContext::default(),
        registry(),
        &mut NoMovementEffects,
    );
    assert!(partial.samples.is_empty());
    let resumed = physics.advance(
        Duration::from_millis(25),
        MovementInput::default(),
        &EmptyWorld,
    );
    assert!(resumed.blocked.is_none());
    assert_eq!(resumed.samples.len(), 1);
    assert_eq!(resumed.samples[0].tick, 201);
}

/// Loading drops movement requests and corrections from the discarded prediction history.
#[test]
fn loading_discards_previous_movement_and_deferred_corrections() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.5, 72.0 + PLAYER_NETWORK_OFFSET, 0.5], 100, false);
    let moved = physics.advance(
        Duration::from_millis(50),
        physics_movement_input([0.0, 1.0], 0.0, true, false, false, false, None),
        &EmptyWorld,
    );
    assert_eq!(moved.samples.len(), 1);
    assert_ne!(physics.state().unwrap().requested_movement, Vec3::ZERO);
    physics.defer_prediction_correction(crate::movement::PhysicsAnchor {
        network_position: [20.0, 80.0, 30.0],
        tick: 500,
        on_ground: true,
        velocity: Some([0.0; 3]),
    });
    assert!(physics.has_pending_prediction_corrections());
    physics.advance_dimension_wait(
        Duration::from_millis(50),
        0.0,
        PhysicsSampleContext::default(),
        registry(),
        &mut NoMovementEffects,
    );
    assert_eq!(physics.state().unwrap().requested_movement, Vec3::ZERO);
    assert!(!physics.has_pending_prediction_corrections());
}
