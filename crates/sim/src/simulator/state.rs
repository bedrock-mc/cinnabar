use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{SurfaceResponse, Vec3, WorldCollisionIdentity, WorldQueryError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerState {
    pub tick: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub movement: Vec3,
    pub on_ground: bool,
    pub jump_delay: u8,
    /// Axis collisions resolved by the previous tick. Bedrock reads these one
    /// tick late — `bedsim v0.1.3` `simulateMovement` consults `state.CollideX`
    /// and `state.CollideZ` before the current tick resolves motion — so they
    /// are retained state, not a derived output. Traces recorded before this
    /// field existed default to "no retained collision".
    #[serde(default)]
    pub collisions: AxisCollisions,
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
            collisions: AxisCollisions {
                x: false,
                y: false,
                z: false,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisCollisions {
    pub x: bool,
    pub y: bool,
    pub z: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementEnvironment {
    pub on_climbable: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub in_cobweb: bool,
    pub in_powder_snow: bool,
    pub in_scaffolding: bool,
    pub horizontal_speed_factor: f64,
    pub vertical_speed_factor: f64,
    pub surface_response: SurfaceResponse,
}

impl Default for MovementEnvironment {
    fn default() -> Self {
        Self {
            on_climbable: false,
            in_water: false,
            in_lava: false,
            in_cobweb: false,
            in_powder_snow: false,
            in_scaffolding: false,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            surface_response: SurfaceResponse::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickResult {
    pub tick: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub movement: Vec3,
    pub collisions: AxisCollisions,
    pub on_ground: bool,
    pub environment: MovementEnvironment,
    pub world_identity: WorldCollisionIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SimulationError {
    #[error("player state field {field} is not finite")]
    NonFiniteState { field: &'static str },
    #[error("movement input field {field} is not finite")]
    NonFiniteInput { field: &'static str },
    #[error(transparent)]
    World(#[from] WorldQueryError),
    #[error("movement tick overflow")]
    TickOverflow,
}

pub(super) fn validate(state: &PlayerState) -> Result<(), SimulationError> {
    for (field, value) in [
        ("position", state.position),
        ("velocity", state.velocity),
        ("movement", state.movement),
    ] {
        if !value.is_finite() {
            return Err(SimulationError::NonFiniteState { field });
        }
    }
    let min_position = f64::from(i32::MIN) + 2.0;
    let max_position = f64::from(i32::MAX) - 2.0;
    if [state.position.x, state.position.y, state.position.z]
        .into_iter()
        .any(|value| value < min_position || value > max_position)
    {
        return Err(WorldQueryError::CoordinateOutOfRange.into());
    }
    let max_sweep_component = crate::world::MAX_COLLISION_QUERY_EXTENT - crate::PLAYER_HEIGHT;
    if [state.velocity.x, state.velocity.y, state.velocity.z]
        .into_iter()
        .any(|value| value.abs() > max_sweep_component)
    {
        return Err(WorldQueryError::QueryExtentExceeded.into());
    }
    Ok(())
}
