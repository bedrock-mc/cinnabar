//! Forced ability flight without terrain collision or inside-block effects.

use crate::{CollisionRegistryIdentity, WorldCollisionIdentity};

use super::{
    AxisCollisions, ControlledTickResult, MovementEnvironment, MovementInput, PlayerState,
    SimulationError, TickResult, apply_relative_movement, controls, flight, movement_impulse,
};

/// Advances a spectator through terrain with the ordinary ability-flight steering and drag.
pub(super) fn tick(
    mut next: PlayerState,
    state: &mut PlayerState,
    input: MovementInput,
    registry: CollisionRegistryIdentity,
) -> Result<ControlledTickResult, SimulationError> {
    let controls = controls::process(MovementInput {
        sneaking: false,
        ..input
    });
    apply_relative_movement(
        &mut next.velocity,
        movement_impulse(controls.move_vector[0]),
        movement_impulse(controls.move_vector[1]),
        input.yaw_degrees,
        flight::horizontal_speed(&input),
    );
    flight::apply_vertical_control(&mut next.velocity, &input, controls.move_vector);
    next.requested_movement = next.velocity;
    next.movement = next.velocity;
    next.position = (next.position + next.velocity).rounded();
    flight::apply_drag(&mut next.velocity, &input, controls.move_vector, 1.0);
    next.on_ground = false;
    next.collisions = AxisCollisions::default();
    next.jump_delay = 0;
    next.swim_amount = 0.0;
    next.swim_pose_active = false;
    if !next.position.is_finite() || !next.velocity.is_finite() {
        return Err(SimulationError::NonFiniteState {
            field: "spectator_motion",
        });
    }
    let result = ControlledTickResult {
        tick_result: TickResult {
            tick: next.tick,
            position: next.position,
            velocity: next.velocity,
            movement: next.movement,
            collisions: next.collisions,
            on_ground: false,
            environment: MovementEnvironment::default(),
            world_identity: WorldCollisionIdentity {
                registry,
                chunks: Box::default(),
            },
        },
        controls,
        jump_initiated: false,
    };
    *state = next;
    Ok(result)
}
