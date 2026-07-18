mod collision;
mod environment;
mod input;
mod state;

use crate::{
    CollisionWorld, Vec3,
    math::{minecraft_cos, minecraft_sin},
};
use collision::{clip_sneak_edge, resolve_motion};
use environment::sample;

pub use environment::MAX_BLOCK_SAMPLES_PER_TICK;
pub use input::MovementInput;
pub use state::{AxisCollisions, MovementEnvironment, PlayerState, SimulationError, TickResult};

pub(crate) fn validate_player_state(state: &PlayerState) -> Result<(), SimulationError> {
    state::validate(state)
}

pub const TICKS_PER_SECOND: u32 = 20;
const DEFAULT_JUMP_HEIGHT: f64 = 0.42;
const DEFAULT_AIR_FRICTION: f64 = 0.91;
const NORMAL_GRAVITY_MULTIPLIER: f64 = 0.98;
const NORMAL_GRAVITY: f64 = 0.08;
const STEP_HEIGHT: f64 = 0.6;
const DEFAULT_MOVEMENT_SPEED: f64 = 0.1;
const DEFAULT_AIR_SPEED: f64 = 0.02;
const SPRINT_AIR_SPEED: f64 = 0.026;
const SPRINT_SPEED_MULTIPLIER: f64 = 1.3;
const SNEAK_INPUT_MULTIPLIER: f64 = 0.3;
const INPUT_IMPULSE_MULTIPLIER: f64 = 0.98;
const SPRINT_JUMP_IMPULSE: f64 = 0.2;
const JUMP_DELAY_TICKS: u8 = 10;
const COLLISION_EPSILON: f64 = 1.0e-5;

#[derive(Debug, Clone, Copy)]
pub struct Simulator {
    movement_speed: f64,
}

impl Default for Simulator {
    fn default() -> Self {
        Self {
            movement_speed: DEFAULT_MOVEMENT_SPEED,
        }
    }
}

