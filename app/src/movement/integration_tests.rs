use std::time::{Duration, Instant};

use super::{
    LocalPhysicsController, MovementOutboxReconciliation, MovementSource, MovementTicker,
    PhysicsAuthorityGate, PhysicsCorrectionMode, PhysicsCorrectionOutcome, PhysicsMovementSample,
    PhysicsSampleContext, PhysicsTickEvidenceContext, ProcessedMovementState,
    flush_player_auth_inputs, reconcile_candidate_physics_correction,
};
use assets::{BlockPhysicsFlags, RegistryRecord};
use gameplay::movement::coordination::physics_authority_fault_for_frame;
use protocol::PlayerInputMode;
use sha2::{Digest, Sha256};
use sim::{
    Aabb, CollisionIdSpace, CollisionQuery, CollisionRegistryIdentity, CollisionWorld,
    MovementInput, Vec3, WorldCollisionIdentity, WorldQueryError,
};
use ui::UserSettings;

use crate::{
    acceptance::{AcceptanceRun, Phase3TerminalDrainDecision, TRANSPARENT_PRESENTATION_EXIT_GRACE},
    camera::CameraSettingsAuthority,
    environment::{WeatherState, WorldClock, replace_session},
    movement::reset_start_game_prediction,
};

#[path = "transport_tests.rs"]
mod transport_tests;

/// Supplies the immutable publication facts for a completed movement tick.
pub(super) fn evidence_context() -> PhysicsTickEvidenceContext {
    PhysicsTickEvidenceContext {
        fifo_sequence: 40,
        pose_generation: 101,
        dimension: 0,
        perspective: semantic_input::PerspectiveMode::FirstPerson,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: false,
        look_delta: [0.25, -0.5],
        outbound_authorized: true,
        outbox_depth: 1,
        outbox_drops: 0,
        free_camera_packet_count: 0,
    }
}

/// Supplies one versioned collision identity for the adapter fixtures.
fn fixture_world_identity(seed: u8) -> WorldCollisionIdentity {
    WorldCollisionIdentity::new(
        CollisionRegistryIdentity {
            protocol: 1001,
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: [seed; 32],
        },
        [],
    )
    .unwrap()
}

/// Builds a completed tick without a live world or transport.
fn completed_sample(tick: u64, position: [f32; 3]) -> PhysicsMovementSample {
    PhysicsMovementSample {
        tick,
        position,
        movement: [0.125, -0.0784, -0.25],
        velocity: [0.125, -0.0784, -0.25],
        move_vector: [0.0, 1.0],
        raw_move_vector: [0.0, 1.0],
        analogue_move_vector: [0.0, 1.0],
        pitch: 10.0,
        yaw: 20.0,
        head_yaw: 20.0,
        camera_orientation: [0.0, 0.0, 1.0],
        jumping: false,
        sneaking: false,
        input: Default::default(),
        sprinting: false,
        input_mode: PlayerInputMode::Mouse,
        grounded_before_tick: false,
        grounded_after_tick: false,
        horizontal_collision: false,
        vertical_collision: false,
        jump_repeated: false,
        processed: ProcessedMovementState::default(),
        world_identity: fixture_world_identity(1),
    }
}

/// Retains admitted future ticks so app adapters can exercise correction cancellation.
fn replay_with_admitted_future_ticks(
    mut ticker: MovementTicker,
) -> (
    MovementTicker,
    LocalPhysicsController,
    Vec<super::PhysicsSendIdentity>,
) {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    // Retry/cancellation suites assert byte-level transport behavior that is
    // orthogonal to the provisional spawn-settle window.
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    let mut confirmed = None;
    flush_player_auth_inputs(
        &mut ticker,
        1,
        Some(evidence_context()),
        |identity, _packet| {
            confirmed = Some(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert!(ticker.acknowledge_physics_send(confirmed.unwrap()));

    let mut admitted = Vec::new();
    flush_player_auth_inputs(
        &mut ticker,
        2,
        Some(evidence_context()),
        |identity, _packet| {
            admitted.push(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [0.25, 2.620_01, 0.0],
        101,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();
    (ticker, physics, admitted)
}

struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 1.0, 64.0));
        Ok(CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

pub(super) struct VersionedFloor(pub(super) u8);

impl CollisionWorld for VersionedFloor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 1.0, 64.0));
        Ok(CollisionQuery {
            value: floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
            identity: fixture_world_identity(self.0),
        })
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        let mut sample = Floor.block_physics(block)?;
        sample.identity = fixture_world_identity(self.0);
        Ok(sample)
    }
}

/// Supplies the forward input used by reconciliation fixtures.
pub(super) fn forward_physics_input() -> MovementInput {
    MovementInput {
        forward: 1.0,
        yaw_degrees: 180.0,
        ..MovementInput::default()
    }
}

/// Runs a fixed amount of simulation independently of render frame rate.
fn physics_after_one_second(frame_rate: u32) -> LocalPhysicsController {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);
    let mut elapsed = Duration::ZERO;
    for frame in 0..frame_rate {
        let delta = if frame + 1 == frame_rate {
            Duration::from_secs(1) - elapsed
        } else {
            Duration::from_secs_f64(1.0 / f64::from(frame_rate))
        };
        elapsed += delta;
        let result = physics.advance(delta, forward_physics_input(), &Floor);
        assert!(result.blocked.is_none());
    }
    physics
}

/// Encodes physics records bound to the checked-in block registry.
pub(super) fn synthetic_preg(breg: &[u8], records: &[RegistryRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PREG1001");
    bytes
        .extend_from_slice(&crate::asset_startup::active_content_registry_protocol().to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(records.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(breg));
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.push(u8::try_from(record.collision_seed.boxes.len()).unwrap());
        bytes.push(if record.collision_seed.boxes.is_empty() {
            BlockPhysicsFlags::PASSABLE.bits()
        } else {
            0
        });
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&60_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        for shape in &record.collision_seed.boxes {
            for coordinate in [
                shape.min_x,
                shape.min_y,
                shape.min_z,
                shape.max_x,
                shape.max_y,
                shape.max_z,
            ] {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
    }
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    bytes
}

#[derive(Default)]
struct DeferredCollisionWorld {
    available: std::cell::Cell<bool>,
}

impl CollisionWorld for DeferredCollisionWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        if !self.available.get() {
            return Err(WorldQueryError::UnloadedChunk(world::ChunkKey::new(
                0, 0, 0,
            )));
        }
        Floor.collision_boxes(query)
    }
}

use crate::runtime::phase3_evidence::Phase3EvidenceEmitter;
include!("integration_tests/basics.rs");
include!("integration_tests/replay_retry.rs");
include!("integration_tests/simulation.rs");
