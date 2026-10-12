//! Normalizers for server packets that move or correct actors and the local player.

use bedrock_protocol_1_26_51::enums::MovementEffectType;
use bedrock_protocol_1_26_51::packets::{
    CorrectPlayerMovePrediction, MovePlayer, MovementEffect, SetActorMotion,
};

use super::{
    ActorMotionEvent, MovePlayerEvent, MovePlayerMode, MovementCorrectionSubject,
    MovementEffectEvent, MovementEffectKind, PlayerMovementCorrectionEvent, WorldPacketError,
};

/// A MovePlayer with any non-finite position or rotation component is a semantic rejection.
pub(super) fn normalize_move_player(
    packet: &MovePlayer,
) -> Result<MovePlayerEvent, WorldPacketError> {
    // gophertunnel packet/move_player.go writes Pitch then Yaw as one Vec2 of (pitch, yaw).
    let position = packet.position.to_array();
    let (pitch, yaw) = (packet.rotation.x, packet.rotation.y);
    let head_yaw = packet.y_head_rotation.0;
    for (field, value) in [
        ("position x", position[0]),
        ("position y", position[1]),
        ("position z", position[2]),
        ("pitch", pitch),
        ("yaw", yaw),
        ("head yaw", head_yaw),
    ] {
        if !value.is_finite() {
            return Err(WorldPacketError::NonFiniteMovePlayerField { field });
        }
    }
    let mode = MovePlayerMode::from(packet.position_mode);
    Ok(MovePlayerEvent {
        runtime_id: packet.player_runtime_id.0,
        position,
        pitch,
        yaw,
        head_yaw,
        mode,
        on_ground: packet.on_ground,
        teleported: mode.is_teleport(),
        source_tick: packet.tick.0,
        riding_runtime_id: packet.riding_runtime_id.0,
        teleport_cause: packet
            .teleport_data
            .as_ref()
            .map(|data| data.teleportation_cause.0),
    })
}

/// A well-formed impulse with non-finite components cannot enter prediction, so the packet is
/// skipped instead of clamped.
pub(super) fn normalize_actor_motion(packet: &SetActorMotion) -> Option<ActorMotionEvent> {
    let motion = packet.motion.to_array();
    if motion.iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some(ActorMotionEvent {
        actor_runtime_id: packet.target_runtime_id.0,
        motion,
        tick: packet.tick.0,
    })
}

pub(super) fn normalize_movement_effect(packet: &MovementEffect) -> MovementEffectEvent {
    MovementEffectEvent {
        actor_runtime_id: packet.target_runtime_id.0,
        kind: match packet.effect_id {
            MovementEffectType::GlideBoost => MovementEffectKind::GlideBoost,
            MovementEffectType::DolphinBoost => MovementEffectKind::DolphinBoost,
            MovementEffectType::GeyserBoost => MovementEffectKind::GeyserBoost,
            MovementEffectType::Unknown(value) => MovementEffectKind::Unknown(value),
        },
        duration_ticks: packet.effect_duration.0,
        tick: packet.tick.0,
    }
}

/// A correction whose velocity or rotation is non-finite is skipped like SetActorMotion. The raw
/// position is exempt: downstream resolution applies its documented sentinel recovery.
pub(super) fn normalize_correction(
    packet: &CorrectPlayerMovePrediction,
) -> Option<PlayerMovementCorrectionEvent> {
    let delta = packet.pos_delta.to_array();
    // gophertunnel packet/correct_player_move_prediction.go writes Rotation as (pitch, yaw).
    let (pitch, yaw) = (packet.rotation.x, packet.rotation.y);
    if delta.iter().any(|value| !value.is_finite()) || !pitch.is_finite() || !yaw.is_finite() {
        return None;
    }
    Some(PlayerMovementCorrectionEvent {
        position: packet.pos.to_array(),
        delta,
        pitch,
        yaw,
        subject: MovementCorrectionSubject::from(packet.prediction_type),
        on_ground: packet.on_ground,
        tick: packet.tick.0,
    })
}
