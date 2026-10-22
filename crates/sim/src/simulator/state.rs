use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Aabb, SurfaceResponse, Vec3, WorldCollisionIdentity, WorldQueryError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerState {
    /// Completed ticks in the current spin attack; absence means the pose is inactive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spin_attack_ticks: Option<u8>,
    pub tick: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub movement: Vec3,
    /// Requested displacement before collision resolution, retained for controls.
    #[serde(default)]
    pub requested_movement: Vec3,
    pub on_ground: bool,
    pub jump_delay: u8,
    /// Vanilla swim-amount blend retained across ticks and replay. Both
    /// the swimming and crawling flags advance it before the jump system.
    #[serde(default)]
    pub swim_amount: f32,
    /// Retained swimming-or-crawling flag observed by the swim-amount blend before
    /// this tick's local swim/pose trigger applies its new choice.
    #[serde(default)]
    pub swim_pose_active: bool,
    /// Axis collisions resolved by the previous tick. Bedrock reads these one
    /// tick late — `bedsim v0.1.3` `simulateMovement` consults `state.CollideX`
    /// and `state.CollideZ` before the current tick resolves motion — so they
    /// are retained state, not a derived output. Traces recorded before this
    /// field existed default to "no retained collision".
    #[serde(default)]
    pub collisions: AxisCollisions,
    /// Previous tick's `[pitch, yaw]` in degrees. Glide steering reads the
    /// rotation interpolated from it; absence reads as the current rotation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_rotation: Option<[f32; 2]>,
    /// Resolved faces survive center rounding and deterministic replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) collision_shape: Option<CollisionShape>,
    /// Fall distance after the previous tick, which this tick's powder snow
    /// collision reads. Corrections restore it with the retained tick.
    #[serde(default, skip_serializing_if = "is_zero_distance")]
    pub(super) fall_distance: f32,
    /// Block-climber contact measured after the previous tick's move, which
    /// this tick's descent action, climb jump and gravity read. Like vanilla's
    /// actor flags, it survives placements and corrections until the climber
    /// measures again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) climber: Option<super::scaffolding::ClimberContact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CollisionShape {
    bounds: Aabb,
    position: Vec3,
    height: f64,
}

impl PlayerState {
    /// Uses the compact collision pose throughout a spin attack.
    pub(super) fn hitbox_height(&self, input: super::MovementInput) -> f64 {
        let launching = input
            .riptide
            .is_some_and(super::RiptideLaunch::is_supported)
            && input.mode != super::MovementMode::Riding
            && !input.spectator;
        if self.spin_attack_ticks.is_some() || launching {
            crate::PLAYER_WIDTH
        } else {
            input.mode.hitbox_height(input.sneaking)
        }
    }
    #[must_use]
    pub const fn new(position: Vec3) -> Self {
        Self {
            spin_attack_ticks: None,
            tick: 0,
            position,
            velocity: Vec3::ZERO,
            movement: Vec3::ZERO,
            requested_movement: Vec3::ZERO,
            on_ground: false,
            jump_delay: 0,
            swim_amount: 0.0,
            swim_pose_active: false,
            collisions: AxisCollisions {
                x: false,
                y: false,
                z: false,
            },
            previous_rotation: None,
            collision_shape: None,
            fall_distance: 0.0,
            climber: None,
        }
    }

    /// Fall distance after the latest tick, which powder snow's collision reads.
    #[must_use]
    pub const fn fall_distance(&self) -> f32 {
        self.fall_distance
    }

    /// Keeps `previous`'s block-climber contact through a spatial reset, as
    /// vanilla's corrections and teleports leave the climber's flags untouched.
    pub fn keep_climber_contact(&mut self, previous: &PlayerState) {
        self.climber = previous.climber;
    }

    /// Applies an external placement, rebuilding contact faces even at the same
    /// position. Retained climber contact is kept.
    pub fn set_position(&mut self, position: Vec3) {
        self.position = position;
        self.collision_shape = None;
    }

    /// Returns retained contact faces, rebuilding after a position correction.
    pub(super) fn collision_box(&self, height: f64) -> Aabb {
        let Some(shape) = self
            .collision_shape
            .filter(|shape| shape.position == self.position.rounded())
        else {
            return Aabb::player_with_height_at(self.position, height);
        };
        let mut bounds = shape.bounds;
        if shape.height != height {
            bounds.max.y = f64::from(bounds.min.y as f32 + height as f32);
        }
        bounds
    }

    /// Records the resolved box independently of its rounded feet center.
    pub(super) fn retain_collision_box(&mut self, bounds: Aabb, height: f64) {
        self.collision_shape = Some(CollisionShape {
            bounds: Aabb::new(bounds.min.rounded(), bounds.max.rounded()),
            position: self.position.rounded(),
            height,
        });
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
    /// Scaffolding at the feet layer (`InScaffolding`).
    pub in_scaffolding: bool,
    /// Scaffolding at the layer below the feet (`OverScaffolding`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub over_scaffolding: bool,
    /// A climbable block at the feet layer (`InAscendingBlock`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub in_ascending_block: bool,
    /// A climbable block at the layer below the feet (`OverDescendingBlock`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub over_descending_block: bool,
    /// This tick's descent through a climbable block (`DescendThroughBlock`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub descending_through_block: bool,
    /// No block-climber contact is known: a spectator tick, or unreadable
    /// terrain with no contact retained from earlier ticks.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub climber_unsampled: bool,
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
            over_scaffolding: false,
            in_ascending_block: false,
            over_descending_block: false,
            descending_through_block: false,
            climber_unsampled: false,
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
    #[error("movement speed authority must be finite and nonnegative")]
    InvalidMovementSpeed,
    #[error("swim amount must be finite and within [0, 1]")]
    InvalidSwimAmount,
    #[error("liquid contact height must be finite and positive")]
    InvalidLiquidContactHeight,
    #[error("item-use movement modifier must be finite and within [0, 1]")]
    InvalidItemUseMovementModifier,
    #[error(transparent)]
    World(#[from] WorldQueryError),
    #[error("movement tick overflow")]
    TickOverflow,
}

/// Omits a zero fall distance from serialized states.
fn is_zero_distance(value: &f32) -> bool {
    *value == 0.0
}

pub(super) fn validate(state: &PlayerState) -> Result<(), SimulationError> {
    if let Some(shape) = state.collision_shape {
        if !shape.bounds.min.is_finite()
            || !shape.bounds.max.is_finite()
            || !shape.position.is_finite()
            || !shape.height.is_finite()
        {
            return Err(SimulationError::NonFiniteState {
                field: "collision_shape",
            });
        }
        crate::world::validate_collision_query(shape.bounds)?;
    }
    if !state.fall_distance.is_finite() {
        return Err(SimulationError::NonFiniteState {
            field: "fall_distance",
        });
    }
    if !state.swim_amount.is_finite() || !(0.0..=1.0).contains(&state.swim_amount) {
        return Err(SimulationError::InvalidSwimAmount);
    }
    for (field, value) in [
        ("position", state.position),
        ("velocity", state.velocity),
        ("movement", state.movement),
        ("requested_movement", state.requested_movement),
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
