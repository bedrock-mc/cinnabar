//! Pinned presentation tables and timing helpers: sprite selection, blink
//! phases, and recorded color approximations.

use assets::HudTextureRole;

use crate::ui_runtime::gameplay_hud::{HeartVariant, HudEffect};

pub use ui::gui_scale;

/// Vanilla survival hotbar width in GUI px (start cap + nine slots + end cap).
pub(super) const HOTBAR_WIDTH: f32 = 182.0;
/// Fixed height of the bottom-anchored HUD stack in GUI px, measured from the
/// selected-item label zone top down to the hotbar's bottom edge.
pub(super) const BOTTOM_STACK_HEIGHT: f32 = 59.0;
/// Effects blink through their final 10 s (200 ticks).
pub(super) const EFFECT_BLINK_TICKS: u64 = 200;
/// Row cap for pathological health maxima: six stacked rows (60 hearts).
pub(super) const MAX_HEART_ROWS: u16 = 6;
/// The reference caps mount hearts at 30.
pub(super) const MAX_MOUNT_HEARTS: u16 = 30;
/// Pinned harmful effect ids (Bedrock ids; poison family, wither, darkness,
/// slowness, mining fatigue, instant damage, nausea, blindness, hunger,
/// weakness, levitation, bad omen). Everything else sits on the beneficial row.
pub(super) const HARMFUL_EFFECT_IDS: [i32; 13] = [2, 4, 7, 9, 15, 17, 18, 19, 20, 24, 25, 28, 30];
/// Boss bar tint per authoritative color. The carried track sprites are the
/// official Bedrock progress textures; these multipliers are a recorded
/// approximation of the reference bar hues pending the native gallery
/// (RebeccaPurple is exact by definition).
pub const BOSS_TINTS: [(ui::BossColor, [u8; 4]); 8] = [
    (ui::BossColor::Pink, [255, 105, 180, 255]),
    (ui::BossColor::Blue, [85, 85, 255, 255]),
    (ui::BossColor::Red, [255, 85, 85, 255]),
    (ui::BossColor::Green, [85, 255, 85, 255]),
    (ui::BossColor::Yellow, [255, 255, 85, 255]),
    (ui::BossColor::Purple, [170, 0, 170, 255]),
    (ui::BossColor::RebeccaPurple, [102, 51, 153, 255]),
    (ui::BossColor::White, [255, 255, 255, 255]),
];

/// Bedrock effect id -> carried icon role, pinned to the id table verified
/// against gophertunnel (1-27) and PocketMine (28-30). Fatal poison presents
/// with poison's icon as in the vanilla client. Unknown ids return `None` and
/// the effect entry is skipped rather than guessed.
#[must_use]
pub fn effect_icon_role(effect_id: i32) -> Option<HudTextureRole> {
    Some(match effect_id {
        1 => HudTextureRole::EffectIconSpeed,
        2 => HudTextureRole::EffectIconSlowness,
        3 => HudTextureRole::EffectIconHaste,
        4 => HudTextureRole::EffectIconMiningFatigue,
        5 => HudTextureRole::EffectIconStrength,
        8 => HudTextureRole::EffectIconJumpBoost,
        9 => HudTextureRole::EffectIconNausea,
        10 => HudTextureRole::EffectIconRegeneration,
        11 => HudTextureRole::EffectIconResistance,
        12 => HudTextureRole::EffectIconFireResistance,
        13 => HudTextureRole::EffectIconWaterBreathing,
        14 => HudTextureRole::EffectIconInvisibility,
        15 => HudTextureRole::EffectIconBlindness,
        16 => HudTextureRole::EffectIconNightVision,
        17 => HudTextureRole::EffectIconHunger,
        18 => HudTextureRole::EffectIconWeakness,
        19 | 25 => HudTextureRole::EffectIconPoison,
        20 => HudTextureRole::EffectIconWither,
        21 => HudTextureRole::EffectIconHealthBoost,
        22 => HudTextureRole::EffectIconAbsorption,
        24 => HudTextureRole::EffectIconLevitation,
        26 => HudTextureRole::EffectIconConduitPower,
        27 => HudTextureRole::EffectIconSlowFalling,
        28 => HudTextureRole::EffectIconBadOmen,
        29 => HudTextureRole::EffectIconVillageHero,
        30 => HudTextureRole::EffectIconDarkness,
        _ => return None,
    })
}

