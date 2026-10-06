use crate::{CollisionRegistryIdentity, Vec3, WorldCollisionIdentity};

use super::{
    ControlledTickResult, MovementEnvironment, MovementMode, PlayerState, ProcessedControls,
    SimulationError, TickResult,
};

pub(super) fn tick(
    state: &mut PlayerState,
    mode: MovementMode,
    controls: ProcessedControls,
    registry: CollisionRegistryIdentity,
) -> Result<ControlledTickResult, SimulationError> {
    let tick = state
        .tick
        .checked_add(1)
        .ok_or(SimulationError::TickOverflow)?;
    state.tick = tick;
    state.velocity = Vec3::ZERO;
    state.movement = Vec3::ZERO;
    if mode != MovementMode::Riding {
        state.jump_delay = 0;
    }
    Ok(ControlledTickResult {
        tick_result: TickResult {
            tick,
            position: state.position,
            velocity: state.velocity,
            movement: state.movement,
            collisions: state.collisions,
            on_ground: state.on_ground,
            environment: MovementEnvironment::default(),
            world_identity: WorldCollisionIdentity {
                registry,
                chunks: Box::default(),
            },
        },
        controls,
        jump_initiated: false,
    })
}
