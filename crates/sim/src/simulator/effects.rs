use serde::{Deserialize, Serialize};

/// Protocol-independent movement effects sampled for one fixed simulation tick.
///
/// Amplifiers preserve Bedrock's bounded signed `i32` and zero-based
/// convention. Packet identifiers and lifecycle belong to the application
/// boundary; converting any amplifier to the force-law scalar remains finite.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementEffects {
    pub jump_boost: Option<i32>,
    pub levitation: Option<i32>,
    pub slow_falling: bool,
}

impl MovementEffects {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.jump_boost.is_none() && self.levitation.is_none() && !self.slow_falling
    }
}

pub(super) fn apply_vertical(
    velocity_y: &mut f64,
    effects: MovementEffects,
    gravity: f64,
    gravity_multiplier: f64,
) {
    if let Some(amplifier) = effects.levitation {
        let target = 0.05 * (f64::from(amplifier) + 1.0);
        *velocity_y += (target - *velocity_y) * 0.2;
    } else {
        *velocity_y = (*velocity_y - gravity) * gravity_multiplier;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn signed_wire_amplifier_domain_has_finite_effect_levels() {
        for amplifier in [i32::MIN, -2, -1, 0, 1, i32::MAX] {
            let level = f64::from(amplifier) + 1.0;
            assert!((0.1 * level).is_finite());
            assert!((0.05 * level).is_finite());
        }

        let mut value = 0x9e37_79b9_u32;
        for _ in 0..10_000 {
            value = value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let level = f64::from(value as i32) + 1.0;
            assert!((0.1 * level).is_finite());
            assert!((0.05 * level).is_finite());
        }
    }

    #[test]
    fn non_integer_and_non_finite_amplifier_encodings_are_rejected() {
        for jump_boost in ["1.5", "1e400"] {
            let encoded =
                format!(r#"{{"jump_boost":{jump_boost},"levitation":null,"slow_falling":false}}"#);
            assert!(serde_json::from_str::<super::MovementEffects>(&encoded).is_err());
        }
    }
}
