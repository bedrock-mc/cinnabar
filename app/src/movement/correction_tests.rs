//! Server-correction and replay semantics for retained prediction state.
//!
//! Split from `integration_tests` to keep each test module inside the
//! architecture policy line limit.

use std::time::Duration;

use super::integration_tests::{VersionedWall, evidence_context, forward_physics_input};
use super::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsCorrectionMode,
    PhysicsSampleContext, flush_player_auth_inputs, reconcile_candidate_physics_correction,
};
use sim::{Aabb, BlockPhysicsSample, CollisionQuery, CollisionWorld, WorldQueryError};
use world::{ChunkCollisionRevision, ChunkKey};

fn collided_prediction(
    world: &impl CollisionWorld,
) -> (LocalPhysicsController, super::LocalPhysicsFrame) {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(400),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        world,
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    assert!(
        physics.state().unwrap().collisions.z,
        "the witness needs a retained horizontal collision"
    );
    (physics, frame)
}

fn ticker_with_samples(
    samples: impl IntoIterator<Item = super::PhysicsMovementSample>,
) -> MovementTicker {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    ticker
}

fn admit_all(ticker: &mut MovementTicker) -> Vec<super::PhysicsSendIdentity> {
    let mut identities = Vec::new();
    flush_player_auth_inputs(
        ticker,
        usize::MAX,
        Some(evidence_context()),
        |identity, _packet| {
            identities.push(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    identities
}

#[test]
fn newest_tick_position_correction_preserves_retained_momentum() {
    let world = VersionedWall(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(100),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    let retained = physics.state().unwrap().clone();
    assert!(retained.velocity.length_squared() > 0.0);
    let mut corrected_position = frame.samples.last().unwrap().position;
    corrected_position[0] += 0.001;
    let mut ticker = ticker_with_samples(frame.samples);

    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        corrected_position,
        retained.tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &world,
    )
    .unwrap();

    let corrected = physics.state().unwrap();
    assert_eq!(corrected.tick, retained.tick);
    assert_eq!(corrected.velocity, retained.velocity);
    assert_eq!(corrected.movement, retained.movement);
    assert_eq!(corrected.jump_delay, retained.jump_delay);
    assert_ne!(corrected.position, retained.position);
}

/// Axis collisions describe the motion that produced a position, so they cannot
/// be reconstructed from a corrected anchor alone. A server correction that
/// moves the player repudiates that motion, and now that retained collisions
/// gate the discrete ladder-climb branch, carrying stale flags across such a
/// correction would apply an upward impulse the server never sanctioned.
#[test]
fn a_position_changing_correction_clears_retained_axis_collisions() {
    let world = VersionedWall(1);
    let (physics, frame) = collided_prediction(&world);
    let corrected_tick = physics.state().unwrap().tick;
    for corrected in [[4.0, 2.620_01, 0.0], [0.0, 2.620_01, 4.0]] {
        let mut moved = physics.clone();
        let mut ticker = ticker_with_samples(frame.samples.clone());
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut moved,
            corrected,
            corrected_tick,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &world,
        )
        .unwrap();
        assert!(
            !moved.state().unwrap().collisions.z,
            "an X/Z-changing correction must not retain repudiated collisions"
        );
    }
}

/// A matching prediction is not server-confirmed until its packet has actually
/// crossed the transport boundary.
#[test]
fn a_predicted_but_unsent_tick_does_not_preserve_retained_axis_collisions() {
    let world = VersionedWall(1);
    let (mut physics, frame) = collided_prediction(&world);
    let corrected_tick = physics.state().unwrap().tick;
    let confirmed = frame
        .samples
        .iter()
        .find(|sample| sample.tick == corrected_tick)
        .expect("the correction tick is retained")
        .position;
    let mut ticker = ticker_with_samples(frame.samples);

    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        confirmed,
        corrected_tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &world,
    )
    .unwrap();

    assert!(
        !physics.state().unwrap().collisions.z,
        "a pending outbox sample is not proof that the server received the position"
    );
}

/// Exact sent position plus unchanged, currently queryable collision identity
/// confirms the motion that produced the retained flags.
#[test]
fn an_actually_sent_tick_with_unchanged_identity_preserves_retained_axis_collisions() {
    let world = ChunkVersionedWall(1);
    let (mut physics, frame) = collided_prediction(&world);
    let corrected_tick = physics.state().unwrap().tick;
    let confirmed = frame
        .samples
        .iter()
        .find(|sample| sample.tick == corrected_tick)
        .unwrap()
        .position;
    let mut ticker = ticker_with_samples(frame.samples);
    let identities = admit_all(&mut ticker);
    assert!(
        identities
            .into_iter()
            .all(|identity| ticker.acknowledge_physics_send(identity))
    );

    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        confirmed,
        corrected_tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &world,
    )
    .unwrap();

    assert!(
        physics.state().unwrap().collisions.z,
        "an exact actually-sent correction with current identity should preserve collision flags"
    );
}

