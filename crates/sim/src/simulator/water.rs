//! Vanilla liquid drag, jump ascent and swimming pitch steering.

use crate::{
    Aabb, BlockPhysicsFlags, CollisionQuery, CollisionWorld, Vec3, WorldQueryError,
    math::minecraft_sin,
};

use super::{
    DEPTH_STRIDER_MAX_LEVEL, DEPTH_STRIDER_TARGET_DRAG, MovementInput, MovementMode, WATER_DRAG,
};

// Head-water sensing compares the eye against liquid level / 9, offset one level down.
const LIQUID_LEVEL_DIVISOR: f32 = 9.0;
const HEAD_SURFACE_OFFSET: f32 = -1.0 / LIQUID_LEVEL_DIVISOR;

/// Native head-water sensing at pose-adjusted attach location 7. Unlike the
/// swimming steering guard, this requires the primary water material and tests
/// the liquid level against the eye, preserving the sampled world identity.
pub fn sample_water_head(
    world: &(impl CollisionWorld + ?Sized),
    feet: Vec3,
    height: f64,
) -> Result<CollisionQuery<bool>, WorldQueryError> {
    let point = Vec3::new(
        f64::from(feet.x as f32),
        f64::from(feet.y as f32 + height as f32),
        f64::from(feet.z as f32),
    );
    submerged(world, point, &[BlockPhysicsFlags::WATER])
}

/// Whether `point` lies below the surface of the water or lava filling its block, as vanilla
/// tests an actor's breathing point.
pub fn sample_liquid_submersion(
    world: &(impl CollisionWorld + ?Sized),
    point: Vec3,
) -> Result<CollisionQuery<bool>, WorldQueryError> {
    submerged(
        world,
        point,
        &[BlockPhysicsFlags::WATER, BlockPhysicsFlags::LAVA],
    )
}

/// Compares the point with the primary layer's liquid level / 9, offset one level down.
fn submerged(
    world: &(impl CollisionWorld + ?Sized),
    point: Vec3,
    liquids: &[BlockPhysicsFlags],
) -> Result<CollisionQuery<bool>, WorldQueryError> {
    let block = super::environment::block_at(point)?;
    let sample = world.block_physics(block)?;
    let facts = sample.primary();
    let in_liquid = if liquids.iter().any(|liquid| facts.flags.contains(*liquid)) {
        let level = if facts.fluid_height_blocks >= 1.0 {
            1.0
        } else {
            (LIQUID_LEVEL_DIVISOR
                - (facts.fluid_height_blocks as f32 * LIQUID_LEVEL_DIVISOR).round())
            .clamp(1.0, 8.0)
        };
        let surface =
            (i64::from(block[1]) + 1) as f32 - (level / LIQUID_LEVEL_DIVISOR + HEAD_SURFACE_OFFSET);
        (point.y as f32) < surface
    } else {
        false
    };
    Ok(CollisionQuery {
        value: in_liquid,
        identity: sample.identity,
    })
}

/// The swim-amount blend advances before the jump step each tick. Crawl flag
/// 114 also advances the blend.
pub(super) fn advance_swim_amount(previous: f32, previous_pose_active: bool) -> f32 {
    if previous_pose_active {
        (previous + 0.1_f32).min(1.0)
    } else {
        (previous - 0.1_f32).max(0.0)
    }
}

/// Vanilla suppresses every jump path while the blend
/// is partial, and a swimming jump additionally requires head water.
pub(super) fn jump_suppressed(
    mode: MovementMode,
    swim_amount: f32,
    head_in_water: Option<bool>,
) -> bool {
    (swim_amount > 0.0 && swim_amount < 1.0)
        || (mode == MovementMode::Swimming && head_in_water == Some(false))
}

/// Bounding-box input update samples the primary material cell at attach 7.
/// Its liquid flag is independent of the block's rendered fluid height.
pub(super) fn sample_attach(
    world: &impl CollisionWorld,
    feet: Vec3,
    height: f64,
    previous_samples: usize,
) -> Result<CollisionQuery<bool>, WorldQueryError> {
    if previous_samples == super::MAX_BLOCK_SAMPLES_PER_TICK {
        return Err(WorldQueryError::QueryExtentExceeded);
    }
    let point = Vec3::new(
        f64::from(feet.x as f32),
        f64::from(feet.y as f32 + height as f32),
        f64::from(feet.z as f32),
    );
    let sample = world.block_physics(super::environment::block_at(point)?)?;
    let flags = sample.primary().flags;
    Ok(CollisionQuery {
        value: flags.contains(BlockPhysicsFlags::WATER) || flags.contains(BlockPhysicsFlags::LAVA),
        identity: sample.identity,
    })
}

