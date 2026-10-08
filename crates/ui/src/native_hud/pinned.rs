//! Native HUD sprite tables and timing.

use assets::HudTextureRole;
use super::{HeartVariant, HudEffect};

/// Damage blink: hearts flash for one second, alternating every 150 ms
/// (the reference alternates every 3 ticks for 20 ticks).
pub(super) const DAMAGE_FLASH_WINDOW_MILLIS: u64 = 1_000;
pub(super) const DAMAGE_FLASH_PHASE_MILLIS: u64 = 150;
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

/// `Some(true)` while the damage blink shows the flash sprites, `Some(false)`
/// during the off phase, `None` outside the blink window.
pub(super) fn damage_flash_phase(drop_millis: Option<u64>, now_millis: u64) -> Option<bool> {
    let elapsed = now_millis.saturating_sub(drop_millis?);
    if elapsed >= DAMAGE_FLASH_WINDOW_MILLIS {
        return None;
    }
    Some((elapsed / DAMAGE_FLASH_PHASE_MILLIS).is_multiple_of(2))
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
