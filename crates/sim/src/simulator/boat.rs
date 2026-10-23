//! Movement of a boat that the local player drives and the client predicts.
//!
//! One tick runs, in order: paddle force from the rider's input, momentum from the block
//! the boat sits in or on, steering, an unstepped collision move, then buoyancy (wave
//! timer, float test, gravity and the float push). Arithmetic stays in `f32` where the
//! boat's own state is single precision.

use crate::{
    Aabb, BlockPhysicsFlags, CollisionWorld, Vec3, WorldCollisionIdentity, WorldQueryError,
};

use super::collision::resolve_unstepped_motion;

/// A boat's collision box and position origin, from its actor facts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoatBody {
    /// Collision box width and depth.
    pub width: f32,
    /// Collision box height.
    pub height: f32,
    /// How far the network position sits above the bottom of the box.
    pub height_offset: f32,
}

/// Paddle force for full input.
const PADDLE_FORCE: f32 = 3.0;
/// A new paddle stroke begins this many ticks after the previous one.
const STROKE_TICKS: i64 = 10;
/// Force lost between stroke starts.
const STROKE_FORCE_LOSS: f32 = 0.1;
/// Reversing scales the paddle input by this factor.
const REVERSE_POWER: f32 = -0.15;
/// Paddle force to forward speed.
const PADDLE_SPEED: f32 = 0.013_75;
/// Forward speed of one paddle to turning torque.
const PADDLE_TORQUE: f32 = 3.0;
/// Torque to degrees of angular velocity.
const TORQUE_DEGREES: f32 = 10.0;
/// Squared horizontal speed below which turning is boosted.
const SLOW_SPEED_SQUARED: f32 = 0.01;
/// Torque boost while nearly stopped.
const SLOW_TURN_BOOST: f32 = 1.6;
/// Momentum while floating or resurfacing in water.
const WATER_MOMENTUM: f32 = 0.9;
/// Momentum in the air.
const AIR_MOMENTUM: f32 = 1.0;
/// The rider loses control after resurfacing for this many ticks.
const OUT_OF_CONTROL_TICKS: u32 = 25;
/// Moves longer than this many blocks are shortened to it.
const MAX_MOVE_DISTANCE: f32 = 16.0;
/// Buoyancy gravity and drag.
const GRAVITY: f32 = 0.04;
const VERTICAL_DRAG: f32 = 0.98;
/// Float strength of a fully submerged boat.
const BASE_BUOYANCY: f32 = 1.0;
/// Wave height and timer.
const WAVE_AMPLITUDE: f32 = 0.035;
const WAVE_SPEED_SCALE: f64 = 30.0;
const WAVE_TIMER_STEP: f64 = 0.05;
const BIG_WAVE_PROBABILITY: f32 = 0.03;
const BIG_WAVE_SPEED: f32 = 10.0;

/// Paddle state: the stroke clock, each paddle's stroke start and current force.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoatPaddles {
    /// Ticks the paddles have been driven.
    pub clock: i64,
    /// Clock value at each paddle's current stroke start; -1 when the paddle rests.
    pub stroke_start: [i64; 2],
    /// Left and right paddle force.
    pub force: [f32; 2],
}

impl Default for BoatPaddles {
    fn default() -> Self {
        Self {
            clock: 0,
            stroke_start: [0; 2],
            force: [0.0; 2],
        }
    }
}

/// The predicted state of one boat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoatState {
    /// Collision box and position origin.
    pub body: BoatBody,
    /// Network position: centre of the box, `body.height_offset` above its bottom.
    pub position: [f32; 3],
    /// Displacement applied next tick.
    pub velocity: [f32; 3],
    /// Facing in degrees; the boat travels along `(cos yaw, sin yaw)`.
    pub yaw: f32,
    /// Degrees added to `yaw` each tick.
    pub angular_velocity: f32,
    pub on_ground: bool,
    pub paddles: BoatPaddles,
    /// Consecutive ticks spent resurfacing.
    pub out_of_control_ticks: u32,
    /// Phase of the surface wave.
    pub wave_timer: f64,
}

