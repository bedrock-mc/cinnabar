//! Non-walking locomotion: ability flight, pose-swimming and elytra gliding.
//!
//! Flight controls, liquid movement and glide steering follow the current vanilla client.

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

// Vanilla glide coefficients, applied in f32 in the order of `glide_velocity`.
const GLIDE_LIFT_SCALE: f32 = 0.75;
const GLIDE_LOOK_LENGTH_DIVISOR: f32 = 0.4;
const GLIDE_FALL_CONVERSION: f32 = -0.1;
const GLIDE_CLIMB_CONVERSION: f32 = -0.04;
const GLIDE_CLIMB_VERTICAL_BOOST: f32 = 3.2;
const GLIDE_ALIGNMENT: f32 = 0.1;
const GLIDE_HORIZONTAL_DRAG: f32 = 0.99;
const GLIDE_VERTICAL_DRAG: f32 = 0.98;
// Gliding gravity is signed downward; slow falling replaces it regardless of vertical direction.
const GLIDE_GRAVITY: f32 = -0.08;
const GLIDE_SLOW_FALLING_GRAVITY: f32 = -0.01;
// Firework boost steers toward `look * 1.5` by half the difference, plus `look * 0.1`.
const GLIDE_BOOST_TARGET: f32 = 1.5;
const GLIDE_BOOST_BLEND: f32 = 0.5;
const GLIDE_BOOST_PUSH: f32 = 0.1;

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
            next.velocity = glide_velocity(next.velocity, &input, next.previous_rotation);
        }
        _ => {}
    }

    let view = ScaffoldingView::new(
        world,
        crate::Aabb::player_with_height_at(next.position, input.mode.hitbox_height(input.sneaking)),
        input.sneaking,
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

/// Wraps a degree difference into `[-180, 180)` with float `fmod`, as vanilla does.
fn wrap_degrees(degrees: f32) -> f32 {
    let wrapped = (degrees + 180.0) % 360.0;
    let wrapped = if wrapped < 0.0 {
        wrapped + 360.0
    } else {
        wrapped
    };
    wrapped + -180.0
}

/// The previous rotation advanced by the wrapped difference to the current one.
fn interpolated_degrees(previous: f32, current: f32) -> f32 {
    wrap_degrees(current - previous) + previous
}

/// Elytra travel: lift from the current pitch, look steering from the interpolated rotation.
fn glide_velocity(
    velocity: Vec3,
    input: &MovementInput,
    previous_rotation: Option<[f32; 2]>,
) -> Vec3 {
    let pitch = input.pitch_degrees as f32;
    let yaw = input.yaw_degrees as f32;
    let [previous_pitch, previous_yaw] = previous_rotation.unwrap_or([pitch, yaw]);
    let look_yaw =
        interpolated_degrees(previous_yaw, yaw) * -1.0_f32.to_radians() + -std::f32::consts::PI;
    let look_pitch = interpolated_degrees(previous_pitch, pitch) * -1.0_f32.to_radians();
    let sin = |angle: f32| minecraft_sin(f64::from(angle)) as f32;
    let cos = |angle: f32| minecraft_cos(f64::from(angle)) as f32;
    let horizontal = -cos(look_pitch);
    let look = [
        horizontal * sin(look_yaw),
        sin(look_pitch),
        cos(look_yaw) * horizontal,
    ];
    let look_horizontal_squared = look[0] * look[0] + look[2] * look[2];
    let look_horizontal = look_horizontal_squared.sqrt();
    let look_length = (look[1] * look[1] + look[0] * look[0] + look[2] * look[2]).sqrt()
        / GLIDE_LOOK_LENGTH_DIVISOR;
    let pitch_radians = pitch.to_radians();
    let pitch_cos = cos(pitch_radians);
    let lift = look_length.min(1.0) * pitch_cos * pitch_cos;

    let [mut x, mut y, mut z] = [velocity.x as f32, velocity.y as f32, velocity.z as f32];
    let speed_horizontal = (x * x + z * z).sqrt();
    let gravity = if input.effects.slow_falling {
        GLIDE_SLOW_FALLING_GRAVITY
    } else {
        GLIDE_GRAVITY
    };
    y -= (GLIDE_LIFT_SCALE * lift + -1.0) * gravity;
    if look_horizontal_squared > 0.0 && y < 0.0 {
        let converted = lift * GLIDE_FALL_CONVERSION * y;
        x += (look[0] * converted) / look_horizontal;
        y += converted;
        z += (look[2] * converted) / look_horizontal;
    }
    // Vanilla leaves this unguarded; a zero horizontal look would only produce NaN here.
    if pitch_radians < 0.0 && look_horizontal_squared > 0.0 {
        let converted = sin(pitch_radians) * speed_horizontal * GLIDE_CLIMB_CONVERSION;
        x -= (converted * look[0]) / look_horizontal;
        y += GLIDE_CLIMB_VERTICAL_BOOST * converted;
        z -= (converted * look[2]) / look_horizontal;
    }
    if look_horizontal_squared > 0.0 {
        x += ((look[0] / look_horizontal) * speed_horizontal - x) * GLIDE_ALIGNMENT;
        z += ((look[2] / look_horizontal) * speed_horizontal - z) * GLIDE_ALIGNMENT;
    }
    if input.effects.glide_boost {
        let boost = |axis: f32, look: f32| {
            axis + (GLIDE_BOOST_TARGET * look - axis) * GLIDE_BOOST_BLEND + look * GLIDE_BOOST_PUSH
        };
        x = boost(x, look[0]);
        y = boost(y, look[1]);
        z = boost(z, look[2]);
    }
    Vec3::new(
        f64::from(x * GLIDE_HORIZONTAL_DRAG),
        f64::from(y * GLIDE_VERTICAL_DRAG),
        f64::from(z * GLIDE_HORIZONTAL_DRAG),
    )
}
