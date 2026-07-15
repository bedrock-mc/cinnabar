use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    Aabb, CollisionWorld, Vec3, WorldQueryError,
    math::{minecraft_cos, minecraft_sin},
};

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct MovementInput {
    pub strafe: f64,
    pub forward: f64,
    pub yaw_degrees: f64,
    pub jumping: bool,
    pub jump_pressed: bool,
    pub sprinting: bool,
    pub sneaking: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub tick: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub movement: Vec3,
    pub on_ground: bool,
    pub jump_delay: u8,
}

impl PlayerState {
    #[must_use]
    pub const fn new(position: Vec3) -> Self {
        Self {
            tick: 0,
            position,
            velocity: Vec3::ZERO,
            movement: Vec3::ZERO,
            on_ground: false,
            jump_delay: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxisCollisions {
    pub x: bool,
    pub y: bool,
    pub z: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TickResult {
    pub tick: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub movement: Vec3,
    pub collisions: AxisCollisions,
    pub on_ground: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SimulationError {
    #[error(transparent)]
    World(#[from] WorldQueryError),
    #[error("movement tick overflow")]
    TickOverflow,
}

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
    /// Advances exactly one 20 Hz Bedrock movement tick.
    ///
    /// The update is transactional: query failure or tick overflow leaves the
    /// caller's state byte-for-byte unchanged.
    pub fn tick(
        &self,
        state: &mut PlayerState,
        input: MovementInput,
        world: &impl CollisionWorld,
    ) -> Result<TickResult, SimulationError> {
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
        let friction = if grounded_at_start {
            DEFAULT_AIR_FRICTION * world.block_friction(block_below(next.position)?)?
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
            speed * (0.162_771_36 / (friction * friction * friction))
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
        let strafe = input.strafe.clamp(-max_input, max_input) * INPUT_IMPULSE_MULTIPLIER;
        let forward = input.forward.clamp(-max_input, max_input) * INPUT_IMPULSE_MULTIPLIER;
        apply_relative_movement(
            &mut next.velocity,
            strafe,
            forward,
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

        let motion = resolve_motion(world, next.position, next.velocity, grounded_at_start)?;
        next.position += motion.resolved;
        next.movement = motion.resolved;
        next.on_ground = (motion.collisions.y && next.velocity.y < 0.0)
            || (grounded_at_start
                && !motion.collisions.y
                && next.velocity.y.abs() <= COLLISION_EPSILON);
        next.velocity = motion.resolved;
        if motion.collisions.x {
            next.velocity.x = 0.0;
        }
        if motion.collisions.y {
            next.velocity.y = 0.0;
        }
        if motion.collisions.z {
            next.velocity.z = 0.0;
        }

        next.velocity.y = (next.velocity.y - NORMAL_GRAVITY) * NORMAL_GRAVITY_MULTIPLIER;
        next.velocity.x *= friction;
        next.velocity.z *= friction;
        next.jump_delay = next.jump_delay.saturating_sub(1);

        let result = TickResult {
            tick: next.tick,
            position: next.position,
            velocity: next.velocity,
            movement: next.movement,
            collisions: motion.collisions,
            on_ground: next.on_ground,
        };
        *state = next;
        Ok(result)
    }
}

fn block_below(position: Vec3) -> Result<[i32; 3], WorldQueryError> {
    let values = [
        position.x.floor(),
        (position.y - 0.5).floor(),
        position.z.floor(),
    ];
    if values.into_iter().any(|value| {
        !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX)
    }) {
        return Err(WorldQueryError::CoordinateOutOfRange);
    }
    Ok(values.map(|value| value as i32))
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

#[derive(Debug, Clone, Copy)]
struct ResolvedMotion {
    resolved: Vec3,
    collisions: AxisCollisions,
}

fn resolve_motion(
    world: &impl CollisionWorld,
    position: Vec3,
    velocity: Vec3,
    was_on_ground: bool,
) -> Result<ResolvedMotion, WorldQueryError> {
    let start = Aabb::player_at(position);
    let colliders = world.collision_boxes(start.swept(velocity))?;
    let (normal_box, normal) = resolve_axes_reverse(start, velocity, &colliders);
    let normal_horizontal_collision = normal.x != velocity.x || normal.z != velocity.z;
    let normal_y_collision = normal.y != velocity.y;
    let on_ground = was_on_ground || (normal_y_collision && velocity.y < 0.0);

    let resolved_box = if on_ground && normal_horizontal_collision {
        let (step_box, step) = resolve_step(start, velocity, &colliders);
        let step_blocked = !world.collision_boxes(step_box)?.is_empty();
        if !step_blocked && step.horizontal_length_squared() > normal.horizontal_length_squared() {
            step_box
        } else {
            normal_box
        }
    } else {
        normal_box
    };

    let end_position = Vec3::new(
        (resolved_box.min.x + resolved_box.max.x) * 0.5,
        resolved_box.min.y,
        (resolved_box.min.z + resolved_box.max.z) * 0.5,
    );
    let resolved = end_position - position;
    Ok(ResolvedMotion {
        resolved,
        collisions: AxisCollisions {
            x: (velocity.x - resolved.x).abs() >= COLLISION_EPSILON,
            y: (velocity.y - resolved.y).abs() >= COLLISION_EPSILON,
            z: (velocity.z - resolved.z).abs() >= COLLISION_EPSILON,
        },
    })
}

fn resolve_axes_reverse(start: Aabb, velocity: Vec3, colliders: &[Aabb]) -> (Aabb, Vec3) {
    let mut current = start;
    let mut resolved = Vec3::ZERO;
    for axis in [1, 0, 2] {
        let mut axis_velocity = Vec3::ZERO;
        axis_velocity[axis] = velocity[axis];
        for collider in colliders.iter().rev().copied() {
            axis_velocity = current.clip_against(collider, axis_velocity);
        }
        current = current.translated(axis_velocity);
        resolved += axis_velocity;
    }
    (current, resolved)
}

fn resolve_step(start: Aabb, velocity: Vec3, colliders: &[Aabb]) -> (Aabb, Vec3) {
    let mut current = start;
    let mut up = Vec3::new(0.0, STEP_HEIGHT, 0.0);
    for collider in colliders.iter().copied() {
        up = current.clip_against(collider, up);
    }
    current = current.translated(up);

    let mut horizontal = Vec3::ZERO;
    for axis in [0, 2] {
        let mut axis_velocity = Vec3::ZERO;
        axis_velocity[axis] = velocity[axis];
        for collider in colliders.iter().copied() {
            axis_velocity = current.clip_against(collider, axis_velocity);
        }
        current = current.translated(axis_velocity);
        horizontal += axis_velocity;
    }

    let mut down = up * -1.0;
    for collider in colliders.iter().copied() {
        down = current.clip_against(collider, down);
    }
    current = current.translated(down);
    (current, horizontal + up + down)
}
