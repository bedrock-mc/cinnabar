//! Riptide launch and spin lifetime.

use super::{MovementInput, MovementMode, PlayerState};
use crate::math::{MIN_DIRECTION_LENGTH, minecraft_cos, minecraft_sin};
use serde::{Deserialize, Serialize};

/// A released trident's level and `[pitch, yaw]` in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiptideLaunch {
    level: u8,
    rotation: [f32; 2],
}

impl RiptideLaunch {
    /// Retains finite facing and a level whose impulse fits the collision-query budget.
    pub fn new(level: u8, rotation: [f32; 2]) -> Option<Self> {
        let launch = Self { level, rotation };
        launch.is_supported().then_some(launch)
    }

    /// Returns the released enchantment level for skipped-request diagnostics.
    pub fn level(self) -> u8 {
        self.level
    }

    /// Rejects unsupported releases even when deserialization bypassed the constructor.
    pub(super) fn is_supported(self) -> bool {
        self.level > 0
            && self.rotation.into_iter().all(f32::is_finite)
            && f64::from(self.speed()) <= crate::MAX_COLLISION_QUERY_EXTENT - crate::PLAYER_HEIGHT
    }

    /// Returns the additive impulse magnitude before ground and water adjustment.
    fn speed(self) -> f32 {
        (f32::from(self.level) + 1.0) * 0.25 * 3.0
    }
}

/// Starts a released trident's spin and adds its impulse when movement is allowed.
pub(super) fn launch(state: &mut PlayerState, input: MovementInput, shallow_water: bool) {
    let Some(launch) = input.riptide.filter(|launch| launch.is_supported()) else {
        return;
    };
    if input.mode == MovementMode::Riding || input.spectator {
        return;
    }
    state.spin_attack_ticks = Some(0);
    if input.immobile {
        return;
    }
    let [pitch, yaw] = launch.rotation.map(f32::to_radians);
    let sin = |angle: f32| minecraft_sin(f64::from(angle)) as f32;
    let cos = |angle: f32| minecraft_cos(f64::from(angle)) as f32;
    let x = -sin(yaw) * cos(pitch);
    let y = -sin(pitch);
    let z = cos(yaw) * cos(pitch);
    let length = (z * z + y * y + x * x).sqrt();
    let [x, y, z] = if length >= MIN_DIRECTION_LENGTH {
        [x / length, y / length, z / length]
    } else {
        [0.0; 3]
    };
    let speed = launch.speed();
    let mut vertical = speed * y;
    if state.on_ground {
        vertical = if shallow_water {
            (vertical / 0.8) * 0.98
        } else {
            vertical + 0.08
        };
    }
    state.velocity.x = f64::from(state.velocity.x as f32 + speed * x);
    state.velocity.y = f64::from(state.velocity.y as f32 + vertical);
    state.velocity.z = f64::from(state.velocity.z as f32 + speed * z);
}

/// Ends a spin on side collision, on ground after six ticks, or after twenty ticks.
pub(super) fn finish(state: &mut PlayerState) {
    let Some(elapsed) = state.spin_attack_ticks else {
        return;
    };
    state.spin_attack_ticks = if state.collisions.x
        || state.collisions.z
        || elapsed >= 19
        || (elapsed >= 5 && state.on_ground)
    {
        None
    } else {
        Some(elapsed + 1)
    };
}
