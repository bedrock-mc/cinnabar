//! Native tick-sampled local camera height, separate from collision/network position.

use protocol::PLAYER_NETWORK_OFFSET;
use sim::{MovementMode, SneakProfile};

// Crouching drops the eye by 0.35 (f32 0x3eb33333) from base game version
// 1.20.10, and by 0.125 in older levels, under the collision-height gate.
const CROUCH_EYE_DROP: f32 = 0.35;
const LEGACY_CROUCH_EYE_DROP: f32 = 0.125;
// Horizontal poses use this eye height on each client tick.
const HORIZONTAL_POSE_EYE_HEIGHT: f32 = 0.4;
// The same native tick uses a 0.5 blend for every axis; not render-frame damping.
const OFFSET_TICK_BLEND: f32 = 0.5;

#[cfg(test)]
#[path = "eye/replay_tests.rs"]
mod replay_tests;

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct LocalEyeOffset {
    previous: f32,
    current: f32,
    interpolation_held: bool,
}

impl LocalEyeOffset {
    /// Holds the completed eye offset when collision data pauses movement ticks.
    pub(super) fn finish_interpolation(&mut self) {
        self.previous = self.current;
        self.interpolation_held = true;
    }

    /// Keeps a collision pause applied to the current camera after rebuilding its pose.
    pub(super) fn preserve_interpolation_hold(&mut self, prior: Self) {
        if prior.interpolation_held {
            self.finish_interpolation();
        }
    }

    /// Eases the eye toward the selected pose, with a spin using the horizontal
    /// target and a crouch drop selected by the level's sneak profile.
    pub(super) fn tick(
        &mut self,
        mode: MovementMode,
        sneaking: bool,
        profile: SneakProfile,
        spinning: bool,
    ) {
        let target = match mode {
            _ if spinning => PLAYER_NETWORK_OFFSET - HORIZONTAL_POSE_EYE_HEIGHT,
            MovementMode::Swimming | MovementMode::Crawling | MovementMode::Gliding => {
                PLAYER_NETWORK_OFFSET - HORIZONTAL_POSE_EYE_HEIGHT
            }
            _ if sneaking => match profile {
                SneakProfile::Current => CROUCH_EYE_DROP,
                SneakProfile::Legacy => LEGACY_CROUCH_EYE_DROP,
            },
            _ => 0.0,
        };
        self.previous = self.current;
        self.current += (target - self.current) * OFFSET_TICK_BLEND;
        self.interpolation_held = false;
    }

    /// Native camera getter subtracts the interpolated offset.
    pub(super) fn height(&self, alpha: f32) -> f32 {
        // Native camera anchor is the same 1.62001002 float
        // already retained as the player protocol offset, not rounded 1.62.
        PLAYER_NETWORK_OFFSET
            - (self.previous + (self.current - self.previous) * alpha.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURRENT: SneakProfile = SneakProfile::Current;

    #[test]
    fn crouch_drop_halves_the_remaining_gap_once_per_tick_and_interpolates() {
        let mut eye = LocalEyeOffset::default();
        assert_eq!(eye.height(0.0), PLAYER_NETWORK_OFFSET);
        eye.tick(MovementMode::Walking, true, CURRENT, false);
        assert_eq!(eye.height(0.0), PLAYER_NETWORK_OFFSET);
        assert_eq!(eye.height(0.5), PLAYER_NETWORK_OFFSET - 0.0875);
        assert_eq!(eye.height(1.0), PLAYER_NETWORK_OFFSET - 0.175);
        eye.tick(MovementMode::Walking, true, CURRENT, false);
        assert_eq!(eye.height(1.0), PLAYER_NETWORK_OFFSET - 0.2625);
        eye.tick(MovementMode::Walking, false, CURRENT, false);
        assert_eq!(eye.height(0.0), PLAYER_NETWORK_OFFSET - 0.2625);
        assert_eq!(eye.height(1.0), PLAYER_NETWORK_OFFSET - 0.13125);
    }

    /// Levels older than base game version 1.20.10 lower the crouching eye by 0.125.
    #[test]
    fn legacy_sneak_profile_uses_the_shallower_crouch_eye_drop() {
        let mut eye = LocalEyeOffset::default();
        eye.tick(MovementMode::Walking, true, SneakProfile::Legacy, false);
        assert_eq!(eye.height(1.0), PLAYER_NETWORK_OFFSET - 0.0625);
        for _ in 0..32 {
            eye.tick(MovementMode::Walking, true, SneakProfile::Legacy, false);
        }
        assert!((eye.height(1.0) - (PLAYER_NETWORK_OFFSET - 0.125)).abs() < 1.0e-6);
    }

    #[test]
    fn horizontal_pose_overrides_sneak_and_returns_to_standing() {
        for mode in [
            MovementMode::Swimming,
            MovementMode::Crawling,
            MovementMode::Gliding,
        ] {
            let mut eye = LocalEyeOffset::default();
            for _ in 0..32 {
                eye.tick(mode, true, CURRENT, false);
            }
            assert!((eye.height(1.0) - HORIZONTAL_POSE_EYE_HEIGHT).abs() < 1.0e-6);
            for _ in 0..32 {
                eye.tick(MovementMode::Walking, false, CURRENT, false);
            }
            assert_eq!(eye.height(1.0), PLAYER_NETWORK_OFFSET);
        }
    }

    #[test]
    fn riptide_uses_the_horizontal_eye_even_while_sneaking() {
        let mut eye = LocalEyeOffset::default();
        for _ in 0..32 {
            eye.tick(MovementMode::Walking, true, CURRENT, true);
        }
        assert!((eye.height(1.0) - HORIZONTAL_POSE_EYE_HEIGHT).abs() < 1.0e-6);
        eye.tick(MovementMode::Walking, false, CURRENT, false);
        assert!(eye.height(1.0) > HORIZONTAL_POSE_EYE_HEIGHT);
        assert!(eye.height(1.0) < PLAYER_NETWORK_OFFSET);
    }
}
