//! Camera preset, legacy request, and shake normalization.

use super::*;
use crate::WorldPacketError;
use bedrock_protocol_1_26_51::enums::{
    CameraPresetAudioListener, CameraShakeAction as WireShakeAction,
    CameraShakeType as WireShakeType, ControlScheme,
};
use bedrock_protocol_1_26_51::packets::{Camera, CameraPresets, CameraShake};
use bedrock_protocol_1_26_51::wire;
use std::sync::Arc;

/// Unwraps an optional little-endian float field.
fn float(value: Option<wire::F32LE>) -> Option<f32> {
    value.map(|value| value.0)
}

/// Retains preset indices and rejects unusable registry data through counted semantic skips.
pub(crate) fn normalize_presets(packet: CameraPresets) -> Result<CameraEvent, WorldPacketError> {
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
            let position = [
                float(preset.pos_x),
                float(preset.pos_y),
                float(preset.pos_z),
            ];
            let rotation_degrees = [float(preset.rot_x), float(preset.rot_y)];
            let radius = float(preset.radius);
            let rotation_speed = float(preset.rotation_speed);
            let block_listening_radius = float(preset.block_listening_radius);
            let yaw_limit_min = float(preset.yaw_limit_min);
            let yaw_limit_max = float(preset.yaw_limit_max);
            for value in position
                .into_iter()
                .chain(rotation_degrees)
                .chain([
                    radius,
                    rotation_speed,
                    block_listening_radius,
                    yaw_limit_min,
                    yaw_limit_max,
                ])
                .flatten()
            {
                validate_finite(value, "preset.transform")?;
            }
            for limits in [
                preset.horizontal_rotation_limit.as_ref(),
                preset.vertical_rotation_limit.as_ref(),
                preset.starting_rotation.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
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
                rotation_speed,
                snap_to_target: preset.snap_to_target,
                horizontal_rotation_limit: preset.horizontal_rotation_limit.map(|v| [v.x, v.y]),
                vertical_rotation_limit: preset.vertical_rotation_limit.map(|v| [v.x, v.y]),
                continue_targeting: preset.continue_targeting,
                block_listening_radius,
                view_offset,
                entity_offset,
                radius,
                yaw_limit_min,
                yaw_limit_max,
                listener: preset.listener.map(|listener| match listener {
                    CameraPresetAudioListener::Camera => 0,
                    CameraPresetAudioListener::Player => 1,
                    CameraPresetAudioListener::Unknown(value) => value,
                }),
                player_effects: preset.player_effects,
                apply_inherited_starting_rotation: preset.apply_inherited_starting_rotation,
                starting_rotation: preset.starting_rotation.map(|v| [v.x, v.y]),
                control_scheme: preset.control_scheme.map(|scheme| match scheme {
                    ControlScheme::LockedPlayerRelativeStrafe => 0,
                    ControlScheme::CameraRelative => 1,
                    ControlScheme::CameraRelativeStrafe => 2,
                    ControlScheme::PlayerRelative => 3,
                    ControlScheme::PlayerRelativeStrafe => 4,
                    ControlScheme::Unknown(value) => value,
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
pub(crate) fn normalize_switch(packet: Camera) -> CameraEvent {
    CameraEvent::Switch(CameraSwitchEvent {
        camera_unique_id: packet.camera_id.0,
        target_player_unique_id: packet.target_player_id.0,
    })
}

/// Non-finite shake fields are counted semantic skips.
pub(crate) fn normalize_shake(packet: CameraShake) -> Result<CameraEvent, WorldPacketError> {
    validate_finite(packet.intensity.0, "shake.intensity")?;
    validate_finite(packet.seconds.0, "shake.seconds")?;
    Ok(CameraEvent::Shake(CameraShakeEvent {
        intensity: packet.intensity.0,
        duration_seconds: packet.seconds.0,
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