#[test]
fn command_queue_admission_without_socket_send_does_not_confirm_collisions() {
    let world = ChunkVersionedWall(1);
    let (mut physics, frame) = collided_prediction(&world);
    let corrected_tick = physics.state().unwrap().tick;
    let confirmed = frame.samples.last().unwrap().position;
    let mut ticker = ticker_with_samples(frame.samples);
    admit_all(&mut ticker);

    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        confirmed,
        corrected_tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &world,
    )
    .unwrap();

    assert!(
        !physics.state().unwrap().collisions.z,
        "command admission is not proof of a successful downstream socket write"
    );
}

#[test]
fn socket_acknowledgements_are_current_session_unique_and_fifo() {
    let world = VersionedWall(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(Duration::from_millis(100), Default::default(), &world);
    let mut ticker = ticker_with_samples(frame.samples);
    let identities = admit_all(&mut ticker);
    assert_eq!(identities.len(), 2);

    assert!(
        !ticker.acknowledge_physics_send(identities[1]),
        "out-of-order acknowledgement must not skip the FIFO front"
    );
    let mut stale = identities[0];
    stale.session_generation = stale.session_generation.saturating_add(1);
    assert!(
        !ticker.acknowledge_physics_send(stale),
        "stale-session acknowledgement must be ignored"
    );
    assert!(ticker.acknowledge_physics_send(identities[0]));
    assert!(
        !ticker.acknowledge_physics_send(identities[0]),
        "duplicate acknowledgement must not confirm twice"
    );
    assert!(ticker.acknowledge_physics_send(identities[1]));
    assert_eq!(ticker.sent_physics_packet_count(), 2);
    assert_eq!(ticker.sent_history.len(), 2);
}

#[test]
fn newest_same_position_with_changed_identity_clears_retained_axis_collisions() {
    let predicted_world = ChunkVersionedWall(1);
    let (mut physics, frame) = collided_prediction(&predicted_world);
    let corrected_tick = physics.state().unwrap().tick;
    let confirmed = frame.samples.last().unwrap().position;
    let mut ticker = ticker_with_samples(frame.samples);
    let identities = admit_all(&mut ticker);
    assert!(
        identities
            .into_iter()
            .all(|identity| ticker.acknowledge_physics_send(identity))
    );

    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        confirmed,
        corrected_tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &ChunkVersionedWall(2),
    )
    .unwrap();

    assert!(
        !physics.state().unwrap().collisions.z,
        "a stale sent identity cannot authorize retained collisions"
    );
}

struct EvictedWorld;

impl CollisionWorld for EvictedWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Err(WorldQueryError::UnloadedChunk(ChunkKey::new(0, 0, 0)))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Err(WorldQueryError::UnloadedChunk(ChunkKey::new(0, 0, 0)))
    }
}

struct ChunkVersionedWall(u64);

impl ChunkVersionedWall {
    fn identity(&self) -> sim::WorldCollisionIdentity {
        let registry = VersionedWall(1)
            .block_physics([0, 0, 0])
            .unwrap()
            .identity
            .registry;
        sim::WorldCollisionIdentity::new(
            registry,
            [ChunkCollisionRevision {
                chunk: ChunkKey::new(0, 0, 0),
                revision: self.0,
            }],
        )
        .unwrap()
    }
}

impl CollisionWorld for ChunkVersionedWall {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let mut result = VersionedWall(1).collision_boxes(query)?;
        result.identity = self.identity();
        Ok(result)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = VersionedWall(1).block_physics(block)?;
        sample.identity = self.identity();
        Ok(sample)
    }
}

#[test]
fn unavailable_current_chunk_clears_retained_axis_collisions_without_disconnect() {
    let predicted_world = ChunkVersionedWall(1);
    let (mut physics, frame) = collided_prediction(&predicted_world);
    let corrected_tick = physics.state().unwrap().tick;
    let confirmed = frame.samples.last().unwrap().position;
    let mut ticker = ticker_with_samples(frame.samples);
    let identities = admit_all(&mut ticker);
    assert!(
        identities
            .into_iter()
            .all(|identity| ticker.acknowledge_physics_send(identity))
    );

    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        confirmed,
        corrected_tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &EvictedWorld,
    )
    .unwrap();

    assert!(ticker.physics_is_authorized());
    assert!(physics.is_active());
    assert!(!physics.state().unwrap().collisions.z);
}

