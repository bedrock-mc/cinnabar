mod collision;
mod controls;
mod effects;
mod environment;
mod flight;
mod immobile;
mod input;
mod mode;
#[cfg(test)]
mod numeric_tests;
mod scaffolding;
mod state;
mod travel;
mod water;

use crate::{
    Aabb, CollisionWorld, Vec3,
    math::{minecraft_cos, minecraft_sin},
};
use collision::{clip_sneak_edge, resolve_motion};
use environment::sample;

pub use controls::{ControlledTickResult, ProcessedControls};
pub use effects::MovementEffects;
pub use environment::MAX_BLOCK_SAMPLES_PER_TICK;
pub use input::MovementInput;
pub use mode::{MovementMode, pose_fits};
pub use state::{AxisCollisions, MovementEnvironment, PlayerState, SimulationError, TickResult};
pub use water::{sample_liquid_submersion, sample_water_head};

pub(crate) fn validate_player_state(state: &PlayerState) -> Result<(), SimulationError> {
    state::validate(state)
}

pub use world::TICKS_PER_SECOND;
const DEFAULT_JUMP_HEIGHT: f64 = 0.42;
const DEFAULT_AIR_FRICTION: f64 = 0.91;
const NORMAL_GRAVITY_MULTIPLIER: f64 = 0.98;
pub const NORMAL_GRAVITY: f64 = 0.08;
const STEP_HEIGHT: f64 = 0.5625;
const DEFAULT_MOVEMENT_SPEED: f64 = 0.1;
const DEFAULT_AIR_SPEED: f64 = 0.02;
const SPRINT_AIR_SPEED: f64 = 0.026;
/// Native total/current sprint attribute modifier, applied once on sprint entry.
pub const SPRINT_SPEED_MULTIPLIER: f64 = 1.3;
const SNEAK_INPUT_MULTIPLIER: f64 = 0.3;
const CONSUMABLE_INPUT_MULTIPLIER: f64 = 0.1225;
const INPUT_IMPULSE_MULTIPLIER: f64 = 0.98;
const SPRINT_JUMP_IMPULSE: f64 = 0.2;
/// Simulator post-jump cooldown length in ticks. A consumed request sets
/// this and each subsequent tick decrements it; prediction replays rebuild
/// initiations against the same gate, so it is part of the public contract.
pub const JUMP_DELAY_TICKS: u8 = 10;
// Vanilla move finalization uses the f32 epsilon.
const COLLISION_EPSILON: f64 = f32::EPSILON as f64;
/// `bedsim v0.1.3` `ClimbSpeed`, vanilla's ladder ascent speed.
const CLIMB_SPEED: f64 = 0.2;
// Provisional block-modifier and enchantment coefficients with no bedsim oracle;
// each needs independent measurement.
const HONEY_JUMP_FACTOR: f64 = 0.5;
const HONEY_SLIDE_TRIGGER: f64 = -0.13;
const HONEY_SLIDE_SPEED: f64 = -0.05;
const SOUL_SAND_ACCELERATION_FRICTION: f32 = 1.225;
const GROUND_BASE_FRICTION: f32 = 0.546_000_06;
const DEPTH_STRIDER_MAX_LEVEL: u8 = 3;
const WATER_DRAG: f64 = 0.8;
/// Ground drag depth strider blends water drag toward (default ground friction times air friction).
const DEPTH_STRIDER_TARGET_DRAG: f64 = GROUND_BASE_FRICTION as f64;
/// Provisional scaffolding sneak-descent speed; needs independent measurement.
const SCAFFOLDING_SNEAK_DESCENT: f64 = 0.15;
/// `bedsim v0.1.3` `walkOnBlock` damps slime by `0.4 + |yMov| * 0.2`. It only
/// runs on ticks whose resolved vertical movement is exactly zero, so `yMov` is
/// zero and the factor collapses to its constant term.
const SLIME_WALK_DAMPING: f64 = 0.4;
/// Restitution ignores descents below the ordinary gravity step.
const MIN_REBOUND_SPEED: f32 = 0.080_000_12;
// Known modelling limitation: bedsim distinguishes `state.Sneaking` (the
// latched sneak state, which start/stop edges can drive independently) from
// `state.PressingSneak` (the raw held button), and `walkOnBlock` and
// `landOnBlock` each consult both. `MovementInput` carries only one `sneaking`
// field, so both map to it. Every conformance fixture drives sneak purely from
// the held button, where the two are always equal, so no pinned observation
// distinguishes them. A future input model that latches sneak across start/stop
// edges must split this field before it can claim parity on those ticks.

