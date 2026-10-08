//! Server camera events owned independently of the wire codec.

use super::{
    CameraAimAssistActorPriority, CameraAimAssistPresetSettings, CameraAimAssistRegistry,
    CameraAimAssistSettings, CameraSpline, CameraSplineInstruction,
};
use std::sync::Arc;

/// Maximum UTF-8 bytes retained for one camera easing identifier.
///
/// This is an allocation-safety ceiling, not an identifier allowlist.
pub const MAX_CAMERA_EASE_IDENTIFIER_BYTES: usize = 64;

/// Maximum retained camera presets; preset ids index this list, so extras past it are dropped.
pub const MAX_CAMERA_PRESETS: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub enum CameraEvent {
    /// The server's preset registry; a preset id in `set` indexes it.
    Presets(Arc<[CameraPreset]>),
    Switch(CameraSwitchEvent),
    /// Boxed: an instruction is several times larger than any other camera event.
    Instruction(Box<CameraInstructionEvent>),
    Shake(CameraShakeEvent),
    Splines(Arc<[CameraSpline]>),
    AimAssist(CameraAimAssistSettings),
    AimAssistPresets(CameraAimAssistRegistry),
    AimAssistActorPriority(Arc<[CameraAimAssistActorPriority]>),
}

/// Server camera capabilities and overrides; normalization rejects non-finite fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraPreset {
    pub name: Arc<str>,
    pub inherit_from: Arc<str>,
    pub position: [Option<f32>; 3],
    /// Pitch then yaw in degrees.
    pub rotation_degrees: [Option<f32>; 2],
    pub rotation_speed: Option<f32>,
    pub snap_to_target: Option<bool>,
    pub horizontal_rotation_limit: Option<[f32; 2]>,
    pub vertical_rotation_limit: Option<[f32; 2]>,
    pub continue_targeting: Option<bool>,
    pub block_listening_radius: Option<f32>,
    pub view_offset: Option<[f32; 2]>,
    pub entity_offset: Option<[f32; 3]>,
    pub radius: Option<f32>,
    pub yaw_limit_min: Option<f32>,
    pub yaw_limit_max: Option<f32>,
    /// Raw audio listener selector: zero follows the camera and one follows the player.
    pub listener: Option<u8>,
    pub player_effects: Option<bool>,
    pub apply_inherited_starting_rotation: bool,
    pub starting_rotation: Option<[f32; 2]>,
    pub aim_assist: Option<CameraAimAssistPresetSettings>,
    /// Raw control-scheme selector; unknown values stay available for counted runtime skips.
    pub control_scheme: Option<u8>,
}

/// One legacy CameraPacket carrying two actor unique ids as named by the wire.
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
    pub spline: Option<CameraSplineInstruction>,
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
