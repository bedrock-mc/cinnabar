//! Native tick-sampled local camera height, separate from collision/network position.

use protocol::PLAYER_NETWORK_OFFSET;
use sim::MovementMode;

// VanillaOffsetSystem dispatcher selects the
// current-game-version 0x3eb33333 drop. The older-version branch is not our target.
const CROUCH_EYE_DROP: f32 = 0.35;
// Horizontal poses use this eye height on each client tick.
const HORIZONTAL_POSE_EYE_HEIGHT: f32 = 0.4;
// The same native tick uses a 0.5 blend for every axis; not render-frame damping.
const OFFSET_TICK_BLEND: f32 = 0.5;

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct LocalEyeOffset {
    previous: f32,
    current: f32,
}

impl LocalEyeOffset {
    pub(super) fn tick(&mut self, mode: MovementMode, sneaking: bool) {
        let target = match mode {
            MovementMode::Swimming | MovementMode::Crawling | MovementMode::Gliding => {
                PLAYER_NETWORK_OFFSET - HORIZONTAL_POSE_EYE_HEIGHT
            }
            _ if sneaking => CROUCH_EYE_DROP,
            _ => 0.0,
        };
        self.previous = self.current;
        self.current += (target - self.current) * OFFSET_TICK_BLEND;
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

    #[test]
    fn crouch_drop_halves_the_remaining_gap_once_per_tick_and_interpolates() {
        let mut eye = LocalEyeOffset::default();
        assert_eq!(eye.height(0.0), PLAYER_NETWORK_OFFSET);
        eye.tick(MovementMode::Walking, true);
        assert_eq!(eye.height(0.0), PLAYER_NETWORK_OFFSET);
        assert_eq!(eye.height(0.5), PLAYER_NETWORK_OFFSET - 0.0875);
        assert_eq!(eye.height(1.0), PLAYER_NETWORK_OFFSET - 0.175);
        eye.tick(MovementMode::Walking, true);
        assert_eq!(eye.height(1.0), PLAYER_NETWORK_OFFSET - 0.2625);
        eye.tick(MovementMode::Walking, false);
        assert_eq!(eye.height(0.0), PLAYER_NETWORK_OFFSET - 0.2625);
        assert_eq!(eye.height(1.0), PLAYER_NETWORK_OFFSET - 0.13125);
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
                eye.tick(mode, true);
            }
            assert!((eye.height(1.0) - HORIZONTAL_POSE_EYE_HEIGHT).abs() < 1.0e-6);
            for _ in 0..32 {
                eye.tick(MovementMode::Walking, false);
            }
            assert_eq!(eye.height(1.0), PLAYER_NETWORK_OFFSET);
        }
    }
}
