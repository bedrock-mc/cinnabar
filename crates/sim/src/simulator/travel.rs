//! Non-walking locomotion: ability flight, pose-swimming and elytra gliding.
//!
//! Flight controls and liquid movement follow the current vanilla client.
//! The glide equations remain provisional; see the locomotion gate in plan.md.

use crate::{
    CollisionWorld, Vec3,
    math::{minecraft_cos, minecraft_sin},
};

use super::{
    AxisCollisions, COLLISION_EPSILON, ControlledTickResult, MovementInput, MovementMode,
    NORMAL_GRAVITY, PlayerState, SimulationError, TickResult, apply_relative_movement,
    collision::resolve_motion, controls, effects, environment::SampledEnvironment,
    scaffolding::ScaffoldingView,
};

const GLIDE_LIFT_SCALE: f64 = 0.75;
const GLIDE_FALL_CONVERSION: f64 = 0.1;
const GLIDE_CLIMB_CONVERSION: f64 = 0.04;
const GLIDE_CLIMB_VERTICAL_BOOST: f64 = 3.2;
const GLIDE_ALIGNMENT: f64 = 0.1;
const GLIDE_DRAG: [f64; 3] = [0.99, 0.98, 0.99];
const SLOW_FALLING_GRAVITY: f64 = 0.01;

pub(super) fn tick_mode(
    mut next: PlayerState,
    state: &mut PlayerState,
    input: MovementInput,
    controls: controls::ProcessedControls,
    mut sampled: SampledEnvironment,
    grounded_at_start: bool,
    world: &impl CollisionWorld,
) -> Result<ControlledTickResult, SimulationError> {
    if input.mode == MovementMode::Riding {
        next.velocity = Vec3::ZERO;
        next.movement = Vec3::ZERO;
        next.requested_movement = Vec3::ZERO;
        next.jump_delay = 0;
        next.collisions = AxisCollisions::default();
        let result = TickResult {
            tick: next.tick,
            position: next.position,
            velocity: Vec3::ZERO,
            movement: Vec3::ZERO,
            collisions: AxisCollisions::default(),
            on_ground: next.on_ground,
            environment: sampled.movement,
            world_identity: sampled.identity,
        };
        *state = next;
        return Ok(ControlledTickResult {
            tick_result: result,
            controls,
            jump_initiated: false,
        });
    }
    let mut controls = controls;
    let in_water = sampled.movement.in_water;
    let mut identity = sampled.identity.clone();
    let flight_ground_friction = if input.mode == MovementMode::Flying && grounded_at_start {
        sampled.friction
    } else {
        1.0
    };
    match input.mode {
        MovementMode::Flying => {
            // Sneak descends while flying instead of slowing the walk.
            controls = controls::process(MovementInput {
                sneaking: false,
                ..input
            });
            apply_relative_movement(
                &mut next.velocity,
                super::movement_impulse(controls.move_vector[0]),
                super::movement_impulse(controls.move_vector[1]),
                input.yaw_degrees,
                super::flight::horizontal_speed(&input),
            );
            super::flight::apply_vertical_control(&mut next.velocity, &input, controls.move_vector);
        }
        MovementMode::Swimming if in_water => {
            apply_relative_movement(
                &mut next.velocity,
                super::movement_impulse(controls.move_vector[0]),
                super::movement_impulse(controls.move_vector[1]),
                input.yaw_degrees,
                super::water_travel_speed(
                    &input,
                    sampled.movement.horizontal_speed_factor,
                    super::depth_strider_level(input.depth_strider, grounded_at_start),
                ),
            );
            let attach = (!input.jumping)
                .then_some(input.liquid_attach_height)
                .flatten()
                .map(|height| {
                    super::water::sample_attach(world, next.position, height, sampled.block_samples)
                })
                .transpose()?;
            if let Some(attach) = &attach {
                identity = identity.merge(&attach.identity)?;
                sampled.block_samples += 1;
            }
            super::water::steer(
                &mut next.velocity.y,
                &input,
                attach.map(|sample| sample.value),
            );
            if input.sneaking {
                super::water::sink(&mut next.velocity.y);
            }
        }
        MovementMode::Gliding => {
            next.velocity = glide_velocity(next.velocity, input);
        }
        _ => {}
    }

    let view = ScaffoldingView::new(
        world,
        crate::Aabb::player_with_height_at(next.position, input.mode.hitbox_height(input.sneaking)),
        sampled.descend_through,
    );
    let height = input.mode.hitbox_height(input.sneaking);
    next.requested_movement = next.velocity;
    let motion = resolve_motion(
        &view,
        next.position,
        next.velocity,
        grounded_at_start,
        height,
    )?;
    let mut identity = identity.merge(&motion.identity)?;
    let pre_collision_velocity = next.velocity;
    next.position = motion.position;
    next.on_ground = motion.stepped
        || (motion.collisions.y && pre_collision_velocity.y < 0.0)
        || (grounded_at_start
            && !motion.collisions.y
            && pre_collision_velocity.y.abs() <= COLLISION_EPSILON);
    next.movement = motion.resolved;
    next.velocity = motion.resolved;
    if motion.stepped || motion.collisions.y {
        next.velocity.y = 0.0;
    }
    if motion.collisions.x {
        next.velocity.x = 0.0;
    }
    if motion.collisions.z {
        next.velocity.z = 0.0;
    }

    match input.mode {
        MovementMode::Flying => {
            super::flight::apply_drag(
                &mut next.velocity,
                &input,
                controls.move_vector,
                flight_ground_friction,
            );
        }
        MovementMode::Swimming if in_water => {
            super::water::apply_drag(
                &mut next.velocity,
                &input,
                super::depth_strider_level(input.depth_strider, grounded_at_start),
            );
            effects::apply_vertical(&mut next.velocity.y, input.effects, 0.0, 1.0);
        }
        MovementMode::Gliding => {}
        _ => {
            effects::apply_vertical(
                &mut next.velocity.y,
                input.effects,
                NORMAL_GRAVITY,
                super::NORMAL_GRAVITY_MULTIPLIER,
            );
            next.velocity.x *= super::DEFAULT_AIR_FRICTION;
            next.velocity.z *= super::DEFAULT_AIR_FRICTION;
        }
    }
    if input.mode == MovementMode::Swimming
        && in_water
        && (motion.collisions.x || motion.collisions.z)
    {
        if motion.collisions.x {
            next.movement.x = 0.0;
        }
        if motion.collisions.z {
            next.movement.z = 0.0;
        }
        let exit = super::water::climb_out(
            world,
            motion.aabb,
            state.position.y,
            next.position.y,
            &mut next.velocity,
            sampled.block_samples,
        )?;
        identity = identity.merge(&exit.identity)?;
    }
    next.jump_delay = next.jump_delay.saturating_sub(1);
    next.collisions = motion.collisions;

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
    Ok(ControlledTickResult {
        tick_result: result,
        controls,
        jump_initiated: false,
    })
}

