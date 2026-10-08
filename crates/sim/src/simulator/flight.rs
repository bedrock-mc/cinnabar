//! Ability-flight controls and independent horizontal/vertical drag.

use crate::Vec3;

use super::{DEFAULT_AIR_FRICTION, MovementInput, effects::damp_horizontal};

const DEFAULT_FLY_SPEED: f32 = 0.05;
const DEFAULT_VERTICAL_FLY_SPEED: f32 = 1.0;
// Horizontal fly speed scales with the ability fly speed, doubled while sprinting.
const FLY_SPRINT_MULTIPLIER: f32 = 2.0;
// Vertical fly input adds these f32 impulses for jump and sneak.
const FLY_ASCEND: f32 = 0.15;
const FLY_DESCEND: f32 = -0.22;
const HOVER_INPUT_THRESHOLD: f32 = 0.01;
const CREATIVE_HOVER_MODIFIER: f32 = 0.375;
const OTHER_HOVER_MODIFIER: f32 = 0.75;
// Vertical fly drag retains `1 - VERTICAL_FRICTION * air_drag_modifier` in f32,
// independent of horizontal drag.
const VERTICAL_FRICTION: f32 = 0.399_999_98;

pub(super) fn horizontal_speed(input: &MovementInput) -> f64 {
    let speed = input
        .fly_speed
        .map_or(DEFAULT_FLY_SPEED, |value| value as f32);
    f64::from(if input.sprinting {
        speed * FLY_SPRINT_MULTIPLIER
    } else {
        speed
    })
}

/// Creative idle hover reduces existing vertical motion before this tick's
/// movement. Held jump/descent cancel that reduction and add their ability-
/// scaled acceleration; simultaneous held controls stop vertical movement.
pub(super) fn apply_vertical_control(
    velocity: &mut Vec3,
    input: &MovementInput,
    move_vector: [f64; 2],
) {
    if input.jumping && input.sneaking {
        velocity.y = 0.0;
        return;
    }
    let mut previous = velocity.y as f32;
    if hovering(move_vector) && input.creative_flight && !input.jumping && !input.sneaking {
        previous *= CREATIVE_HOVER_MODIFIER;
    }
    let acceleration = if input.jumping {
        FLY_ASCEND
    } else if input.sneaking {
        FLY_DESCEND
    } else {
        0.0
    };
    let speed = input
        .vertical_fly_speed
        .map_or(DEFAULT_VERTICAL_FLY_SPEED, |value| value as f32);
    velocity.y = f64::from(acceleration * speed + previous);
}

/// Horizontal hover overrides the ordinary air friction. Native flight
/// vertical drag runs separately and never multiplies by that override.
pub(super) fn apply_drag(
    velocity: &mut Vec3,
    input: &MovementInput,
    move_vector: [f64; 2],
    ground_friction: f64,
) {
    let modifier = match (hovering(move_vector), input.creative_flight) {
        (false, _) => 1.0,
        (true, true) => CREATIVE_HOVER_MODIFIER,
        (true, false) => OTHER_HOVER_MODIFIER,
    };
    let horizontal = ground_friction as f32 * modifier * DEFAULT_AIR_FRICTION as f32;
    velocity.x = damp_horizontal(velocity.x, horizontal);
    velocity.z = damp_horizontal(velocity.z, horizontal);
    let retention = input.vertical_physics.modified_retention(VERTICAL_FRICTION);
    velocity.y = f64::from(velocity.y as f32 * retention);
}

fn hovering(move_vector: [f64; 2]) -> bool {
    (move_vector[0] as f32)
        .abs()
        .max((move_vector[1] as f32).abs())
        < HOVER_INPUT_THRESHOLD
}
