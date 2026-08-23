//! Server-correction and replay semantics for retained prediction state.
//!
//! Split from `integration_tests` to keep each test module inside the
//! architecture policy line limit.

use std::time::Duration;

use super::integration_tests::{VersionedWall, evidence_context, forward_physics_input};
use super::{
    CORRECTION_TELEPORT_DISPLACEMENT_BLOCKS, CorrectionShape, LocalPhysicsController,
    MovementSource, MovementTicker, PhysicsCorrectionMode, PhysicsCorrectionOutcome,
    PhysicsSampleContext, flush_player_auth_inputs, reconcile_candidate_physics_correction,
    reconcile_committed_correction,
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
    // Correction suites assert byte-level transport behavior that is
    // orthogonal to the provisional spawn-settle window.
    ticker.testing_lift_spawn_settle_gate();
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
    ticker.testing_lift_spawn_settle_gate();
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

    // The replacement StartGame anchors a fresh settle episode; lift it so
    // the bounded-history accounting below keeps its original meaning.
    ticker.testing_lift_spawn_settle_gate();
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

/// Protocol 2168 carries no correction shape field, so the client derives one.
/// An exactly matching server position with an agreeing ground flag confirms
/// the prediction; anything else within the displacement bound replays, and a
/// larger displacement snaps through the teleport anchor path.
#[test]
fn correction_shapes_classify_from_exact_agreement_then_displacement() {
    let world = VersionedWall(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    physics.advance_with_context(
        Duration::from_millis(100),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    let network_position = physics.network_position().unwrap();
    let on_ground = physics.state().unwrap().on_ground;

    assert_eq!(
        physics.correction_shape(network_position, on_ground),
        CorrectionShape::Confirmed,
        "the server echoing the exact predicted position confirms it"
    );
    // A ground-flag disagreement is real information: it must replay instead
    // of being silently absorbed by the confirming path.
    assert_eq!(
        physics.correction_shape(network_position, !on_ground),
        CorrectionShape::Replay
    );
    let mut nearby = network_position;
    nearby[0] += 0.001;
    assert_eq!(
        physics.correction_shape(nearby, on_ground),
        CorrectionShape::Replay
    );
    let mut distant = network_position;
    distant[2] += CORRECTION_TELEPORT_DISPLACEMENT_BLOCKS + 1.0;
    assert_eq!(
        physics.correction_shape(distant, on_ground),
        CorrectionShape::TeleportSnap
    );
    // Non-finite anchors cannot be reconciled spatially and fail toward the
    // bounded teleport path, whose hard reanchor rejects them closed.
    assert_eq!(
        physics.correction_shape([f32::NAN; 3], on_ground),
        CorrectionShape::TeleportSnap
    );
}

#[test]
fn a_rotation_only_correction_leaves_prediction_history_and_outbox_untouched() {
    let world = VersionedWall(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    physics.queue_server_motion([0.75, 4.5, -0.25]);
    let frame = physics.advance_with_context(
        Duration::from_millis(100),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);

    let state_before = physics.state().unwrap().clone();
    let history_before = physics.history_len();
    let network_position = physics.network_position().unwrap();
    let on_ground = state_before.on_ground;
    let mut ticker = ticker_with_samples(frame.samples.clone());
    let next_tick_before = ticker.next_tick();

    let reconciled = reconcile_committed_correction(
        &mut ticker,
        &mut physics,
        network_position,
        state_before.tick,
        on_ground,
        &world,
    )
    .expect("a confirming correction cannot fault");

    assert_eq!(reconciled, None, "nothing spatial was applied");
    assert_eq!(physics.state().unwrap(), &state_before);
    assert_eq!(physics.history_len(), history_before);
    assert_eq!(ticker.next_tick(), next_tick_before);
    assert_eq!(
        ticker.pending_count(),
        frame.samples.len(),
        "the outbound stream stays contiguous"
    );
}

#[test]
fn knockback_overlays_evolve_identically_through_a_confirming_correction() {
    let build_twin = |world: &VersionedWall| {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
        physics.advance_with_context(
            Duration::from_millis(100),
            forward_physics_input(),
            PhysicsSampleContext::default(),
            world,
        );
        // Pending for the next tick: a newer impulse supersedes nothing here.
        physics.queue_server_motion([0.5, 6.0, 0.125]);
        physics
    };
    let world = VersionedWall(1);
    let mut corrected = build_twin(&world);
    let mut untouched = build_twin(&world);
    let network_position = corrected.network_position().unwrap();
    let tick = corrected.state().unwrap().tick;
    let on_ground = corrected.state().unwrap().on_ground;

    let reconciled = reconcile_committed_correction(
        &mut ticker_with_samples([]),
        &mut corrected,
        network_position,
        tick,
        on_ground,
        &world,
    )
    .expect("a confirming correction cannot fault");
    assert_eq!(reconciled, None);

    let corrected_frame = corrected.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    let untouched_frame = untouched.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );

    assert_eq!(corrected.state(), untouched.state());
    assert_eq!(
        corrected_frame.samples.last().map(|sample| sample.velocity),
        untouched_frame.samples.last().map(|sample| sample.velocity),
        "the queued impulse must drive the next tick exactly as without the correction"
    );
}

#[test]
fn a_teleport_shaped_correction_clears_queued_knockback_overlays() {
    let world = VersionedWall(1);
    let build_twin = |world: &VersionedWall| {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
        let frame = physics.advance_with_context(
            Duration::from_millis(100),
            forward_physics_input(),
            PhysicsSampleContext::default(),
            world,
        );
        let ticker = ticker_with_samples(frame.samples.iter().cloned());
        (physics, ticker)
    };
    let (mut overlaid, mut overlaid_ticker) = build_twin(&world);
    let (mut plain, mut plain_ticker) = build_twin(&world);
    // Pending for the next tick when no correction intervenes.
    overlaid.queue_server_motion([0.5, 6.0, 0.125]);

    let mut distant = overlaid.network_position().unwrap();
    distant[2] += CORRECTION_TELEPORT_DISPLACEMENT_BLOCKS + 1.0;
    let tick = overlaid.state().unwrap().tick;
    assert_eq!(
        overlaid.correction_shape(distant, false),
        CorrectionShape::TeleportSnap
    );
    reconcile_committed_correction(
        &mut overlaid_ticker,
        &mut overlaid,
        distant,
        tick,
        false,
        &world,
    )
    .unwrap()
    .expect("teleport-shaped corrections apply");
    reconcile_committed_correction(&mut plain_ticker, &mut plain, distant, tick, false, &world)
        .unwrap()
        .expect("the overlay-free twin snaps identically");

    let overlaid_frame = overlaid.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    let plain_frame = plain.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );

    assert_eq!(
        overlaid.state(),
        plain.state(),
        "the snap must clear the queued impulse exactly as if it never existed"
    );
    assert_eq!(
        overlaid_frame.samples.last().map(|sample| sample.velocity),
        plain_frame.samples.last().map(|sample| sample.velocity)
    );
}

#[test]
fn nearby_corrections_replay_and_distant_ones_snap_like_the_teleport_anchor_path() {
    let world = VersionedWall(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(100),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    let retained_tick = frame.samples.last().unwrap().tick;
    let mut ticker = ticker_with_samples(frame.samples.iter().cloned());

    // The retained-tick small/full path keeps its established semantics:
    // replace position+ground, retain velocity/movement/jump, replay later
    // inputs.
    let mut nearby = physics.network_position().unwrap();
    nearby[0] += 0.001;
    assert_eq!(
        physics.correction_shape(nearby, true),
        CorrectionShape::Replay
    );
    let replay_outcome = reconcile_committed_correction(
        &mut ticker,
        &mut physics,
        nearby,
        retained_tick,
        true,
        &world,
    )
    .unwrap()
    .expect("nearby corrections apply");
    assert!(matches!(
        replay_outcome,
        PhysicsCorrectionOutcome::Replayed { .. }
    ));

    // Beyond the displacement bound there is no retained input script that can
    // reproduce the server position, so the existing teleport anchor semantics
    // apply: snap, clear bounded outbound state, engage the settle window.
    let mut distant = physics.network_position().unwrap();
    distant[2] += CORRECTION_TELEPORT_DISPLACEMENT_BLOCKS + 1.0;
    assert_eq!(
        physics.correction_shape(distant, false),
        CorrectionShape::TeleportSnap
    );
    let snap_outcome = reconcile_committed_correction(
        &mut ticker,
        &mut physics,
        distant,
        retained_tick,
        false,
        &world,
    )
    .unwrap()
    .expect("teleport-shaped corrections apply");
    assert_eq!(
        snap_outcome,
        PhysicsCorrectionOutcome::Snapped {
            tick: retained_tick
        }
    );
    assert_eq!(
        ticker.pending_count(),
        0,
        "the snap clears bounded outbound prediction state"
    );
}