struct ClimbableWall(VersionedWall);

impl CollisionWorld for ClimbableWall {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.0.collision_boxes(query)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = self.0.block_physics(block)?;
        sample.layers[0].flags = sim::BlockPhysicsFlags::CLIMBABLE;
        Ok(sample)
    }
}

#[test]
fn next_ladder_tick_cannot_consume_an_unsent_stale_horizontal_collision() {
    let world = ClimbableWall(VersionedWall(1));
    let (mut physics, frame) = collided_prediction(&world);
    let corrected_tick = physics.state().unwrap().tick;
    let confirmed = frame.samples.last().unwrap().position;
    let mut ticker = ticker_with_samples(frame.samples);

    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        confirmed,
        corrected_tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &world,
    )
    .unwrap();
    let corrected_y = physics.state().unwrap().position.y;
    let next = physics.advance(Duration::from_millis(50), Default::default(), &world);
    assert!(next.blocked.is_none(), "{:?}", next.blocked);
    assert!(
        physics.state().unwrap().position.y <= corrected_y,
        "stale horizontal collision produced an unauthorized ladder ascent"
    );
}

#[test]
fn transport_restore_does_not_confirm_a_tick_before_send_success() {
    let world = VersionedWall(1);
    let (mut physics, frame) = collided_prediction(&world);
    let corrected_tick = physics.state().unwrap().tick;
    let confirmed = frame.samples.last().unwrap().position;
    let mut ticker = ticker_with_samples(frame.samples);
    assert!(
        flush_player_auth_inputs(
            &mut ticker,
            usize::MAX,
            Some(evidence_context()),
            |_identity, _packet| Err("full"),
        )
        .is_err()
    );
    ticker.note_full_restore();

    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        confirmed,
        corrected_tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &world,
    )
    .unwrap();

    assert!(ticker.physics_is_authorized());
    assert!(!physics.state().unwrap().collisions.z);
}

#[test]
fn sent_confirmation_history_is_bounded_and_cleared_by_authority_boundaries() {
    let world = VersionedWall(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for _ in 0..=super::OUTBOX_CAPACITY {
        let sample = physics
            .advance(Duration::from_millis(50), Default::default(), &world)
            .samples
            .pop()
            .unwrap();
        ticker.enqueue_completed_physics(sample).unwrap();
        let identity = admit_all(&mut ticker).pop().unwrap();
        assert!(ticker.acknowledge_physics_send(identity));
        assert_eq!(ticker.take_tick_evidence().len(), 1);
    }
    assert_eq!(ticker.sent_history.len(), super::OUTBOX_CAPACITY);
    assert_eq!(ticker.sent_history.front().unwrap().tick, 102);

    ticker.set_source(MovementSource::FreeCamera);
    assert!(ticker.sent_history.is_empty());

    ticker.set_source(MovementSource::Physics);
    let sample = physics
        .advance(Duration::from_millis(50), Default::default(), &world)
        .samples
        .pop()
        .unwrap();
    ticker.enqueue_completed_physics(sample).unwrap();
    let identity = admit_all(&mut ticker).pop().unwrap();
    assert!(ticker.acknowledge_physics_send(identity));
    assert_eq!(ticker.sent_history.len(), 1);
    ticker.reset(
        8,
        physics.state().unwrap().tick,
        physics.network_position().unwrap(),
    );
    assert!(ticker.sent_history.is_empty());

    ticker.set_source(MovementSource::Physics);
    let sample = physics
        .advance(Duration::from_millis(50), Default::default(), &world)
        .samples
        .pop()
        .unwrap();
    ticker.enqueue_completed_physics(sample).unwrap();
    let identity = admit_all(&mut ticker).pop().unwrap();
    assert!(ticker.acknowledge_physics_send(identity));
    assert_eq!(ticker.sent_history.len(), 1);
    let next_tick = ticker.next_tick();
    ticker.reanchor_surface_spawn(
        physics.state().unwrap().tick,
        physics.network_position().unwrap(),
    );
    assert!(ticker.sent_history.is_empty());
    assert_eq!(
        ticker.next_tick(),
        next_tick,
        "surface-spawn reanchor invalidation must not alter scheduling"
    );
}
