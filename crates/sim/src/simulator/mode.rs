use serde::{Deserialize, Serialize};

use crate::{Aabb, CollisionWorld, PLAYER_HEIGHT, PLAYER_WIDTH, Vec3, WorldQueryError};

/// Sneaking collision height for base game versions at or after the change.
const SNEAK_HEIGHT: f64 = 1.49_f32 as f64;
/// First base game version whose sneaking pose lowers the collision box.
pub const SNEAKING_HEIGHT_CHANGE_VERSION: [u16; 3] = [1, 20, 10];
/// Vanilla's horizontal pose uses collision width as height.
const LOW_POSE_HEIGHT: f64 = PLAYER_WIDTH;
/// Vanilla's bounding-box input update shrinks all probe faces by this inset.
const FIT_INSET: f64 = 0.01_f32 as f64;

/// Locomotion mode the client selected for one tick; the simulator never picks it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementMode {
    #[default]
    Walking,
    /// Pose-swimming: travel follows the look direction through water.
    Swimming,
    /// Land movement in a 0.6-high box, at sneak speed.
    Crawling,
    Gliding,
    /// Ability flight: no gravity, vertical motion from jump and sneak.
    Flying,
    /// Mounted: the vehicle owns motion, so no player physics runs.
    Riding,
}

/// Sneaking dimensions selected once per level by its base game version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SneakProfile {
    /// Base game version 1.20.10 or newer, or any version: sneaking lowers the box.
    #[default]
    Current,
    /// An older base game version keeps the standing box while sneaking.
    Legacy,
}

impl SneakProfile {
    /// Selects the profile for a level base game version; `None` is compatible
    /// with every version and therefore uses the current dimensions.
    #[must_use]
    pub fn for_base_game_version(version: Option<[u16; 3]>) -> Self {
        match version {
            Some(version) if version < SNEAKING_HEIGHT_CHANGE_VERSION => Self::Legacy,
            _ => Self::Current,
        }
    }

    /// Collision height of the sneaking walking pose.
    #[must_use]
    pub const fn height(self) -> f64 {
        match self {
            Self::Current => SNEAK_HEIGHT,
            Self::Legacy => PLAYER_HEIGHT,
        }
    }

    /// Whether the level uses the reduced sneaking box and current eye drop.
    pub(super) const fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }
}

impl MovementMode {
    /// Collision height of this pose; sneaking only changes walking, by `profile`.
    #[must_use]
    pub fn hitbox_height(self, sneaking: bool, profile: SneakProfile) -> f64 {
        match self {
            Self::Swimming | Self::Crawling | Self::Gliding => LOW_POSE_HEIGHT,
            Self::Walking if sneaking => profile.height(),
            Self::Walking | Self::Flying | Self::Riding => PLAYER_HEIGHT,
        }
    }

    #[must_use]
    pub const fn is_walking(self) -> bool {
        matches!(self, Self::Walking)
    }
}

/// Whether the player box for `mode` fits at `feet` without touching a solid.
pub fn pose_fits(
    world: &(impl CollisionWorld + ?Sized),
    feet: Vec3,
    mode: MovementMode,
    sneaking: bool,
    profile: SneakProfile,
) -> Result<bool, WorldQueryError> {
    let query = Aabb::player_with_height_at(feet, mode.hitbox_height(sneaking, profile));
    crate::world::validate_collision_query(query)?;
    let inset = Vec3::new(FIT_INSET, FIT_INSET, FIT_INSET);
    let probe = Aabb::new(query.min + inset, query.max - inset);
    let boxes = world.collision_boxes(probe)?;
    Ok(!boxes.value.into_iter().any(|shape| shape.intersects(probe)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_poses_share_one_height_and_sneak_only_shrinks_walking() {
        let current = SneakProfile::Current;
        assert_eq!(
            MovementMode::Swimming.hitbox_height(false, current),
            PLAYER_WIDTH
        );
        assert_eq!(
            MovementMode::Crawling.hitbox_height(true, current),
            PLAYER_WIDTH
        );
        assert_eq!(
            MovementMode::Gliding.hitbox_height(false, current),
            PLAYER_WIDTH
        );
        assert_eq!(
            MovementMode::Walking.hitbox_height(true, current),
            f64::from(1.49_f32)
        );
        assert_eq!(
            MovementMode::Walking.hitbox_height(false, current),
            PLAYER_HEIGHT
        );
        assert_eq!(
            MovementMode::Flying.hitbox_height(true, current),
            PLAYER_HEIGHT
        );
    }

    /// Base game versions before 1.20.10 keep the standing box while sneaking.
    #[test]
    fn legacy_base_game_versions_keep_the_standing_sneak_height() {
        for (version, expected) in [
            (None, SneakProfile::Current),
            (Some([1, 20, 10]), SneakProfile::Current),
            (Some([1, 26, 50]), SneakProfile::Current),
            (Some([1, 20, 9]), SneakProfile::Legacy),
            (Some([1, 19, 80]), SneakProfile::Legacy),
            (Some([0, 0, 0]), SneakProfile::Legacy),
        ] {
            assert_eq!(SneakProfile::for_base_game_version(version), expected);
        }
        let legacy = SneakProfile::Legacy;
        assert_eq!(
            MovementMode::Walking.hitbox_height(true, legacy),
            PLAYER_HEIGHT
        );
        assert_eq!(
            MovementMode::Crawling.hitbox_height(true, legacy),
            PLAYER_WIDTH
        );
    }
}