#[derive(Debug, Clone, Copy, Default)]
pub struct Simulator {
    _private: (),
}

impl Simulator {
    /// Samples current body contact before selecting this tick's pose. Reusing
    /// the movement sampler keeps trigger queries and travel on the same liquid
    /// contact rules, without carrying the previous position's contact forward.
    pub fn movement_environment(
        &self,
        position: Vec3,
        mode: MovementMode,
        sneaking: bool,
        world: &(impl CollisionWorld + ?Sized),
    ) -> Result<crate::CollisionQuery<MovementEnvironment>, crate::WorldQueryError> {
        let sampled = sample(
            world,
            position,
            Vec3::ZERO,
            mode.hitbox_height(sneaking),
            None,
        )?;
        Ok(crate::CollisionQuery {
            value: sampled.movement,
            identity: sampled.identity,
        })
    }

    /// Advances exactly one 20 Hz Bedrock movement tick transactionally.
    pub fn tick(
        &self,
        state: &mut PlayerState,
        input: MovementInput,
        world: &impl CollisionWorld,
    ) -> Result<TickResult, SimulationError> {
        self.tick_with_controls(state, input, world)
            .map(|output| output.tick_result)
    }

    /// Advances the same transactional tick and publishes its primary controls.
    pub fn tick_with_controls(
        &self,
        state: &mut PlayerState,
        input: MovementInput,
        world: &impl CollisionWorld,
    ) -> Result<ControlledTickResult, SimulationError> {
        state::validate(state)?;
        input::validate(input)?;
        let controls = controls::process(input);
        if input.immobile {
            return immobile::tick(state, input.mode, controls, world.registry_identity());
        }
        let mut next = state.clone();
        next.position = next.position.rounded();
        next.velocity = next.velocity.rounded();
        next.swim_amount = water::advance_swim_amount(next.swim_amount, next.swim_pose_active);
        next.swim_pose_active =
            matches!(input.mode, MovementMode::Swimming | MovementMode::Crawling);
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
        let retained_collisions = next.collisions;
        let mut sampled = sample(
            world,
            next.position,
            next.velocity,
            input.mode.hitbox_height(input.sneaking),
            input.liquid_contact_height,
        )?;
        if input.mode != MovementMode::Riding
            && input
                .liquid_flow_enabled
                .unwrap_or(input.mode != MovementMode::Flying)
        {
            let contact_height = input
                .liquid_contact_height
                .unwrap_or_else(|| input.mode.hitbox_height(input.sneaking));
            let contact = crate::Aabb::player_with_height_at(next.position, contact_height);
            if let Some(current) = world.liquid_current(contact)? {
                sampled.identity = sampled.identity.merge(&current.identity)?;
                next.velocity = Vec3::new(
                    f64::from(next.velocity.x as f32 + current.value.x as f32),
                    f64::from(next.velocity.y as f32 + current.value.y as f32),
                    f64::from(next.velocity.z as f32 + current.value.z as f32),
                );
            }
        }
        let head_in_water = if input.jumping
            && input.mode == MovementMode::Swimming
            && let Some(attach_height) = input.liquid_attach_height
        {
            if sampled.block_samples == MAX_BLOCK_SAMPLES_PER_TICK {
                return Err(crate::WorldQueryError::QueryExtentExceeded.into());
            }
            let head = water::sample_water_head(world, next.position, attach_height)?;
            sampled.identity = sampled.identity.merge(&head.identity)?;
            sampled.block_samples += 1;
            Some(head.value)
        } else {
            None
        };
        let jump_suppressed = water::jump_suppressed(input.mode, next.swim_amount, head_in_water);
        if input.jumping && input.mode != MovementMode::Flying {
            if jump_suppressed {
                if sampled.movement.in_water {
                    next.velocity.y = 0.0;
                }
            } else if sampled.movement.in_water || sampled.movement.in_lava {
                water::jump(&mut next.velocity.y);
            }
        }
        // Vanilla selects water travel by the previous tick's in-water flag,
        // independent of the retained swimming pose on a dry low ceiling.
        if matches!(
            input.mode,
            MovementMode::Gliding | MovementMode::Flying | MovementMode::Riding
        ) || (input.mode == MovementMode::Swimming && sampled.movement.in_water)
        {
            return travel::tick_mode(
                next,
                state,
                input,
                controls,
                sampled,
                grounded_at_start,
                world,
            );
        }
        let friction = if grounded_at_start {
            f64::from(DEFAULT_AIR_FRICTION as f32 * sampled.friction as f32)
        } else {
            DEFAULT_AIR_FRICTION
        };
        let depth_strider = depth_strider_level(input.depth_strider, grounded_at_start);
        // Liquid and ground speeds come from attributes and friction only; block
        // speed factors never scale the acceleration.
        let relative_speed = if sampled.movement.in_water {
            water_travel_speed(&input, depth_strider)
        } else if sampled.movement.in_lava {
            DEFAULT_AIR_SPEED
        } else if grounded_at_start {
            ground_relative_speed(input, &sampled)
        } else if input.sprinting {
            SPRINT_AIR_SPEED
        } else {
            DEFAULT_AIR_SPEED
        };

        apply_relative_movement(
            &mut next.velocity,
            movement_impulse(controls.move_vector[0]),
            movement_impulse(controls.move_vector[1]),
            input.yaw_degrees,
            relative_speed,
        );

        // A held jump on a climbable feet cell climbs instead: no ground jump,
        // sprint impulse, jump delay or start-jump report.
        let climb_jump = input.jumping && sampled.movement.on_climbable;
        let jump_initiated = input.jump_pressed
            && !climb_jump
            && !jump_suppressed
            && next.on_ground
            && next.jump_delay == 0
            && !sampled.movement.in_water
            && !sampled.movement.in_lava;
        if jump_initiated {
            let honey = if sampled.movement.surface_response == crate::SurfaceResponse::Honey {
                HONEY_JUMP_FACTOR
            } else {
                1.0
            };
            let boost = input
                .effects
                .jump_boost
                .map_or(0.0, |amplifier| 0.1_f32 * (amplifier as f32 + 1.0));
            next.velocity.y = f64::from(
                (next.velocity.y as f32).max((DEFAULT_JUMP_HEIGHT as f32 + boost) * honey as f32),
            );
            next.jump_delay = JUMP_DELAY_TICKS;
            if input.sprinting {
                let yaw = f64::from((input.yaw_degrees as f32).to_radians());
                next.velocity.x = f64::from(
                    next.velocity.x as f32 - minecraft_sin(yaw) as f32 * SPRINT_JUMP_IMPULSE as f32,
                );
                next.velocity.z = f64::from(
                    next.velocity.z as f32 + minecraft_cos(yaw) as f32 * SPRINT_JUMP_IMPULSE as f32,
                );
            }
        }

        if sampled.movement.on_climbable || sampled.movement.in_scaffolding {
            next.velocity.y = next.velocity.y.max(-CLIMB_SPEED);
            // `bedsim v0.1.3` `simulateMovement` ascends a climbable block on a
            // held jump *or* on the previous tick's horizontal collision, which
            // is how walking into a ladder climbs it. Scaffolding has no bedsim
            // oracle, so it keeps the held-jump-only clause it already had.
            let wall_climb =
                sampled.movement.on_climbable && (retained_collisions.x || retained_collisions.z);
            if input.jumping || wall_climb {
                next.velocity.y = CLIMB_SPEED;
            } else if input.sneaking {
                // Sneaking descends scaffolding but holds position on a ladder.
                if sampled.movement.in_scaffolding {
                    next.velocity.y = -SCAFFOLDING_SNEAK_DESCENT;
                } else if next.velocity.y < 0.0 {
                    next.velocity.y = 0.0;
                }
            }
        }
        if sampled.movement.in_water || sampled.movement.in_lava {
            if sampled.movement.in_water && input.sneaking {
                water::sink(&mut next.velocity.y);
            }
            next.velocity.y =
                f64::from(next.velocity.y as f32 * (sampled.movement.vertical_speed_factor) as f32);
        }
        if sampled.movement.in_cobweb {
            let (horizontal, vertical) = if input.effects.weaving {
                (0.5, 0.25)
            } else {
                (0.25, 0.05)
            };
            next.velocity.x = f64::from(next.velocity.x as f32 * (horizontal) as f32);
            next.velocity.y = f64::from(next.velocity.y as f32 * (vertical) as f32);
            next.velocity.z = f64::from(next.velocity.z as f32 * (horizontal) as f32);
        } else if sampled.movement.in_powder_snow {
            next.velocity.x = f64::from(
                next.velocity.x as f32 * (sampled.movement.horizontal_speed_factor) as f32,
            );
            next.velocity.y =
                f64::from(next.velocity.y as f32 * (sampled.movement.vertical_speed_factor) as f32);
            next.velocity.z = f64::from(
                next.velocity.z as f32 * (sampled.movement.horizontal_speed_factor) as f32,
            );
        }
        if sampled.movement.surface_response == crate::SurfaceResponse::Honey
            && !grounded_at_start
            && (retained_collisions.x || retained_collisions.z)
            && next.velocity.y < HONEY_SLIDE_TRIGGER
        {
            next.velocity.y = HONEY_SLIDE_SPEED;
        }
        let mut identity = sampled.identity;
        if input.sneaking
            && input.mode != MovementMode::Crawling
            && grounded_at_start
            && next.velocity.y <= 0.0
        {
            let (clipped, edge_identity) = clip_sneak_edge(world, next.position, next.velocity)?;
            next.velocity = clipped;
            if let Some(edge_identity) = edge_identity {
                identity = identity.merge(&edge_identity)?;
            }
        }

        let pre_collision_velocity = next.velocity;
        next.requested_movement = pre_collision_velocity;
        let motion = resolve_motion(
            &scaffolding::ScaffoldingView::new(
                world,
                Aabb::player_with_height_at(
                    next.position,
                    input.mode.hitbox_height(input.sneaking),
                ),
                input.sneaking,
            ),
            next.position,
            next.velocity,
            grounded_at_start,
            input.mode.hitbox_height(input.sneaking),
        )?;
        identity = identity.merge(&motion.identity)?;
        next.position = motion.position;
        next.on_ground = motion.stepped
            || (motion.collisions.y && next.velocity.y < 0.0)
            || (grounded_at_start
                && !motion.collisions.y
                && next.velocity.y.abs() <= COLLISION_EPSILON);

        let landing_surface = if motion.collisions.y && pre_collision_velocity.y < 0.0 {
            let surface = if let Some(block) = motion.support {
                let support =
                    environment::sample_primary(world, block, &mut sampled.block_samples)?;
                identity = identity.merge(&support.identity)?;
                support.value.surface_response
            } else {
                crate::SurfaceResponse::None
            };
            if !matches!(
                sampled.movement.surface_response,
                crate::SurfaceResponse::BubbleUp | crate::SurfaceResponse::BubbleDown
            ) {
                sampled.movement.surface_response = surface;
            }
            surface
        } else {
            sampled.movement.surface_response
        };

        // `bedsim v0.1.3` applies `walkOnBlock` to the resolved velocity before
        // publishing this tick's movement, so the damping is visible in both.
        let mut resolved = motion.resolved;
        if resolved.y == 0.0
            && next.on_ground
            && !input.sneaking
            && landing_surface == crate::SurfaceResponse::Slime
        {
            resolved.x = f64::from(resolved.x as f32 * (SLIME_WALK_DAMPING) as f32);
            resolved.z = f64::from(resolved.z as f32 * (SLIME_WALK_DAMPING) as f32);
        }
        next.movement = resolved;
        next.velocity = resolved;
        if motion.stepped {
            next.velocity.y = 0.0;
        }
        if motion.collisions.x {
            next.velocity.x = 0.0;
        }
        if motion.collisions.y {
            let bounces = !input.sneaking && pre_collision_velocity.y as f32 <= -MIN_REBOUND_SPEED;
            next.velocity.y = match landing_surface {
                crate::SurfaceResponse::Slime if bounces => -pre_collision_velocity.y,
                crate::SurfaceResponse::Bed if bounces => {
                    // Vanilla bed restitution.
                    f64::from(-0.75_f32 * pre_collision_velocity.y as f32)
                }
                _ => 0.0,
            };
        }
        if motion.collisions.z {
            next.velocity.z = 0.0;
        }

        let liquid_ledge_exit = (sampled.movement.in_water || sampled.movement.in_lava)
            && (motion.collisions.x || motion.collisions.z);
        let auto_climb = if !sampled.movement.in_water
            && !sampled.movement.in_lava
            && (motion.collisions.x || motion.collisions.z)
        {
            let feet = environment::sample_primary(
                world,
                environment::block_at(next.position)?,
                &mut sampled.block_samples,
            )?;
            identity = identity.merge(&feet.identity)?;
            feet.value
                .flags
                .contains(crate::BlockPhysicsFlags::CLIMBABLE)
        } else {
            false
        };
        if auto_climb {
            next.velocity.y = CLIMB_SPEED;
        }
        if sampled.movement.in_cobweb {
            next.velocity = Vec3::ZERO;
            effects::apply_vertical(
                &mut next.velocity.y,
                input.effects,
                NORMAL_GRAVITY,
                NORMAL_GRAVITY_MULTIPLIER,
            );
        } else if sampled.movement.in_water || sampled.movement.in_lava {
            // When both liquid facts overlap, the pinned v0.1.5 slice follows
            // water travel rather than composing water gravity with lava drag.
            if sampled.movement.in_water {
                water::apply_drag(&mut next.velocity, &input, depth_strider);
            } else {
                next.velocity.x = f64::from(next.velocity.x as f32 * 0.5_f32);
                next.velocity.y = f64::from(next.velocity.y as f32 * 0.5_f32);
                next.velocity.z = f64::from(next.velocity.z as f32 * 0.5_f32);
            }
            // Pinned v0.1.5 open-water and ledge controls distinguish water's
            // non-swimming gravity from lava's ordinary liquid gravity.
            let gravity = if sampled.movement.in_water {
                0.005
            } else {
                0.02
            };
            effects::apply_vertical(&mut next.velocity.y, input.effects, gravity, 1.0);
        } else {
            let gravity = if input.effects.slow_falling && next.velocity.y < 0.0 {
                0.01
            } else {
                NORMAL_GRAVITY
            };
            effects::apply_vertical(
                &mut next.velocity.y,
                input.effects,
                if auto_climb { 0.0 } else { gravity },
                if auto_climb {
                    1.0
                } else {
                    NORMAL_GRAVITY_MULTIPLIER
                },
            );
            next.velocity.x = effects::damp_horizontal(next.velocity.x, friction as f32);
            next.velocity.z = effects::damp_horizontal(next.velocity.z, friction as f32);
        }
        if liquid_ledge_exit {
            if motion.collisions.x {
                next.movement.x = 0.0;
            }
            if motion.collisions.z {
                next.movement.z = 0.0;
            }
            let exit = water::climb_out(
                world,
                motion.aabb,
                state.position.y,
                next.position.y,
                &mut next.velocity,
                sampled.block_samples,
            )?;
            identity = identity.merge(&exit.identity)?;
        }
        match sampled.movement.surface_response {
            crate::SurfaceResponse::BubbleUp => next.velocity.y = next.velocity.y.max(0.1),
            crate::SurfaceResponse::BubbleDown => next.velocity.y = next.velocity.y.min(-0.1),
            _ => {}
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
            jump_initiated,
        })
    }
}

