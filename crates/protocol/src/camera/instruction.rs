//! Normalization of independent camera instruction options.

use super::*;
use crate::WorldPacketError;
use std::sync::Arc;
use valentine::bedrock::version::v1_26_51::CameraInstruction;

/// Keeps each option independent until camera evaluation applies it in vanilla order.
pub(crate) fn normalize_instruction(
    instruction: CameraInstruction,
) -> Result<CameraEvent, WorldPacketError> {
    let spline = instruction
        .spline
        .map(super::spline::normalize_instruction)
        .transpose()?;
    let set = instruction.set.map(normalize_set).transpose()?;
    if let Some(fade) = &instruction.fade
        && let Some(time) = &fade.time
    {
        validate_finite(time.fade_in_time, "fade.fade_in")?;
        validate_finite(time.hold_time, "fade.hold")?;
        validate_finite(time.fade_out_time, "fade.fade_out")?;
    }
    if let Some(fade) = &instruction.fade
        && let Some(color) = &fade.color
    {
        validate_finite(color.red, "fade.red")?;
        validate_finite(color.green, "fade.green")?;
        validate_finite(color.blue, "fade.blue")?;
    }
    if let Some(fov) = &instruction.field_of_view {
        validate_finite(fov.fieldof_view, "fov.degrees")?;
        validate_finite(fov.fov_ease_time, "fov.ease_time")?;
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
                fade_in_seconds: time.fade_in_time,
                hold_seconds: time.hold_time,
                fade_out_seconds: time.fade_out_time,
            }),
            color: fade.color.map(|color| CameraFadeColor {
                red: color.red,
                green: color.green,
                blue: color.blue,
            }),
        }),
        target: instruction.target.map(|target| CameraTargetInstruction {
            center_offset: target.target_center_offset.map(|pos| [pos.x, pos.y, pos.z]),
            actor_unique_id: target.target_actor_id,
        }),
        remove_target: instruction.remove_target.unwrap_or(false),
        fov: instruction.field_of_view.map(|fov| CameraFovInstruction {
            degrees: fov.fieldof_view,
            ease_time_seconds: fov.fov_ease_time,
            ease_type: Arc::from(fov.fov_ease_type),
            clear: fov.fieldof_view_clear,
        }),
        attach_to_entity: instruction
            .attach_to_entity
            .map(|attach| attach.entity_actor_id),
        detach_from_entity: instruction.detach_from_entity.unwrap_or(false),
    })))
}

/// Validates all supplied overrides before publishing a camera set.
fn normalize_set(
    set: valentine::bedrock::version::v1_26_51::CameraInstructionOptionsSetInstruction,
) -> Result<CameraSetInstruction, WorldPacketError> {
    if let Some(ease) = &set.ease {
        validate_finite(ease.time, "set.ease.time")?;
    }
    if let Some(pos) = &set.pos {
        validate_position([pos.pos.x, pos.pos.y, pos.pos.z], "set.position")?;
    }
    if let Some(rot) = &set.rot {
        validate_finite(rot.x, "set.rotation.pitch")?;
        validate_finite(rot.y, "set.rotation.yaw")?;
    }
    if let Some(facing) = &set.facing {
        validate_position([facing.pos.x, facing.pos.y, facing.pos.z], "set.facing")?;
    }
    if let Some(offset) = &set.view_offset {
        validate_finite(offset.x, "set.view_offset.x")?;
        validate_finite(offset.y, "set.view_offset.y")?;
    }
    if let Some(offset) = &set.entity_offset {
        validate_position(
            [
                offset.entity_offset_x,
                offset.entity_offset_y,
                offset.entity_offset_z,
            ],
            "set.entity_offset",
        )?;
    }
    Ok(CameraSetInstruction {
        preset_id: set.preset,
        ease: set.ease.map(|ease| CameraEase {
            kind: ease.type_,
            time_seconds: ease.time,
        }),
        position: set.pos.map(|pos| [pos.pos.x, pos.pos.y, pos.pos.z]),
        rotation_degrees: set.rot.map(|rot| [rot.x, rot.y]),
        facing_position: set
            .facing
            .map(|facing| [facing.pos.x, facing.pos.y, facing.pos.z]),
        view_offset: set.view_offset.map(|offset| [offset.x, offset.y]),
        entity_offset: set.entity_offset.map(|offset| {
            [
                offset.entity_offset_x,
                offset.entity_offset_y,
                offset.entity_offset_z,
            ]
        }),
        default_preset: set.default,
        remove_ignore_starting_values: set.remove_ignore_starting_values_component,
    })
}
