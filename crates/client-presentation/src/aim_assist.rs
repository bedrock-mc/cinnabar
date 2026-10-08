//! Server-owned aim-assist settings, registries and allocation-free target evaluation.

mod blocks;
mod frame;
mod registry;
mod selection;

pub use frame::AimAssistFrame;
pub use registry::{AimAssistCategory, ServerAimAssist};
pub use selection::{
    AimAssistCandidate, AimAssistFrustum, AimAssistTarget, TargetKind, target_score,
};

/// Omitted preset IDs select the registered vanilla default.
pub const DEFAULT_AIM_ASSIST_PRESET: &str = "minecraft:aim_assist_default";

/// Actor-data keys used by the server's resolved aim-assist priority table.
pub const AIM_ASSIST_PRESET_METADATA_KEY: u32 = 136;
pub const AIM_ASSIST_CATEGORY_METADATA_KEY: u32 = 137;
pub const AIM_ASSIST_ACTOR_METADATA_KEY: u32 = 138;

/// Camera control schemes, independent of the physical input device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AimAssistControlScheme {
    LockedPlayerRelativeStrafe = 0,
    CameraRelative = 1,
    CameraRelativeStrafe = 2,
    PlayerRelative = 3,
    PlayerRelativeStrafe = 4,
}

impl AimAssistControlScheme {
    /// Unknown schemes cannot select an entry from the native action-rotation matrix.
    #[must_use]
    pub const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::LockedPlayerRelativeStrafe),
            1 => Some(Self::CameraRelative),
            2 => Some(Self::CameraRelativeStrafe),
            3 => Some(Self::PlayerRelative),
            4 => Some(Self::PlayerRelativeStrafe),
            _ => None,
        }
    }
}

/// Projectile actions may rotate the player; aim assist never adds continuous look input.
#[must_use]
pub fn rotates_player_on_projectile(camera: &str, scheme: AimAssistControlScheme) -> bool {
    match camera {
        "minecraft:free" | "minecraft:fixed_boom" => true,
        "minecraft:follow_orbit" => matches!(
            scheme,
            AimAssistControlScheme::CameraRelative | AimAssistControlScheme::PlayerRelative
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
