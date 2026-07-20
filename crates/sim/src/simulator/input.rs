use serde::{Deserialize, Serialize};

use super::SimulationError;

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
    Ok(())
}
