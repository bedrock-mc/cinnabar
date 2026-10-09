//! Item-state projection shared by gameplay and UI.

/// Maximum held-use duration for native bows and tridents.
pub const LONG_WEAPON_USE_TICKS: u32 = 72_000;

/// The main-hand bow frame follows draw power while its use counter remains nonzero.
/// Other items keep frame zero; crossbows use their separate projectile-aware rule.
pub fn ranged_animation_frame(
    identifier: Option<&str>,
    elapsed: Option<u32>,
    duration: u32,
) -> u32 {
    let Some(elapsed) =
        elapsed.filter(|elapsed| identifier == Some("minecraft:bow") && *elapsed < duration)
    else {
        return 0;
    };
    let seconds = elapsed as f32 / 20.0;
    let power = ((seconds * seconds + 2.0 * seconds) / 3.0).min(1.0);
    (3.0 * power * 0.99) as u32 + 1
}

/// Vanilla crossbow animation frame, including loaded projectile art.
pub fn crossbow_animation_frame(
    elapsed: Option<u32>,
    duration: u32,
    projectile: Option<&str>,
    offhand_firework: bool,
) -> u32 {
    if let Some(elapsed) = elapsed.filter(|_| duration > 0) {
        let fraction = elapsed as f32 / duration as f32;
        let power = ((fraction * fraction + 2.0 * fraction) / 3.0).min(1.0);
        let frame = (power * 0.99 * 5.0) as u32;
        if frame >= 4 && power < 1.0 && offhand_firework {
            5
        } else {
            frame
        }
    } else {
        projectile.map_or(0, |projectile| {
            if projectile == "minecraft:arrow" {
                4
            } else {
                5
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_bow_frames_are_not_the_pose_charge_curve() {
        assert_eq!(
            ranged_animation_frame(Some("minecraft:bow"), None, LONG_WEAPON_USE_TICKS),
            0
        );
        for tick in 0..=8 {
            assert_eq!(
                ranged_animation_frame(Some("minecraft:bow"), Some(tick), LONG_WEAPON_USE_TICKS),
                1
            );
        }
        for tick in 9..=14 {
            assert_eq!(
                ranged_animation_frame(Some("minecraft:bow"), Some(tick), LONG_WEAPON_USE_TICKS),
                2
            );
        }
        for tick in 15..=30 {
            assert_eq!(
                ranged_animation_frame(Some("minecraft:bow"), Some(tick), LONG_WEAPON_USE_TICKS),
                3
            );
        }
    }

    #[test]
    fn non_ranged_items_and_completed_bow_counters_stay_idle() {
        for identifier in [
            None,
            Some("minecraft:apple"),
            Some("minecraft:trident"),
            Some("custom:held"),
        ] {
            assert_eq!(ranged_animation_frame(identifier, Some(15), 40), 0);
        }
        assert_eq!(
            ranged_animation_frame(Some("minecraft:bow"), Some(39), 40),
            3
        );
        for elapsed in [40, 41] {
            assert_eq!(
                ranged_animation_frame(Some("minecraft:bow"), Some(elapsed), 40),
                0
            );
        }
        assert_eq!(ranged_animation_frame(Some("minecraft:bow"), Some(0), 0), 0);
    }
}