/// Vanilla water travel speed: the water base blended toward the ground
/// movement speed, multiplying the effective enchantment level before division.
fn water_travel_speed(input: &MovementInput, depth_strider: f64) -> f64 {
    let base = DEFAULT_AIR_SPEED as f32;
    let ground = effective_movement_speed(input);
    f64::from(base + ((ground - base) * depth_strider as f32) / f32::from(DEPTH_STRIDER_MAX_LEVEL))
}

/// Caps Depth Strider's level and halves it while airborne, before interpolation.
fn depth_strider_level(level: u8, grounded: bool) -> f64 {
    let level = f32::from(level.min(DEPTH_STRIDER_MAX_LEVEL));
    f64::from(if grounded { level } else { level * 0.5 })
}

/// Reconstructs the attribute current from the simulator's pre-sprint input.
/// Both land travel and Depth Strider read the same effective native speed.
fn effective_movement_speed(input: &MovementInput) -> f32 {
    let speed = input.movement_speed.unwrap_or(DEFAULT_MOVEMENT_SPEED) as f32;
    if input.sprinting {
        speed * SPRINT_SPEED_MULTIPLIER as f32
    } else {
        speed
    }
}

/// Applies the current client's f32 steering products before widening retained motion.
fn apply_relative_movement(
    velocity: &mut Vec3,
    strafe: f64,
    forward: f64,
    yaw_degrees: f64,
    relative_speed: f64,
) {
    let strafe = strafe as f32;
    let forward = forward as f32;
    let force_squared = forward * forward + strafe * strafe;
    if force_squared < 1.0e-4 {
        return;
    }
    let force = relative_speed as f32 / force_squared.sqrt().max(1.0);
    let yaw = (yaw_degrees as f32).to_radians();
    let sin = yaw.sin();
    let cos = yaw.cos();
    velocity.x = f64::from((strafe * force * cos - sin * forward * force) + velocity.x as f32);
    velocity.z = f64::from((strafe * force * sin + forward * force * cos) + velocity.z as f32);
}

/// Uses the current ground-speed ratio; Soul Speed removes only the terrain penalty here.
fn ground_relative_speed(input: MovementInput, sampled: &environment::SampledEnvironment) -> f64 {
    let soul_sand = sampled.movement.surface_response == crate::SurfaceResponse::SoulSand;
    let mut acceleration_friction = sampled.friction as f32;
    if soul_sand && input.soul_speed == 0 {
        acceleration_friction *= SOUL_SAND_ACCELERATION_FRICTION;
    }
    let drag = acceleration_friction * DEFAULT_AIR_FRICTION as f32;
    let ratio = if drag == 0.0 {
        1.0
    } else {
        GROUND_BASE_FRICTION / drag
    };
    let speed = effective_movement_speed(&input);
    f64::from(speed * ratio * ratio * ratio)
}

/// Rounds the control impulse before the relative-movement calculation.
fn movement_impulse(axis: f64) -> f64 {
    f64::from(axis as f32 * INPUT_IMPULSE_MULTIPLIER as f32)
}

#[cfg(test)]
mod environment_review_tests;
