//! Tick-driven vertical motion for status icons. Cycle lengths and thresholds are
//! behavior recorded from gameplay and need independent measurement.

/// Hearts vibrate while health plus absorption is at most this many half-hearts.
const LOW_HEALTH_HALVES: u32 = 4;
/// Gap ticks appended to the regeneration wave after it passes the last heart.
const REGEN_WAVE_GAP: u64 = 5;
/// GUI px a regenerating heart rises at the crest of the wave.
const REGEN_LIFT: f32 = 2.0;

fn mix(tick: u64, index: u32) -> u64 {
    let mut value = tick
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(u64::from(index).wrapping_mul(0xD1B5_4A32_D192_ED03));
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

/// Upward lift in GUI px for the heart at `index` (0 = no motion, positive = up).
pub(super) fn heart_lift(
    index: u32,
    health_hearts: u32,
    halves_with_absorption: u32,
    regenerating: bool,
    tick: u64,
) -> f32 {
    let mut lift = 0.0;
    if halves_with_absorption <= LOW_HEALTH_HALVES {
        lift += (mix(tick, index) & 1) as f32;
    }
    if regenerating && u64::from(index) == tick % (u64::from(health_hearts) + REGEN_WAVE_GAP) {
        lift += REGEN_LIFT;
    }
    lift
}

/// True on the ticks where the hunger row shakes: saturation empty, pulsing
/// more often as the food level drops.
pub(super) fn hunger_shakes(saturation_empty: bool, food_points: u32, tick: u64) -> bool {
    saturation_empty && tick.is_multiple_of(u64::from(food_points) * 3 + 1)
}

/// Vertical offset in GUI px (-1, 0 or 1, positive = down) for one hunger icon during a shake.
pub(super) fn hunger_shake_offset(index: u32, tick: u64) -> f32 {
    (mix(tick, index) % 3) as f32 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_idle_hearts_do_not_move() {
        for index in 0..10 {
            assert_eq!(heart_lift(index, 10, 20, false, 123), 0.0);
        }
    }

    #[test]
    fn low_health_jitters_within_one_pixel() {
        let lifts: Vec<f32> = (0..64)
            .map(|tick| heart_lift(3, 10, 4, false, tick))
            .collect();
        assert!(lifts.iter().all(|lift| (0.0..=1.0).contains(lift)));
        assert!(lifts.contains(&0.0) && lifts.contains(&1.0));
    }

    #[test]
    fn regeneration_wave_lifts_one_heart_per_tick_then_pauses() {
        for tick in 0..15u64 {
            let raised: Vec<u32> = (0..10)
                .filter(|index| heart_lift(*index, 10, 20, true, tick) > 0.0)
                .collect();
            if tick < 10 {
                assert_eq!(raised, vec![tick as u32]);
            } else {
                assert!(raised.is_empty());
            }
        }
    }

    #[test]
    fn hunger_shake_needs_empty_saturation() {
        assert!(!hunger_shakes(false, 20, 0));
        assert!(hunger_shakes(true, 20, 0));
        assert!(!hunger_shakes(true, 20, 1));
        assert!(hunger_shakes(true, 0, 7));
    }

    #[test]
    fn hunger_shake_offset_stays_within_one_pixel() {
        assert!((0..64).all(|tick| hunger_shake_offset(2, tick).abs() <= 1.0));
    }
}