impl Simulator {
    /// Advances exactly one 20 Hz Bedrock movement tick transactionally.
    pub fn tick(
        &self,
        state: &mut PlayerState,
        input: MovementInput,
        world: &impl CollisionWorld,
    ) -> Result<TickResult, SimulationError> {
        state::validate(state)?;
        input::validate(input)?;
        let mut next = state.clone();
        next.tick = next
            .tick
            .checked_add(1)
            .ok_or(SimulationError::TickOverflow)?;
        if next.velocity.length_squared() < 1.0e-12 {
            next.velocity = Vec3::ZERO;
        }

        if !input.jumping {
            next.jump_delay = 0;
        }
        let grounded_at_start = next.on_ground;
        let sampled = sample(world, next.position, next.velocity)?;
        let friction = if grounded_at_start {
            DEFAULT_AIR_FRICTION * sampled.friction
        } else {
            DEFAULT_AIR_FRICTION
        };
        let relative_speed = if grounded_at_start {
            let speed = self.movement_speed
                * if input.sprinting {
                    SPRINT_SPEED_MULTIPLIER
                } else {
                    1.0
                };
            speed
                * sampled.movement.horizontal_speed_factor
                * (0.162_771_36 / (friction * friction * friction))
        } else if sampled.movement.in_water || sampled.movement.in_lava {
            DEFAULT_AIR_SPEED * sampled.movement.horizontal_speed_factor
        } else if input.sprinting {
            SPRINT_AIR_SPEED
        } else {
            DEFAULT_AIR_SPEED
        };

        let max_input = if input.sneaking {
            SNEAK_INPUT_MULTIPLIER
        } else {
            1.0
        };
        apply_relative_movement(
            &mut next.velocity,
            input.strafe.clamp(-max_input, max_input) * INPUT_IMPULSE_MULTIPLIER,
            input.forward.clamp(-max_input, max_input) * INPUT_IMPULSE_MULTIPLIER,
            input.yaw_degrees,
            relative_speed,
        );

        if input.jump_pressed && next.on_ground && next.jump_delay == 0 {
            next.velocity.y = next.velocity.y.max(DEFAULT_JUMP_HEIGHT);
            next.jump_delay = JUMP_DELAY_TICKS;
            if input.sprinting {
                let yaw = input.yaw_degrees.to_radians();
                next.velocity.x -= minecraft_sin(yaw) * SPRINT_JUMP_IMPULSE;
                next.velocity.z += minecraft_cos(yaw) * SPRINT_JUMP_IMPULSE;
            }
        }

        if sampled.movement.on_climbable || sampled.movement.in_scaffolding {
            next.velocity.y = next.velocity.y.max(-0.2);
            if input.jumping {
                next.velocity.y = 0.2;
            } else if input.sneaking && next.velocity.y < 0.0 {
                next.velocity.y = 0.0;
            }
        }
        if sampled.movement.in_water || sampled.movement.in_lava {
            if input.jumping {
                next.velocity.y += 0.04;
            }
            next.velocity.y *= sampled.movement.vertical_speed_factor;
        }
        if sampled.movement.in_cobweb {
            next.velocity.x *= 0.25;
            next.velocity.y *= 0.05;
            next.velocity.z *= 0.25;
        } else if sampled.movement.in_powder_snow {
            next.velocity.x *= sampled.movement.horizontal_speed_factor;
            next.velocity.y *= sampled.movement.vertical_speed_factor;
            next.velocity.z *= sampled.movement.horizontal_speed_factor;
        }
        let mut identity = sampled.identity;
        if input.sneaking && grounded_at_start && next.velocity.y <= 0.0 {
            let (clipped, edge_identity) = clip_sneak_edge(world, next.position, next.velocity)?;
            next.velocity = clipped;
            if let Some(edge_identity) = edge_identity {
                identity = identity.merge(&edge_identity)?;
            }
        }

        let pre_collision_velocity = next.velocity;
        let motion = resolve_motion(world, next.position, next.velocity, grounded_at_start)?;
        identity = identity.merge(&motion.identity)?;
        next.position += motion.resolved;
        next.movement = motion.resolved;
        next.on_ground = motion.stepped
            || (motion.collisions.y && next.velocity.y < 0.0)
            || (grounded_at_start
                && !motion.collisions.y
                && next.velocity.y.abs() <= COLLISION_EPSILON);
        next.velocity = motion.resolved;
        if motion.stepped {
            next.velocity.y = 0.0;
        }
        if motion.collisions.x {
            next.velocity.x = 0.0;
        }
        if motion.collisions.y {
            next.velocity.y = match sampled.movement.surface_response {
                crate::SurfaceResponse::Slime
                    if !grounded_at_start && !input.sneaking && pre_collision_velocity.y < 0.0 =>
                {
                    -pre_collision_velocity.y
                }
                crate::SurfaceResponse::Bed
                    if !grounded_at_start && pre_collision_velocity.y < 0.0 =>
                {
                    (-0.66 * pre_collision_velocity.y).min(1.0)
                }
                _ => 0.0,
            };
        }
        if motion.collisions.z {
            next.velocity.z = 0.0;
        }

        if sampled.movement.in_cobweb {
            next.velocity = Vec3::ZERO;
        } else if sampled.movement.in_water || sampled.movement.in_lava {
            let drag = if sampled.movement.in_lava { 0.5 } else { 0.8 };
            next.velocity.x *= drag;
            next.velocity.y = (next.velocity.y - 0.02) * drag;
            next.velocity.z *= drag;
        } else {
            next.velocity.y = (next.velocity.y - NORMAL_GRAVITY) * NORMAL_GRAVITY_MULTIPLIER;
            next.velocity.x *= friction;
            next.velocity.z *= friction;
        }
        match sampled.movement.surface_response {
            crate::SurfaceResponse::BubbleUp => next.velocity.y = next.velocity.y.max(0.1),
            crate::SurfaceResponse::BubbleDown => next.velocity.y = next.velocity.y.min(-0.1),
            _ => {}
        }
        next.jump_delay = next.jump_delay.saturating_sub(1);

        let result = TickResult {
            tick: next.tick,
            position: next.position,
            velocity: next.velocity,
            movement: next.movement,
            collisions: motion.collisions,
            on_ground: next.on_ground,
            environment: sampled.movement,
            world_identity: identity,
        };
        *state = next;
        Ok(result)
    }
}

fn apply_relative_movement(
    velocity: &mut Vec3,
    strafe: f64,
    forward: f64,
    yaw_degrees: f64,
    relative_speed: f64,
) {
    let force_squared = forward.mul_add(forward, strafe * strafe);
    if force_squared < 1.0e-4 {
        return;
    }
    let force = relative_speed / force_squared.sqrt().max(1.0);
    let forward = forward * force;
    let strafe = strafe * force;
    let yaw = yaw_degrees.to_radians();
    let sin = minecraft_sin(yaw);
    let cos = minecraft_cos(yaw);
    velocity.x += strafe * cos - forward * sin;
    velocity.z += forward * cos + strafe * sin;
}
