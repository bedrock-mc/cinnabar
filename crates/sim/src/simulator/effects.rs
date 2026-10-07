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
    #[serde(default)]
    pub weaving: bool,
    /// Active blindness blocks a new sprint without changing existing motion.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blindness: bool,
}

impl MovementEffects {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.jump_boost.is_none()
            && self.levitation.is_none()
            && !self.slow_falling
            && !self.weaving
            && !self.blindness
    }
}

/// Server-owned actor facts that select gravity and vertical drag.
///
/// The default is the vanilla player: `HasGravity` set, no `UsesUniformAirDrag`,
/// and no `minecraft:air_drag_modifier` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VerticalPhysics {
    pub has_gravity: bool,
    pub uniform_air_drag: bool,
    /// Scales the vertical and flight drag fractions; absence reads as 1.
    pub air_drag_modifier: Option<f64>,
}

impl Default for VerticalPhysics {
    fn default() -> Self {
        Self {
            has_gravity: true,
            uniform_air_drag: false,
            air_drag_modifier: None,
        }
    }
}

impl VerticalPhysics {
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Ground/air gravity; none without `HasGravity`.
    pub(super) fn gravity(self, gravity: f64) -> f64 {
        if self.has_gravity { gravity } else { 0.0 }
    }

    /// Ground/air vertical drag retention: uniform air drag wins over gravity's
    /// drag, and an actor with neither keeps its vertical velocity.
    pub(super) fn vertical_drag_retention(self) -> f32 {
        let retention = if self.uniform_air_drag {
            UNIFORM_AIR_DRAG_RETENTION
        } else if self.has_gravity {
            super::NORMAL_GRAVITY_MULTIPLIER as f32
        } else {
            return 1.0;
        };
        self.modified_retention(1.0 - retention)
    }

    /// Retention after scaling `drag` by the modifier, clamped to `[0, 1]`.
    pub(super) fn modified_retention(self, drag: f32) -> f32 {
        let drag = drag
            * self
                .air_drag_modifier
                .map_or(1.0, |modifier| modifier as f32);
        if drag > 1.0 { 0.0 } else { 1.0 - drag.max(0.0) }
    }
}

const UNIFORM_AIR_DRAG_RETENTION: f32 = 0.91;

/// Applies levitation or gravity, then the travel mode's vertical drag.
pub(super) fn apply_vertical(
    velocity_y: &mut f64,
    effects: MovementEffects,
    gravity: f64,
    gravity_multiplier: f64,
) {
    let mut velocity = *velocity_y as f32;
    if let Some(amplifier) = effects.levitation {
        velocity *= 0.8_f32;
        velocity += amplifier.wrapping_add(1) as f32 * 0.01_f32;
    } else {
        velocity -= gravity as f32;
    }
    *velocity_y = f64::from(velocity * gravity_multiplier as f32);
}

/// Clears a horizontal lane at the float epsilon before applying friction.
pub(super) fn damp_horizontal(value: f64, retention: f32) -> f64 {
    let value = value as f32;
    if value.abs() <= f32::EPSILON {
        0.0
    } else {
        f64::from(value * retention)
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
