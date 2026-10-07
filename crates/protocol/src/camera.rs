//! Bounded, vendor-independent server camera ingress.

mod aim_assist;
mod aim_assist_types;
mod instruction;
mod presets;
mod spline;
mod types;

use crate::WorldPacketError;
use std::sync::Arc;

pub use aim_assist::{MAX_CAMERA_AIM_ASSIST_ENTRIES, camera_aim_assist_activation_packet};
pub(crate) use aim_assist::{
    normalize_actor_priorities, normalize_aim_presets, normalize_settings,
};
pub use aim_assist_types::{
    CameraAimAssistAction, CameraAimAssistActorPriority, CameraAimAssistCategory,
    CameraAimAssistExclusions, CameraAimAssistItemSetting, CameraAimAssistPreset,
    CameraAimAssistPresetSettings, CameraAimAssistPriorities, CameraAimAssistPriority,
    CameraAimAssistRegistry, CameraAimAssistSettings, CameraAimAssistTargetMode,
};
pub(crate) use instruction::normalize_instruction;
pub(crate) use presets::{normalize_presets, normalize_shake, normalize_switch};
pub(crate) use spline::normalize_registry;
pub use spline::{
    CameraSpline, CameraSplineInstruction, CameraSplineKind, CameraSplineProgressKeyFrame,
    CameraSplineRotationKeyFrame, MAX_CAMERA_SPLINE_POINTS,
};
pub use types::{
    CameraEase, CameraEvent, CameraFadeColor, CameraFadeInstruction, CameraFadeTimes,
    CameraFovInstruction, CameraInstructionEvent, CameraPreset, CameraSetInstruction,
    CameraShakeAction, CameraShakeEvent, CameraShakeType, CameraSwitchEvent,
    CameraTargetInstruction, MAX_CAMERA_EASE_IDENTIFIER_BYTES, MAX_CAMERA_PRESETS,
};

/// Rejects a vector containing a non-finite component.
fn validate_position(position: [f32; 3], field: &'static str) -> Result<(), WorldPacketError> {
    for value in position {
        validate_finite(value, field)?;
    }
    Ok(())
}

/// Keeps non-finite server values out of presentation state.
fn validate_finite(value: f32, field: &'static str) -> Result<(), WorldPacketError> {
    if !value.is_finite() {
        return Err(WorldPacketError::NonFiniteCameraField { field });
    }
    Ok(())
}

/// Bounds owned identifiers before presentation retains them.
fn bounded_identifier(value: String, field: &'static str) -> Result<Arc<str>, WorldPacketError> {
    if value.len() > MAX_CAMERA_EASE_IDENTIFIER_BYTES * 2 {
        return Err(WorldPacketError::CameraIdentifierTooLong {
            field,
            bytes: value.len(),
            max: MAX_CAMERA_EASE_IDENTIFIER_BYTES * 2,
        });
    }
    Ok(value.into())
}

/// Oversized but well-formed collections use the session's counted semantic skip path.
fn validate_count(count: usize, max: usize, field: &'static str) -> Result<(), WorldPacketError> {
    if count > max {
        return Err(WorldPacketError::CameraCollectionTooLarge { field, count, max });
    }
    Ok(())
}
