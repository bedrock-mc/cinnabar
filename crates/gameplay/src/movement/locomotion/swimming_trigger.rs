//! Current SwimTriggerSystem (0x09fd25a0), for unmounted desktop input.

use sim::view_direction;

use super::{CollisionWorld, ModeIntent, ModeObservation, MovementMode, Vec3, WorldQueryError};

// PE VAs 0x14feff2ac, 0x14ffab6c8, 0x15013adc8 and 0x14ffd5070.
const MIN_SWIM_INPUT: f32 = std::f32::consts::FRAC_1_SQRT_2;
const ENTRY_UPWARD_LIMIT: f32 = 0.15;
const SURFACE_KEEP_ANGLE: f32 = 45.0;
const NATIVE_DEGREES_PER_RADIAN: f32 = 57.295_776;

pub(super) fn select(
    previous: MovementMode,
    intent: ModeIntent,
    observed: ModeObservation,
    world: &(impl CollisionWorld + ?Sized),
) -> Result<bool, WorldQueryError> {
    let look = view_direction(observed.pitch, observed.yaw);
    if previous != MovementMode::Swimming {
        let magnitude = input_magnitude(observed);
        if !observed.sprinting
            || observed.swim_entry_direction_invalid(magnitude)
            || !head_in_water(world, observed)?
        {
            return Ok(false);
        }
        if (look.y as f32) < ENTRY_UPWARD_LIMIT {
            return Ok(true);
        }
        let attach = attach_cell(observed);
        let height = previous.hitbox_height(observed.sneaking) as f32;
        let center = [
            observed.feet.x as f32,
            observed.feet.y as f32 + height * 0.5,
            observed.feet.z as f32,
        ];
        let mut above = center.map(|axis| axis.floor() as i32);
        above[1] += 1;
        return Ok(primary_is_air(world, attach)? == Some(false)
            && primary_is_air(world, above)? == Some(false));
    }

    // Hunger, mounting and desktop/touch input determine separate native stop
    // requests. Continued swimming does not require the sprint flag or forward input.
    let keep = input_magnitude(observed) >= MIN_SWIM_INPUT
        && !intent.swim_hunger_blocked
        && observed.in_water;
    if keep {
        let attach_air = primary_is_air(world, attach_cell(observed))?;
        if attach_air != Some(true) || surface_angle_keeps_swimming(look) {
            return Ok(true);
        }
    }
    // Native stop action is emitted only while the standing probe is clear.
    Ok(!standing_fits(world, observed.feet)?)
}

fn surface_angle_keeps_swimming(look: Vec3) -> bool {
    let x = look.x as f32;
    let z = look.z as f32;
    let angle = (x * x + z * z).acos() * NATIVE_DEGREES_PER_RADIAN;
    // These native positive comparisons intentionally preserve NaN behavior.
    angle <= SURFACE_KEEP_ANGLE || look.y as f32 <= 0.0
}

impl ModeObservation {
    fn swim_entry_direction_invalid(self, magnitude: f32) -> bool {
        magnitude < MIN_SWIM_INPUT
            || self.move_forward <= 0.0
            || self.move_sideways.abs() > MIN_SWIM_INPUT
    }
}

fn input_magnitude(observed: ModeObservation) -> f32 {
    (observed.move_sideways * observed.move_sideways
        + observed.move_forward * observed.move_forward)
        .sqrt()
}

fn attach_cell(observed: ModeObservation) -> [i32; 3] {
    [
        observed.feet.x as f32,
        observed.feet.y as f32 + observed.liquid_attach_height,
        observed.feet.z as f32,
    ]
    .map(|axis| axis.floor() as i32)
}

fn primary_is_air(
    world: &(impl CollisionWorld + ?Sized),
    cell: [i32; 3],
) -> Result<Option<bool>, WorldQueryError> {
    Ok(world.primary_is_air(cell)?.map(|query| query.value))
}

fn head_in_water(
    world: &(impl CollisionWorld + ?Sized),
    observed: ModeObservation,
) -> Result<bool, WorldQueryError> {
    Ok(sim::sample_water_head(
        world,
        observed.feet,
        f64::from(observed.liquid_attach_height),
    )?
    .value)
}

fn standing_fits(
    world: &(impl CollisionWorld + ?Sized),
    feet: Vec3,
) -> Result<bool, WorldQueryError> {
    sim::pose_fits(world, feet, MovementMode::Walking, false)
}

#[cfg(test)]
mod tests;