// Horizontal water drag while sprinting.
const SPRINT_WATER_DRAG: f32 = 0.9;
// Upward impulse per tick while jumping in liquid.
const LIQUID_JUMP_ACCELERATION: f32 = 0.04;
// Downward impulse per tick while sneaking in water.
const WATER_SINK_ACCELERATION: f32 = -0.04;
// Swimming pitch steering rates; the faster rate applies below the dive threshold.
const SWIM_STEER_RATE: f32 = 0.06;
const SWIM_DIVE_STEER_RATE: f32 = 0.085;
const SWIM_DIVE_THRESHOLD: f32 = -0.2;
// Liquid exit: probe raised by 0.6, then set vertical velocity to 0.3.
const LIQUID_EXIT_RAISE: f32 = 0.6;
const LIQUID_EXIT_VELOCITY: f32 = 0.3;

/// Liquid travel's horizontal collision checks the resolved pose box above the
/// surface after drag/gravity. Both water and lava, with any pose, use this path.
pub(super) fn climb_out(
    world: &impl CollisionWorld,
    aabb: Aabb,
    previous_y: f64,
    position_y: f64,
    velocity: &mut Vec3,
    previous_samples: usize,
) -> Result<CollisionQuery<()>, WorldQueryError> {
    let raise = (previous_y as f32 - position_y as f32) + LIQUID_EXIT_RAISE + velocity.y as f32;
    let delta = [velocity.x as f32, raise, velocity.z as f32];
    let translated = |face: Vec3| {
        Vec3::new(
            f64::from(face.x as f32 + delta[0]),
            f64::from(face.y as f32 + delta[1]),
            f64::from(face.z as f32 + delta[2]),
        )
    };
    let probe = Aabb::new(translated(aabb.min), translated(aabb.max));
    let (liquid, mut identity) =
        super::environment::contains_liquid(world, probe, previous_samples)?;
    if !liquid {
        let occupied = super::collision::has_collision(world, probe)?;
        identity = identity.merge(&occupied.identity)?;
        if !occupied.value {
            velocity.y = f64::from(LIQUID_EXIT_VELOCITY);
        }
    }
    Ok(CollisionQuery {
        value: (),
        identity,
    })
}

/// Water drag modifies each retained velocity after movement. Depth Strider
/// adjusts the horizontal axes; vertical retention remains the water default.
pub(super) fn apply_drag(velocity: &mut Vec3, input: &MovementInput, depth_strider: f64) {
    let base = if input.sprinting {
        SPRINT_WATER_DRAG
    } else {
        WATER_DRAG as f32
    };
    let horizontal = base
        + (DEPTH_STRIDER_TARGET_DRAG as f32 - base)
            * (depth_strider as f32 / f32::from(DEPTH_STRIDER_MAX_LEVEL));
    velocity.x = f64::from(velocity.x as f32 * horizontal);
    velocity.y = f64::from(velocity.y as f32 * WATER_DRAG as f32);
    velocity.z = f64::from(velocity.z as f32 * horizontal);
}

/// The ordinary held liquid jump adds ascent before collision resolution.
pub(super) fn jump(velocity_y: &mut f64) {
    *velocity_y = f64::from(*velocity_y as f32 + LIQUID_JUMP_ACCELERATION);
}

/// Held water descent adds its own downward force while ability flight is off.
pub(super) fn sink(velocity_y: &mut f64) {
    *velocity_y = f64::from(*velocity_y as f32 + WATER_SINK_ACCELERATION);
}

/// Swimming pitch steering runs only without a held jump; a held jump ascends
/// through the jump step instead.
pub(super) fn steer(velocity_y: &mut f64, input: &MovementInput, attach_in_liquid: Option<bool>) {
    if input.jumping {
        return;
    }
    let pitch = input.pitch_degrees as f32;
    let target = minecraft_sin(f64::from(-pitch.to_radians())) as f32;
    // Ordinary upward steering requires the liquid material flag written by
    // the bounding-box input update, even if velocity was falling.
    if target > 0.0 && attach_in_liquid == Some(false) {
        *velocity_y = 0.0;
        return;
    }
    let rate = if target < SWIM_DIVE_THRESHOLD {
        SWIM_DIVE_STEER_RATE
    } else {
        SWIM_STEER_RATE
    };
    let previous = *velocity_y as f32;
    *velocity_y = f64::from(rate * (target - previous) + previous);
}