impl BoatState {
    /// A resting boat at a network position.
    #[must_use]
    pub fn at_rest(body: BoatBody, position: [f32; 3], yaw: f32) -> Self {
        Self {
            body,
            position,
            velocity: [0.0; 3],
            yaw,
            angular_velocity: 0.0,
            on_ground: false,
            paddles: BoatPaddles::default(),
            out_of_control_ticks: 0,
            wave_timer: 0.0,
        }
    }

    /// Collision box at the current position.
    #[must_use]
    pub fn collision_box(&self) -> Aabb {
        let half = self.body.width * 0.5;
        let [x, _, z] = self.position;
        let bottom = self.feet_y();
        Aabb::new(
            Vec3::new(f64::from(x - half), f64::from(bottom), f64::from(z - half)),
            Vec3::new(
                f64::from(x + half),
                f64::from(bottom + self.body.height),
                f64::from(z + half),
            ),
        )
    }

    /// Height of the bottom of the collision box.
    #[must_use]
    pub fn feet_y(&self) -> f32 {
        self.position[1] - self.body.height_offset
    }

    fn block(&self) -> [i32; 3] {
        self.position.map(|axis| axis.floor() as i32)
    }
}

/// Left and right paddle power for a move vector whose `x` is positive to the left and
/// `y` positive forward. Reversing mirrors steering and paddles weakly backwards.
#[must_use]
pub fn boat_paddle_power(move_vector: [f32; 2]) -> [f32; 2] {
    let mut x = move_vector[0];
    let y = move_vector[1].clamp(-1.0, 1.0);
    let mut magnitude = (x * x + y * y).sqrt();
    if y < 0.0 {
        x = -x;
        magnitude *= REVERSE_POWER;
    }
    let left = if x <= 0.0 {
        1.0
    } else if x <= 1.0 {
        1.0 - x
    } else {
        0.0
    };
    let right = if x > 0.0 { 1.0 } else { x.max(-1.0) + 1.0 };
    [magnitude * left, magnitude * right]
}

/// Whether the boat floats on or must resurface through the liquid it sits in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoatFloat {
    pub can_float: bool,
    pub resurface: bool,
}

/// Where a tick's buoyancy decision comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoatBuoyancy {
    /// A live tick: test the liquid and advance the wave with this uniform draw in `[0, 1)`.
    Live { wave_draw: f32 },
    /// A replayed tick reuses the recorded float decision and leaves the wave alone.
    Replay(BoatFloat),
}

/// One completed boat tick.
#[derive(Debug, Clone, PartialEq)]
pub struct BoatTick {
    pub state: BoatState,
    /// The float decision this tick used, for replay.
    pub float: BoatFloat,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub identity: WorldCollisionIdentity,
}

/// Advances a driven boat by one tick with the paddle power from [`boat_paddle_power`].
pub fn tick_boat(
    world: &impl CollisionWorld,
    previous: &BoatState,
    paddle_power: [f32; 2],
    buoyancy: BoatBuoyancy,
) -> Result<BoatTick, WorldQueryError> {
    let [x, _, z] = previous.position;
    world.ensure_movement_origin(Vec3::new(
        f64::from(x),
        f64::from(previous.feet_y()),
        f64::from(z),
    ))?;
    let mut state = *previous;
    drive_paddles(&mut state.paddles, paddle_power);

    let start_float = float_test(world, &state)?;
    let mut identity = start_float.identity;
    let (momentum, airborne) = momentum(world, &mut state, start_float.value, &mut identity)?;
    state.velocity[0] *= momentum;
    state.velocity[2] *= momentum;
    state.angular_velocity *= momentum;
    if state.out_of_control_ticks < OUT_OF_CONTROL_TICKS {
        steer(&mut state, momentum, airborne);
    }

    let mut request = state.velocity;
    let length_squared = request.iter().map(|axis| axis * axis).sum::<f32>();
    if length_squared > MAX_MOVE_DISTANCE * MAX_MOVE_DISTANCE {
        let scale = MAX_MOVE_DISTANCE / length_squared.sqrt();
        request = request.map(|axis| axis * scale);
    }
    let motion = resolve_unstepped_motion(
        world,
        state.collision_box(),
        Vec3::new(
            f64::from(request[0]),
            f64::from(request[1]),
            f64::from(request[2]),
        ),
    )?;
    identity = identity.merge(&motion.identity)?;
    let moved = [motion.resolved.x, motion.resolved.y, motion.resolved.z].map(|axis| axis as f32);
    let collided = std::array::from_fn::<bool, 3, _>(|axis| {
        (request[axis] - moved[axis]).abs() > f32::EPSILON
    });
    state.position = [
        motion.position.x as f32,
        motion.aabb.min.y as f32 + state.body.height_offset,
        motion.position.z as f32,
    ];
    state.on_ground = if collided[1] {
        request[1] < 0.0
    } else {
        state.on_ground && request[1] == 0.0
    };
    for (velocity, collided) in state.velocity.iter_mut().zip(collided) {
        if collided {
            *velocity = 0.0;
        }
    }

    let float = match buoyancy {
        BoatBuoyancy::Live { wave_draw } => {
            advance_wave(&mut state, wave_draw);
            let tested = float_test(world, &state)?;
            identity = identity.merge(&tested.identity)?;
            tested.value
        }
        BoatBuoyancy::Replay(float) => float,
    };
    state.velocity[1] = (state.velocity[1] - GRAVITY) * VERTICAL_DRAG;
    apply_float(&mut state, float);

    Ok(BoatTick {
        state,
        float,
        horizontal_collision: collided[0] || collided[2],
        vertical_collision: collided[1],
        identity,
    })
}