/// Unit look direction; pitch is positive downward.
fn look_vector(yaw_degrees: f64, pitch_degrees: f64) -> Vec3 {
    let yaw = yaw_degrees.to_radians();
    let pitch = pitch_degrees.to_radians();
    let horizontal = minecraft_cos(pitch);
    Vec3::new(
        -minecraft_sin(yaw) * horizontal,
        -minecraft_sin(pitch),
        minecraft_cos(yaw) * horizontal,
    )
}

fn glide_velocity(velocity: Vec3, input: MovementInput) -> Vec3 {
    let pitch = input.pitch_degrees.to_radians();
    let look = look_vector(input.yaw_degrees, input.pitch_degrees);
    let look_horizontal = look.x.hypot(look.z);
    let speed_horizontal = velocity.x.hypot(velocity.z);
    let lift = minecraft_cos(pitch) * minecraft_cos(pitch);
    let gravity = if input.effects.slow_falling && velocity.y < 0.0 {
        SLOW_FALLING_GRAVITY
    } else {
        NORMAL_GRAVITY
    };
    let mut next = velocity;
    next.y += gravity * (-1.0 + lift * GLIDE_LIFT_SCALE);
    if look_horizontal > 0.0 {
        if next.y < 0.0 {
            let converted = next.y * -GLIDE_FALL_CONVERSION * lift;
            next.y += converted;
            next.x += look.x * converted / look_horizontal;
            next.z += look.z * converted / look_horizontal;
        }
        if pitch < 0.0 {
            let converted = speed_horizontal * -minecraft_sin(pitch) * GLIDE_CLIMB_CONVERSION;
            next.y += converted * GLIDE_CLIMB_VERTICAL_BOOST;
            next.x -= look.x * converted / look_horizontal;
            next.z -= look.z * converted / look_horizontal;
        }
        next.x += (look.x / look_horizontal * speed_horizontal - next.x) * GLIDE_ALIGNMENT;
        next.z += (look.z / look_horizontal * speed_horizontal - next.z) * GLIDE_ALIGNMENT;
    }
    Vec3::new(
        next.x * GLIDE_DRAG[0],
        next.y * GLIDE_DRAG[1],
        next.z * GLIDE_DRAG[2],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glide_input(pitch_degrees: f64) -> MovementInput {
        MovementInput {
            mode: MovementMode::Gliding,
            pitch_degrees,
            ..MovementInput::default()
        }
    }

    #[test]
    fn look_vector_is_unit_and_points_down_for_positive_pitch() {
        let look = look_vector(0.0, 45.0);
        assert!((look.length_squared() - 1.0).abs() < 1.0e-3);
        assert!(look.y < 0.0 && look.z > 0.0);
    }

    #[test]
    fn steep_dive_gains_horizontal_speed_and_shallow_climb_trades_it_for_height() {
        let dive = glide_velocity(Vec3::new(0.0, -0.5, 0.5), glide_input(60.0));
        assert!(dive.z > 0.5 * GLIDE_DRAG[2] - 1.0e-9);
        let climb = glide_velocity(Vec3::new(0.0, 0.0, 1.0), glide_input(-30.0));
        assert!(climb.y > 0.0);
        assert!(climb.z < 1.0);
    }
}
