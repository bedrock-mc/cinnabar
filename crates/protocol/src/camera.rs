//! Bounded, vendor-independent server camera ingress.
//!
//! The 2168 wire carries legacy preset switches (73), shake commands (159),
//! and the modern instruction union (300). Preset-definition and aim-assist
//! registries stay outside this surface: they are dropped before decode, so no
//! unbounded registry payload is ever allocated.

use std::sync::Arc;

use valentine::bedrock::version::v1_26_44::{
    CameraInstruction, CameraPacket, CameraShakePacket, EnumsCameraShakeAction as WireShakeAction,
    EnumsCameraShakeType as WireShakeType,
};

use crate::WorldPacketError;

/// Maximum UTF-8 bytes retained for one camera easing identifier.
///
/// This is an allocation-safety ceiling, not an identifier allowlist.
pub const MAX_CAMERA_EASE_IDENTIFIER_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum CameraEvent {
    Switch(CameraSwitchEvent),
    Instruction(CameraInstructionEvent),
    Shake(CameraShakeEvent),
}

/// One legacy CameraPacket: switches the target player's camera to a
/// server-chosen camera entity or preset unique id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraSwitchEvent {
    pub camera_unique_id: i64,
    pub target_player_unique_id: i64,
}

/// One instruction packet reduced to the options it actually carries.
///
/// Every field mirrors its wire option; absent options stay `None`/`false`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraInstructionEvent {
    pub set: Option<CameraSetInstruction>,
    pub clear: Option<bool>,
    pub fade: Option<CameraFadeInstruction>,
    pub target: Option<CameraTargetInstruction>,
    pub remove_target: bool,
    pub fov: Option<CameraFovInstruction>,
    pub attach_to_entity: Option<i64>,
    pub detach_from_entity: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraSetInstruction {
    pub preset_id: u32,
    /// Raw wire easing selector; unknown values are retained verbatim.
    pub ease: Option<CameraEase>,
    pub position: Option<[f32; 3]>,
    pub rotation_degrees: Option<[f32; 2]>,
    pub facing_position: Option<[f32; 3]>,
    pub view_offset: Option<[f32; 2]>,
    pub entity_offset: Option<[f32; 3]>,
    pub default_preset: Option<bool>,
    pub remove_ignore_starting_values: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraEase {
    /// Raw wire easing selector; unknown values are retained verbatim.
    pub kind: u8,
    pub time_seconds: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFadeInstruction {
    pub time: Option<CameraFadeTimes>,
    pub color: Option<CameraFadeColor>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFadeTimes {
    pub fade_in_seconds: f32,
    pub hold_seconds: f32,
    pub fade_out_seconds: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFadeColor {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraTargetInstruction {
    pub center_offset: Option<[f32; 3]>,
    pub actor_unique_id: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraFovInstruction {
    pub degrees: f32,
    pub ease_time_seconds: f32,
    pub ease_type: Arc<str>,
    pub clear: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraShakeEvent {
    pub intensity: f32,
    pub duration_seconds: f32,
    pub shake_type: CameraShakeType,
    pub action: CameraShakeAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraShakeType {
    Positional,
    Rotational,
    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraShakeAction {
    Add,
    Stop,
    Unknown(u8),
}

pub(crate) fn normalize_switch(packet: CameraPacket) -> CameraEvent {
    CameraEvent::Switch(CameraSwitchEvent {
        camera_unique_id: packet.camera_id.actor_unique_id,
        target_player_unique_id: packet.target_player_id.actor_unique_id,
    })
}

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

pub(crate) fn normalize_instruction(
    instruction: CameraInstruction,
) -> Result<CameraEvent, WorldPacketError> {
    if instruction.spline.is_some() {
        return Err(WorldPacketError::UnsupportedCameraSpline);
    }
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
    Ok(CameraEvent::Instruction(CameraInstructionEvent {
        set,
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
    }))
}

fn normalize_set(
    set: valentine::bedrock::version::v1_26_44::CameraInstructionOptionsSetInstruction,
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

fn validate_position(position: [f32; 3], field: &'static str) -> Result<(), WorldPacketError> {
    for value in position {
        validate_finite(value, field)?;
    }
    Ok(())
}

fn validate_finite(value: f32, field: &'static str) -> Result<(), WorldPacketError> {
    if !value.is_finite() {
        return Err(WorldPacketError::NonFiniteCameraField { field });
    }
    Ok(())
}
