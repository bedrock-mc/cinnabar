//! Ability-flight controls and independent horizontal/vertical drag.

use crate::{CollisionQuery, CollisionWorld, Vec3, WorldQueryError};

use super::{DEFAULT_AIR_FRICTION, MovementInput};

const DEFAULT_FLY_SPEED: f32 = 0.05;
const DEFAULT_VERTICAL_FLY_SPEED: f32 = 1.0;
// HorizontalFlySpeedControl current RVA 0x03235360, PE VA 0x1501672a8.
const FLY_SPRINT_MULTIPLIER: f32 = 2.0;
// VerticalFlySpeedControl current RVA 0x02c452b0, matching PE float lanes.
const FLY_ASCEND: f32 = 0.15;
const FLY_DESCEND: f32 = -0.22;
const HOVER_INPUT_THRESHOLD: f32 = 0.01;
const CREATIVE_HOVER_MODIFIER: f32 = 0.375;
const OTHER_HOVER_MODIFIER: f32 = 0.75;
// FlyDrag current RVA 0x03217940 reads this friction coefficient; retention
// is its native float subtraction from one, independent of horizontal drag.
const VERTICAL_FRICTION: f32 = 0.399_999_98;

/// Flying shares DefaultMoveSystems' ground-friction probe at starting AABB
/// minimum y minus native float 0.1, including fractional support heights.
pub(super) fn sample_ground_friction(
    world: &impl CollisionWorld,
    feet: Vec3,
    previous_samples: usize,
) -> Result<CollisionQuery<f64>, WorldQueryError> {
    if previous_samples == super::MAX_BLOCK_SAMPLES_PER_TICK {
        return Err(WorldQueryError::QueryExtentExceeded);
    }
    let point = Vec3::new(
        f64::from(feet.x as f32),
        f64::from(feet.y as f32 - 0.1_f32),
        f64::from(feet.z as f32),
    );
    let sample = world.block_physics(super::environment::block_at(point)?)?;
    Ok(CollisionQuery {
        value: sample.primary().friction,
        identity: sample.identity,
    })
}

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
    velocity.y = f64::from(velocity.y as f32 * (1.0 - VERTICAL_FRICTION));
}

fn hovering(move_vector: [f64; 2]) -> bool {
    (move_vector[0] as f32)
        .abs()
        .max((move_vector[1] as f32).abs())
        < HOVER_INPUT_THRESHOLD
}

fn damp_horizontal(value: f64, retention: f32) -> f64 {
    // Current horizontal drag RVA 0x03203a50 clears each lane at its float
    // epsilon before applying friction; this is independent of vertical drag.
    let value = value as f32;
    if value.abs() <= f32::EPSILON {
        0.0
    } else {
        f64::from(value * retention)
    }
}
