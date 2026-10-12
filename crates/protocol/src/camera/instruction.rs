//! Normalization of independent camera instruction options.

use super::*;
use crate::WorldPacketError;
use bedrock_protocol_1_26_51::types::{CameraInstructionData, CameraInstructionSet};
use std::sync::Arc;

/// Keeps each option independent until camera evaluation applies it in vanilla order.
pub(crate) fn normalize_instruction(
    instruction: CameraInstructionData,
) -> Result<CameraEvent, WorldPacketError> {
    let spline = instruction
        .spline
        .map(super::spline::normalize_instruction)
        .transpose()?;
    let set = instruction.set.map(normalize_set).transpose()?;
    if let Some(fade) = &instruction.fade
        && let Some(time) = &fade.time
    {
        validate_finite(time.fade_in_time.0, "fade.fade_in")?;
        validate_finite(time.hold_time.0, "fade.hold")?;
        validate_finite(time.fade_out_time.0, "fade.fade_out")?;
    }
    if let Some(fade) = &instruction.fade
        && let Some(color) = &fade.color
    {
        validate_finite(color.red.0, "fade.red")?;
        validate_finite(color.green.0, "fade.green")?;
        validate_finite(color.blue.0, "fade.blue")?;
    }
    if let Some(fov) = &instruction.field_of_view {
        validate_finite(fov.field_of_view.0, "fov.degrees")?;
        validate_finite(fov.fov_ease_time.0, "fov.ease_time")?;
        if fov.fov_ease_type.len() > MAX_CAMERA_EASE_IDENTIFIER_BYTES {
            return Err(WorldPacketError::CameraIdentifierTooLong {
                field: "fov.ease_type",
                bytes: fov.fov_ease_type.len(),
                max: MAX_CAMERA_EASE_IDENTIFIER_BYTES,
            });
        }
    }
    if let Some(target) = &instruction.target
        && let Some(offset) = &target.target_center_offset
    {
        validate_position([offset.x, offset.y, offset.z], "target.center_offset")?;
    }
    Ok(CameraEvent::Instruction(Box::new(CameraInstructionEvent {
        set,
        spline,
        clear: instruction.clear,
        fade: instruction.fade.map(|fade| CameraFadeInstruction {
            time: fade.time.map(|time| CameraFadeTimes {
                fade_in_seconds: time.fade_in_time.0,
                hold_seconds: time.hold_time.0,
                fade_out_seconds: time.fade_out_time.0,
            }),
            color: fade.color.map(|color| CameraFadeColor {
                red: color.red.0,
                green: color.green.0,
                blue: color.blue.0,
            }),
        }),
        target: instruction.target.map(|target| CameraTargetInstruction {
            center_offset: target.target_center_offset.map(|pos| [pos.x, pos.y, pos.z]),
            actor_unique_id: target.target_actor_id.0,
        }),
        remove_target: instruction.remove_target.unwrap_or(false),
        fov: instruction.field_of_view.map(|fov| CameraFovInstruction {
            degrees: fov.field_of_view.0,
            ease_time_seconds: fov.fov_ease_time.0,
            ease_type: Arc::from(fov.fov_ease_type.to_string_lossy()),
            clear: fov.field_of_view_clear,
        }),
        attach_to_entity: instruction.attach_to_entity.map(|attach| attach.0),
        detach_from_entity: instruction.detach_from_entity.unwrap_or(false),
    })))
}

/// Validates all supplied overrides before publishing a camera set.
fn normalize_set(set: CameraInstructionSet) -> Result<CameraSetInstruction, WorldPacketError> {
    if let Some(ease) = &set.ease {
        validate_finite(ease.time.0, "set.ease.time")?;
    }
    if let Some(pos) = &set.pos {
        validate_position([pos.pos.x, pos.pos.y, pos.pos.z], "set.position")?;
    }
    if let Some(rot) = &set.rot {
        validate_finite(rot.x.0, "set.rotation.pitch")?;
        validate_finite(rot.y.0, "set.rotation.yaw")?;
    }
    if let Some(facing) = &set.facing {
        validate_position([facing.pos.x, facing.pos.y, facing.pos.z], "set.facing")?;
    }
    if let Some(offset) = &set.view_offset {
        validate_finite(offset.x.0, "set.view_offset.x")?;
        validate_finite(offset.y.0, "set.view_offset.y")?;
    }
    if let Some(offset) = &set.entity_offset {
        validate_position(
            [
                offset.entity_offset_x.0,
                offset.entity_offset_y.0,
                offset.entity_offset_z.0,
            ],
            "set.entity_offset",
        )?;
    }
    Ok(CameraSetInstruction {
        preset_id: set.preset.0,
        ease: set.ease.map(|ease| CameraEase {
            kind: ease.type_.0,
            time_seconds: ease.time.0,
        }),
        position: set.pos.map(|pos| [pos.pos.x, pos.pos.y, pos.pos.z]),
        rotation_degrees: set.rot.map(|rot| [rot.x.0, rot.y.0]),
        facing_position: set
            .facing
            .map(|facing| [facing.pos.x, facing.pos.y, facing.pos.z]),
        view_offset: set.view_offset.map(|offset| [offset.x.0, offset.y.0]),
        entity_offset: set.entity_offset.map(|offset| {
            [
                offset.entity_offset_x.0,
                offset.entity_offset_y.0,
                offset.entity_offset_z.0,
            ]
        }),
        default_preset: set.default,
        remove_ignore_starting_values: set.remove_ignore_starting_values_component,
    })
}