/// Turns paddle power into force. A stroke starts at full force; until the next stroke
/// the force is slightly weaker.
fn drive_paddles(paddles: &mut BoatPaddles, power: [f32; 2]) {
    for (paddle, power) in power.into_iter().enumerate() {
        if power == 0.0 {
            paddles.force[paddle] = power;
        } else if paddles.clock.wrapping_sub(paddles.stroke_start[paddle]) < STROKE_TICKS {
            let weakened = ((PADDLE_FORCE * power).abs() - STROKE_FORCE_LOSS).max(0.0);
            paddles.force[paddle] = weakened.copysign(power);
        } else {
            paddles.force[paddle] = PADDLE_FORCE * power;
            paddles.stroke_start[paddle] = paddles.clock;
        }
    }
    paddles.clock = paddles.clock.wrapping_add(1);
}

/// Momentum and whether the paddles find no surface, from the block at the boat's position.
fn momentum(
    world: &impl CollisionWorld,
    state: &mut BoatState,
    float: BoatFloat,
    identity: &mut WorldCollisionIdentity,
) -> Result<(f32, bool), WorldQueryError> {
    if float.can_float {
        state.out_of_control_ticks = 0;
        return Ok((WATER_MOMENTUM, false));
    }
    if float.resurface {
        state.out_of_control_ticks = state.out_of_control_ticks.saturating_add(1);
        return Ok((WATER_MOMENTUM, false));
    }
    let block = state.block();
    let below = [block[0], block[1] - 1, block[2]];
    let sample = world.block_physics(block)?;
    *identity = identity.merge(&sample.identity)?;
    if sample.primary().flags.contains(BlockPhysicsFlags::WATER) {
        return Ok((WATER_MOMENTUM, false));
    }
    let air = match world.primary_is_air(block)? {
        Some(query) => {
            *identity = identity.merge(&query.identity)?;
            query.value
        }
        None => false,
    };
    if air {
        let below_sample = world.block_physics(below)?;
        *identity = identity.merge(&below_sample.identity)?;
        let below_facts = below_sample.primary();
        if state.on_ground {
            return Ok((below_facts.friction as f32, false));
        }
        if below_facts.flags.contains(BlockPhysicsFlags::WATER) {
            return Ok((WATER_MOMENTUM, false));
        }
        return Ok((AIR_MOMENTUM, true));
    }
    if !state.on_ground {
        return Ok((AIR_MOMENTUM, true));
    }
    // A block with its own collision shape supplies the friction; otherwise the one below.
    let origin = Vec3::new(
        f64::from(block[0]),
        f64::from(block[1]),
        f64::from(block[2]),
    );
    let shapes = world.collision_boxes_with_provenance(Aabb::new(origin, origin + Vec3::ONE))?;
    *identity = identity.merge(&shapes.identity)?;
    let solid = shapes
        .value
        .iter()
        .any(|shape| shape.block == Some(block) && !shape.aabb.is_zero_volume());
    let friction = if solid {
        sample.primary().friction
    } else {
        let below_sample = world.block_physics(below)?;
        *identity = identity.merge(&below_sample.identity)?;
        below_sample.primary().friction
    };
    Ok((friction as f32, false))
}

