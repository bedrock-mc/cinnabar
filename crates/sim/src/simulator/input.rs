use serde::{Deserialize, Serialize};

use super::{MovementEffects, SimulationError};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementInput {
    pub strafe: f64,
    pub forward: f64,
    pub yaw_degrees: f64,
    pub jumping: bool,
    pub jump_pressed: bool,
    pub sprinting: bool,
    pub sneaking: bool,
    /// Whether the selected item is actively in its consumable-use phase.
    #[serde(default)]
    pub using_consumable: bool,
    /// Server-authoritative `minecraft:movement` current value captured for
    /// this fixed tick. Absence selects the vanilla default; zero is valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement_speed: Option<f64>,
    #[serde(default, skip_serializing_if = "MovementEffects::is_empty")]
    pub effects: MovementEffects,
}

pub(super) fn validate(input: MovementInput) -> Result<(), SimulationError> {
    for (field, value) in [
        ("strafe", input.strafe),
        ("forward", input.forward),
        ("yaw_degrees", input.yaw_degrees),
    ] {
        if !value.is_finite() {
            return Err(SimulationError::NonFiniteInput { field });
        }
    }
    if input
        .movement_speed
        .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err(SimulationError::InvalidMovementSpeed);
    }
    Ok(())
}
