//! Camera preset, legacy request, and shake normalization.

use super::*;
use crate::WorldPacketError;
use std::sync::Arc;
use valentine::bedrock::version::v1_26_51::{
    CameraPacket, CameraPresetsPacket, CameraShakePacket,
    EnumsCameraShakeAction as WireShakeAction, EnumsCameraShakeType as WireShakeType,
};

/// Retains preset indices and rejects unusable registry data through counted semantic skips.
pub(crate) fn normalize_presets(
    packet: CameraPresetsPacket,
) -> Result<CameraEvent, WorldPacketError> {
    validate_count(
        packet.camera_presets.presets.len(),
        MAX_CAMERA_PRESETS,
        "presets",
    )?;
    let presets = packet
        .camera_presets
        .presets
        .into_iter()
        .map(|preset| {
            let position = [preset.pos_x, preset.pos_y, preset.pos_z];
            let rotation_degrees = [preset.rot_x, preset.rot_y];
            for value in position
                .into_iter()
                .chain(rotation_degrees)
                .chain([preset.radius, preset.rotation_speed, preset.block_listening_radius,
                    preset.yaw_limit_min, preset.yaw_limit_max])
                .flatten()
            {
                validate_finite(value, "preset.transform")?;
            }
            for limits in [preset.horizontal_rotation_limit.as_ref(), preset.vertical_rotation_limit.as_ref(), preset.starting_rotation.as_ref()].into_iter().flatten() {
                validate_finite(limits.x, "preset.rotation_limit")?;
                validate_finite(limits.y, "preset.rotation_limit")?;
            }
            let view_offset = preset.view_offset.map(|offset| [offset.x, offset.y]);
            if let Some(offset) = view_offset {
                for value in offset {
                    validate_finite(value, "preset.view_offset")?;
                }
            }
            let entity_offset = preset
                .entity_offset
                .map(|offset| [offset.x, offset.y, offset.z]);
            if let Some(offset) = entity_offset {
                validate_position(offset, "preset.entity_offset")?;
            }
            Ok(CameraPreset {
                name: bounded_identifier(preset.name, "preset.name")?,
                inherit_from: bounded_identifier(preset.inherit_from, "preset.inherit_from")?,
                position,
                rotation_degrees,
                rotation_speed: preset.rotation_speed,
                snap_to_target: preset.snapto_target,
                horizontal_rotation_limit: preset.horizontal_rotation_limit.map(|v| [v.x, v.y]),
                vertical_rotation_limit: preset.vertical_rotation_limit.map(|v| [v.x, v.y]),
                continue_targeting: preset.continue_targeting,
                block_listening_radius: preset.block_listening_radius,
                view_offset,
                entity_offset,
                radius: preset.radius,
                yaw_limit_min: preset.yaw_limit_min,
                yaw_limit_max: preset.yaw_limit_max,
                listener: preset.listener.map(|listener| {
                    use valentine::bedrock::version::v1_26_51::EnumsSharedTypesv12190CameraPresetAudioListener::*;
                    match listener { Camera => 0, Player => 1, Unknown(value) => value }
                }),
                player_effects: preset.player_effects,
                apply_inherited_starting_rotation: preset.apply_inherited_starting_rotation,
                starting_rotation: preset.starting_rotation.map(|v| [v.x, v.y]),
                control_scheme: preset.control_scheme.map(|scheme| {
                    use valentine::bedrock::version::v1_26_51::EnumsControlSchemeScheme::*;
                    match scheme {
                        LockedPlayerRelativeStrafe => 0,
                        CameraRelative => 1,
                        CameraRelativeStrafe => 2,
                        PlayerRelative => 3,
                        PlayerRelativeStrafe => 4,
                        Unknown(value) => value,
                    }
                }),
                aim_assist: preset
                    .aim_assist
                    .map(super::aim_assist::normalize_preset_settings)
                    .transpose()?,
            })
        })
        .collect::<Result<Arc<[_]>, WorldPacketError>>()?;
    Ok(CameraEvent::Presets(presets))
}

/// Preserves both unique IDs in the legacy camera request.
pub(crate) fn normalize_switch(packet: CameraPacket) -> CameraEvent {
    CameraEvent::Switch(CameraSwitchEvent {
        camera_unique_id: packet.camera_id.actor_unique_id,
        target_player_unique_id: packet.target_player_id.actor_unique_id,
    })
}

/// Non-finite shake fields are counted semantic skips.
pub(crate) fn normalize_shake(packet: CameraShakePacket) -> Result<CameraEvent, WorldPacketError> {
    validate_finite(packet.intensity, "shake.intensity")?;
    validate_finite(packet.seconds, "shake.seconds")?;
    Ok(CameraEvent::Shake(CameraShakeEvent {
        intensity: packet.intensity,
        duration_seconds: packet.seconds,
        shake_type: match packet.shake_type {
            WireShakeType::Positional => CameraShakeType::Positional,
            WireShakeType::Rotational => CameraShakeType::Rotational,
            WireShakeType::Unknown(value) => CameraShakeType::Unknown(value),
        },
        action: match packet.shake_action {
            WireShakeAction::Add => CameraShakeAction::Add,
            WireShakeAction::Stop => CameraShakeAction::Stop,
            WireShakeAction::Unknown(value) => CameraShakeAction::Unknown(value),
        },
    }))
}
