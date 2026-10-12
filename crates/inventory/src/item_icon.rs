//! Item-state projection shared by gameplay and UI.

#[cfg(test)]
use assets::item_use::LONG_WEAPON_USE_TICKS;
use assets::item_use::{ItemProjectileRole, ItemUseAnimation};

/// The main-hand bow frame follows draw power while its use counter remains nonzero.
/// Other items keep frame zero; crossbows use their separate projectile-aware rule.
pub fn ranged_animation_frame(
    animation: ItemUseAnimation,
    elapsed: Option<u32>,
    duration: u32,
) -> u32 {
    let Some(elapsed) =
        elapsed.filter(|elapsed| animation == ItemUseAnimation::Bow && *elapsed < duration)
    else {
        return 0;
    };
    let seconds = elapsed as f32 / simulation_clock::TICKS_PER_SECOND as f32;
    let power = ((seconds * seconds + 2.0 * seconds) / 3.0).min(1.0);
    (3.0 * power * 0.99) as u32 + 1
}

/// Vanilla crossbow animation frame, including loaded projectile art.
pub fn crossbow_animation_frame(
    elapsed: Option<u32>,
    duration: u32,
    projectile: Option<ItemProjectileRole>,
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
        match projectile {
            Some(ItemProjectileRole::Arrow) => 4,
            Some(ItemProjectileRole::Firework) => 5,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admitted_bow_frame_does_not_depend_on_display_identity() {
        assert_eq!(
            ranged_animation_frame(ItemUseAnimation::Bow, Some(15), 40),
            3
        );
        assert_eq!(
            ranged_animation_frame(ItemUseAnimation::None, Some(15), 40),
            0
        );
        assert_eq!(
            crossbow_animation_frame(None, 0, Some(ItemProjectileRole::None), false),
            0
        );
    }

    #[test]
    fn native_bow_frames_are_not_the_pose_charge_curve() {
        assert_eq!(
            ranged_animation_frame(ItemUseAnimation::Bow, None, LONG_WEAPON_USE_TICKS),
            0
        );
        for tick in 0..=8 {
            assert_eq!(
                ranged_animation_frame(ItemUseAnimation::Bow, Some(tick), LONG_WEAPON_USE_TICKS),
                1
            );
        }
        for tick in 9..=14 {
            assert_eq!(
                ranged_animation_frame(ItemUseAnimation::Bow, Some(tick), LONG_WEAPON_USE_TICKS),
                2
            );
        }
        for tick in 15..=30 {
            assert_eq!(
                ranged_animation_frame(ItemUseAnimation::Bow, Some(tick), LONG_WEAPON_USE_TICKS),
                3
            );
        }
    }

    #[test]
    fn non_ranged_items_and_completed_bow_counters_stay_idle() {
        for animation in [ItemUseAnimation::None, ItemUseAnimation::Crossbow] {
            assert_eq!(ranged_animation_frame(animation, Some(15), 40), 0);
        }
        assert_eq!(
            ranged_animation_frame(ItemUseAnimation::Bow, Some(39), 40),
            3
        );
        for elapsed in [40, 41] {
            assert_eq!(
                ranged_animation_frame(ItemUseAnimation::Bow, Some(elapsed), 40),
                0
            );
        }
        assert_eq!(ranged_animation_frame(ItemUseAnimation::Bow, Some(0), 0), 0);
    }
}
