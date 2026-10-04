//! Shared movement fixtures for mining and transport-order integration tests.

use crate::movement::{
    MovementSource, MovementTicker, PhysicsMovementSample, PhysicsTickEvidenceContext,
    ProcessedMovementState,
};
use protocol::PlayerInputMode;

/// Builds one completed, stationary sample with a synthetic collision identity.
pub fn completed(tick: u64) -> PhysicsMovementSample {
    PhysicsMovementSample {
        tick,
        position: [0.5, 2.620_01, 0.5],
        movement: [0.0; 3],
        velocity: [0.0; 3],
        move_vector: [0.0; 2],
        raw_move_vector: [0.0; 2],
        analogue_move_vector: [0.0; 2],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        camera_orientation: [0.0, 0.0, -1.0],
        jumping: false,
        sneaking: false,
        sneak_button: false,
        sprinting: false,
        input_mode: PlayerInputMode::Mouse,
        grounded_before_tick: true,
        grounded_after_tick: true,
        horizontal_collision: false,
        vertical_collision: false,
        jump_repeated: false,
        processed: ProcessedMovementState::default(),
        world_identity: sim::CollisionQuery::synthetic(()).identity,
    }
}

/// Supplies the publication context shared by transport ordering witnesses.
pub fn evidence() -> PhysicsTickEvidenceContext {
    PhysicsTickEvidenceContext {
        fifo_sequence: 19,
        pose_generation: 23,
        dimension: 0,
        perspective: semantic_input::PerspectiveMode::FirstPerson,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: false,
        look_delta: [0.0; 2],
        outbound_authorized: true,
        outbox_depth: 1,
        outbox_drops: 0,
        free_camera_packet_count: 0,
    }
}

/// A physics-authorized ticker holding `ticks` unsent samples from tick 101.
pub fn ticker_with_ticks(ticks: u64) -> MovementTicker {
    let (epoch, _) = tokio::sync::watch::channel(0);
    let mut ticker = MovementTicker::with_epoch_publisher(epoch);
    ticker.reset(7, 100, [0.5, 2.620_01, 0.5]);
    ticker.set_source(MovementSource::Physics);
    for tick in 101..101 + ticks {
        ticker.enqueue_completed_physics(completed(tick)).unwrap();
    }
    ticker
}