pub(super) fn heart_role(
    variant: HeartVariant,
    flash: Option<bool>,
    filled_halves: u32,
) -> Option<HudTextureRole> {
    let half = filled_halves == 1;
    if filled_halves == 0 {
        return None;
    }
    let flashing = flash == Some(true);
    Some(match (variant, flashing, half) {
        (HeartVariant::Normal, false, false) => HudTextureRole::HeartFull,
        (HeartVariant::Normal, false, true) => HudTextureRole::HeartHalf,
        (HeartVariant::Normal, true, false) => HudTextureRole::HeartFlashFull,
        (HeartVariant::Normal, true, true) => HudTextureRole::HeartFlashHalf,
        (HeartVariant::Poisoned, false, false) => HudTextureRole::PoisonHeartFull,
        (HeartVariant::Poisoned, false, true) => HudTextureRole::PoisonHeartHalf,
        (HeartVariant::Poisoned, true, false) => HudTextureRole::PoisonHeartFlashFull,
        (HeartVariant::Poisoned, true, true) => HudTextureRole::PoisonHeartFlashHalf,
        (HeartVariant::Withered, false, false) => HudTextureRole::WitherHeartFull,
        (HeartVariant::Withered, false, true) => HudTextureRole::WitherHeartHalf,
        (HeartVariant::Withered, true, false) => HudTextureRole::WitherHeartFlashFull,
        (HeartVariant::Withered, true, true) => HudTextureRole::WitherHeartFlashHalf,
        (HeartVariant::Frozen, false, false) => HudTextureRole::FreezeHeartFull,
        (HeartVariant::Frozen, false, true) => HudTextureRole::FreezeHeartHalf,
        (HeartVariant::Frozen, true, false) => HudTextureRole::FreezeHeartFlashFull,
        (HeartVariant::Frozen, true, true) => HudTextureRole::FreezeHeartFlashHalf,
    })
}

/// Ticks per blink cycle in the final seconds. Needs independent measurement.
const BLINK_PERIOD_TICKS: f32 = 10.0;
/// Blink swing around the resting opacity, growing toward expiry. Needs independent measurement.
const BLINK_MIN_SWING: f32 = 0.1;
const BLINK_MAX_SWING: f32 = 0.25;
const BLINK_CENTER: f32 = 0.75;

/// Alpha for an effect entry: solid normally; through the final ten seconds it pulses on a fixed
/// period with a swing that widens as the effect runs out.
pub(super) fn effect_blink_alpha(effect: &HudEffect, now_tick: Option<u64>) -> u8 {
    let Some(remaining) = effect.remaining_ticks(now_tick) else {
        return 255;
    };
    if remaining >= EFFECT_BLINK_TICKS {
        return 255;
    }
    let urgency = 1.0 - remaining as f32 / EFFECT_BLINK_TICKS as f32;
    let swing = BLINK_MIN_SWING + (BLINK_MAX_SWING - BLINK_MIN_SWING) * urgency;
    let wave = (remaining as f32 * std::f32::consts::TAU / BLINK_PERIOD_TICKS).cos();
    ((BLINK_CENTER + swing * wave).clamp(0.0, 1.0) * 255.0) as u8
}

/// Durability hue: green at full durability sweeping to red, matching the
/// reference's HSV ramp (hue = fraction / 3, full saturation and value).
pub(super) fn hsv_to_rgb(hue: f32) -> [u8; 4] {
    let hue = hue.clamp(0.0, 1.0) * 6.0;
    let sector = hue.floor() as u32 % 6;
    let fraction = hue - hue.floor();
    let ascending = (fraction * 255.0) as u8;
    let descending = 255 - ascending;
    match sector {
        0 => [255, ascending, 0, 255],
        1 => [descending, 255, 0, 255],
        2 => [0, 255, ascending, 255],
        3 => [0, descending, 255, 255],
        4 => [ascending, 0, 255, 255],
        _ => [255, 0, descending, 255],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(expires: u64) -> HudEffect {
        HudEffect {
            effect_id: 1,
            amplifier: 0,
            ambient: false,
            particles: true,
            expires_at_tick: Some(expires),
        }
    }

    #[test]
    fn effects_are_solid_until_the_final_ten_seconds() {
        assert_eq!(effect_blink_alpha(&effect(1_000), Some(0)), 255);
        assert_eq!(effect_blink_alpha(&effect(1_000), Some(800)), 255);
        let infinite = HudEffect {
            expires_at_tick: None,
            ..effect(0)
        };
        assert_eq!(effect_blink_alpha(&infinite, Some(5)), 255);
    }

    #[test]
    fn blink_pulses_within_bounds_and_widens_toward_expiry() {
        let range = |from: u64, to: u64| {
            let alphas: Vec<u8> = (from..to)
                .map(|now| effect_blink_alpha(&effect(1_000), Some(now)))
                .collect();
            (*alphas.iter().min().unwrap(), *alphas.iter().max().unwrap())
        };
        let (early_low, early_high) = range(801, 821);
        let (late_low, late_high) = range(980, 1_000);
        assert!(late_high - late_low > early_high - early_low);
        assert!(late_low >= 120);
    }
}
