//! Pinned Java-reference presentation tables and timing helpers: GUI-scale
//! rule, sprite selection, blink phases, and recorded color approximations.

use assets::HudTextureRole;

use crate::ui_runtime::gameplay_hud::{HeartVariant, HudEffect};

/// Java auto GUI scale: the largest integer k with `width/k >= 320` and
/// `height/k >= 240`, never below 1. A fixed preference is clamped into the
/// same range, matching the options the reference client offers.
#[must_use]
pub(crate) fn java_gui_scale(physical: [u32; 2], preference: Option<u8>) -> u32 {
    let auto = (physical[0] / 320).min(physical[1] / 240).max(1);
    match preference {
        None | Some(0) => auto,
        Some(fixed) => u32::from(fixed).clamp(1, auto),
    }
}

/// Vanilla survival hotbar width in GUI px (start cap + nine slots + end cap).
pub(super) const HOTBAR_WIDTH: f32 = 182.0;
/// Fixed height of the bottom-anchored HUD stack in GUI px, measured from the
/// selected-item label zone top down to the hotbar's bottom edge.
pub(super) const BOTTOM_STACK_HEIGHT: f32 = 59.0;
/// The pinned hotbar cap alpha from the reviewed `hud_screen.json` authority.
pub(super) const HOTBAR_CAP_ALPHA: u8 = 166;
/// Experience-level green from the Java reference (0x80FF20).
pub(super) const XP_LEVEL_COLOR: [u8; 4] = [128, 255, 32, 255];
/// Damage blink: hearts flash for one second, alternating every 150 ms
/// (the reference alternates every 3 ticks for 20 ticks).
pub(super) const DAMAGE_FLASH_WINDOW_MILLIS: u64 = 1_000;
pub(super) const DAMAGE_FLASH_PHASE_MILLIS: u64 = 150;
/// Selected-item label: visible 2 s after an authoritative identity change,
/// fading out over the final 500 ms (40 ticks / 10 ticks in the reference).
pub(super) const LABEL_WINDOW_MILLIS: u64 = 2_000;
pub(super) const LABEL_FADE_MILLIS: u64 = 500;
/// Effects blink through their final 10 s (200 ticks).
pub(super) const EFFECT_BLINK_TICKS: u64 = 200;
/// Display cap for stacked boss bars; the retained store holds more.
pub(super) const MAX_PRESENTED_BOSS_BARS: usize = 8;
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
pub(super) const BOSS_TINTS: [(ui::BossColor, [u8; 4]); 8] = [
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
pub(crate) fn effect_icon_role(effect_id: i32) -> Option<HudTextureRole> {
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

pub(super) const fn hotbar_slot_role(slot: u8) -> HudTextureRole {
    match slot {
        0 => HudTextureRole::Hotbar0,
        1 => HudTextureRole::Hotbar1,
        2 => HudTextureRole::Hotbar2,
        3 => HudTextureRole::Hotbar3,
        4 => HudTextureRole::Hotbar4,
        5 => HudTextureRole::Hotbar5,
        6 => HudTextureRole::Hotbar6,
        7 => HudTextureRole::Hotbar7,
        _ => HudTextureRole::Hotbar8,
    }
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

/// Alpha for an effect entry: solid normally, blinking through the final ten
/// seconds with a deterministic triangular wave (bounded approximation of the
/// reference's accelerating flicker, pending the native gallery).
pub(super) fn effect_blink_alpha(effect: &HudEffect, now_tick: Option<u64>) -> u8 {
    let Some(remaining) = effect.remaining_ticks(now_tick) else {
        return 255;
    };
    if remaining >= EFFECT_BLINK_TICKS {
        return 255;
    }
    let phase = (remaining % 20) as f32 / 20.0;
    let wave = if phase < 0.5 {
        phase * 2.0
    } else {
        (1.0 - phase) * 2.0
    };
    (64.0 + 191.0 * wave) as u8
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
