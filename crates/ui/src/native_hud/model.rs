/// One retained authoritative status effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudEffect {
    pub effect_id: i32,
    pub amplifier: i32,
    pub ambient: bool,
    pub particles: bool,
    /// Server tick after which the effect is no longer presented. `None` is an
    /// effectively infinite (negative wire duration) effect.
    pub expires_at_tick: Option<u64>,
}

impl HudEffect {
    #[must_use]
    pub fn visible_at_tick(&self, now_tick: Option<u64>) -> bool {
        match (self.expires_at_tick, now_tick) {
            (None, _) => true,
            // Without a server clock the effect stays visible until removed.
            (Some(_), None) => true,
            (Some(expires), Some(now)) => now < expires,
        }
    }

    /// Remaining whole seconds, used for the Java expiry blink.
    #[must_use]
    pub fn remaining_ticks(&self, now_tick: Option<u64>) -> Option<u64> {
        match (self.expires_at_tick, now_tick) {
            (Some(expires), Some(now)) => Some(expires.saturating_sub(now)),
            _ => None,
        }
    }
}

/// Heart row recolor derived from authoritative effects and freezing state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeartVariant {
    #[default]
    Normal,
    Poisoned,
    Withered,
    Frozen,
}

/// Heart presentation follows the current authoritative effect order.
#[must_use]
pub fn heart_variant(effects: &[HudEffect], now_tick: Option<u64>, freezing: f32) -> HeartVariant {
        let mut variant = if freezing >= 1.0 {
            HeartVariant::Frozen
        } else {
            HeartVariant::Normal
        };
        for effect in effects {
            if !effect.visible_at_tick(now_tick) {
                continue;
            }
            match effect.effect_id {
                20 => variant = HeartVariant::Withered,
                19 | 25 => return HeartVariant::Poisoned,
                _ => {}
            }
        }
        variant
}

#[must_use]
pub fn regeneration_active(effects: &[HudEffect], now_tick: Option<u64>) -> bool {
    effects.iter().any(|effect| effect.effect_id == 10 && effect.visible_at_tick(now_tick))
}

#[must_use]
pub fn hunger_effect_active(effects: &[HudEffect], now_tick: Option<u64>) -> bool {
    effects.iter().any(|effect| effect.effect_id == 17 && effect.visible_at_tick(now_tick))
}
