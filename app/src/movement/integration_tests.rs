use std::time::{Duration, Instant};

use super::{
    LocalPhysicsController, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME, MovementOutboxReconciliation,
    MovementSendError, MovementSource, MovementTicker, OUTBOX_CAPACITY, PhysicsAuthorityFault,
    PhysicsAuthorityGate, PhysicsCollisionRegistries, PhysicsCorrectionMode,
    PhysicsCorrectionOutcome, PhysicsMovementSample, PhysicsSampleContext,
    PhysicsTickEvidenceContext, flush_player_auth_inputs, physics_movement_input,
    reconcile_candidate_physics_correction,
};
use assets::{BlockPhysicsFlags, NetworkIdMode, RegistryRecord, read_registry};
use protocol::{PlayerInputFlags, PlayerInputMode};
use sha2::{Digest, Sha256};
use sim::{
    Aabb, CollisionIdSpace, CollisionQuery, CollisionRegistryIdentity, CollisionWorld,
    MovementInput, Vec3, WorldCollisionIdentity, WorldQueryError,
};
use ui::UserSettings;

use crate::{
    acceptance::{AcceptanceRun, Phase3TerminalDrainDecision, TRANSPARENT_PRESENTATION_EXIT_GRACE},
    camera::CameraSettingsAuthority,
};

#[path = "transport_tests.rs"]
mod transport_tests;

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

fn completed_sample(tick: u64, position: [f32; 3]) -> PhysicsMovementSample {
    PhysicsMovementSample {
        tick,
        position,
        move_vector: [0.0, 1.0],
        pitch: 10.0,
        yaw: 20.0,
        head_yaw: 20.0,
        camera_orientation: [0.0, 0.0, 1.0],
        jumping: false,
        sneaking: false,
        sprinting: false,
        input_mode: PlayerInputMode::Mouse,
        grounded_before_tick: false,
        grounded_after_tick: false,
        jump_repeated: false,
        world_identity: fixture_world_identity(1),
    }
}

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

include!("integration_tests/basics.rs");
include!("integration_tests/replay_retry.rs");
include!("integration_tests/authority_reanchor.rs");
include!("integration_tests/simulation.rs");