/// Paddle forces push the boat forward and turn it.
fn steer(state: &mut BoatState, momentum: f32, airborne: bool) {
    let [left, right] = state.paddles.force.map(|force| force * PADDLE_SPEED);
    let mut forward = 0.0_f32;
    let mut torque = 0.0_f32;
    if !airborne {
        forward = left + right;
        torque = PADDLE_TORQUE * left - PADDLE_TORQUE * right;
    }
    for (paddle, push) in [left, right].into_iter().enumerate() {
        if push == 0.0 {
            state.paddles.stroke_start[paddle] = -1;
        }
    }
    let [velocity_x, _, velocity_z] = state.velocity;
    if velocity_z * velocity_z + velocity_x * velocity_x < SLOW_SPEED_SQUARED && torque != 0.0 {
        forward *= momentum;
        torque *= SLOW_TURN_BOOST;
    }
    state.angular_velocity = (torque * TORQUE_DEGREES + state.angular_velocity) * momentum;
    state.yaw += state.angular_velocity;
    let (sin, cos) = ((90.0 - state.yaw) * std::f32::consts::PI / 180.0).sin_cos();
    state.velocity[0] = (forward * sin + state.velocity[0]) * momentum;
    state.velocity[2] = (forward * cos + state.velocity[2]) * momentum;
}

/// Advances the wave phase faster the quicker the boat moves, with occasional big waves.
fn advance_wave(state: &mut BoatState, draw: f32) {
    let [velocity_x, _, velocity_z] = state.velocity;
    let speed = (velocity_z * velocity_z + velocity_x * velocity_x).sqrt();
    let mut step = f64::from(speed) * WAVE_SPEED_SCALE + 1.0;
    if draw <= BIG_WAVE_PROBABILITY {
        step *= f64::from(BIG_WAVE_SPEED);
    }
    state.wave_timer += f64::from((step * WAVE_TIMER_STEP) as f32);
}

/// Floating needs liquid at the boat's position with open air above; resurfacing needs
/// liquid both there and above.
fn float_test(
    world: &impl CollisionWorld,
    state: &BoatState,
) -> Result<crate::CollisionQuery<BoatFloat>, WorldQueryError> {
    let block = state.block();
    let here = world.block_physics(block)?;
    let above = world.block_physics([block[0], block[1] + 1, block[2]])?;
    let identity = here.identity.merge(&above.identity)?;
    let liquid = |facts: &crate::BlockPhysicsFacts| facts.flags.contains(BlockPhysicsFlags::WATER);
    let in_liquid = liquid(here.primary());
    let liquid_above = liquid(above.primary());
    let below_surface =
        state.position[1] < crate::fluid::liquid_surface_y(*here.primary(), block[1]);
    Ok(crate::CollisionQuery {
        value: BoatFloat {
            can_float: in_liquid && !liquid_above && below_surface,
            resurface: in_liquid && liquid_above,
        },
        identity,
    })
}

/// Pushes a floating boat towards the surface, bobbing on the wave.
fn apply_float(state: &mut BoatState, float: BoatFloat) {
    let (submersion, wave) = if float.can_float {
        let y = state.position[1];
        let submersion = ((BASE_BUOYANCY - (y - y.floor())) * 0.9 + 0.1).clamp(0.0, 1.0);
        if submersion == 0.0 {
            return;
        }
        let wave = (state.wave_timer.sin() as f32 + 1.0) * WAVE_AMPLITUDE;
        (submersion, wave)
    } else if float.resurface {
        (1.0, 0.0)
    } else {
        return;
    };
    let target = (submersion - wave - 0.1) * 0.15;
    state.velocity[1] = target.min(state.velocity[1] * 0.7 + 0.05);
}

#[cfg(test)]
#[path = "boat_tests.rs"]
mod tests;
