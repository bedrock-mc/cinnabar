use serde::{Deserialize, Serialize};

use super::{MovementEffects, MovementMode, SimulationError};

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
    /// Axes precede item and pose slowdown. False preserves historical
    /// already-processed input clamping; true retains partial-axis magnitude.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub move_vector_is_raw: bool,
    /// Whether the selected item is actively in its consumable-use phase.
    #[serde(default)]
    pub using_consumable: bool,
    /// Effective item-use factor applied once before pose slowdown. None
    /// preserves consumable flags; explicit zero and one are meaningful.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_use_movement_modifier: Option<f64>,
    /// Effective non-sprinting `minecraft:movement` value captured for this
    /// fixed tick, retaining custom/effect speed. Sprint is applied separately
    /// once. Absence selects the vanilla default; zero is valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement_speed: Option<f64>,
    #[serde(default, skip_serializing_if = "MovementEffects::is_empty")]
    pub effects: MovementEffects,
    /// Client-selected locomotion mode; `Walking` runs the oracle-validated path.
    #[serde(default, skip_serializing_if = "is_walking")]
    pub mode: MovementMode,
    /// Look pitch, degrees positive downward. Read only by swimming and gliding.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pitch_degrees: f64,
    /// Tick-captured height of native attach location 7 above the feet. The
    /// caller retains its pose-offset transition in prediction input; absence
    /// leaves the swimming surface guard unspecified for legacy traces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquid_attach_height: Option<f64>,
    /// Height of the preceding collision pose at this tick's current feet.
    /// Native liquid sensing runs before pose selection; replay retains this
    /// height while resampling water/lava at the corrected position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquid_contact_height: Option<f64>,
    /// Flow policy is sensed before this tick's flight trigger. Retaining its
    /// preceding flying state keeps toggle ticks and correction replay aligned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquid_flow_enabled: Option<bool>,
    /// Depth Strider level on the boots; scales water travel toward ground travel.
    #[serde(default, skip_serializing_if = "is_zero_level")]
    pub depth_strider: u8,
    /// Soul Speed level on the boots; replaces the soul sand slowdown.
    #[serde(default, skip_serializing_if = "is_zero_level")]
    pub soul_speed: u8,
    /// Ability vertical flight speed (per-tick acceleration scale); `None` selects 1.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_fly_speed: Option<f64>,
    /// Creative flight hovers with stronger damping than other flying modes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub creative_flight: bool,
    /// Ability flight speed; `None` selects the vanilla default. Read only when flying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fly_speed: Option<f64>,
}

fn is_walking(mode: &MovementMode) -> bool {
    mode.is_walking()
}

fn is_zero_level(value: &u8) -> bool {
    *value == 0
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

pub(super) fn validate(input: MovementInput) -> Result<(), SimulationError> {
    for (field, value) in [
        ("strafe", input.strafe),
        ("forward", input.forward),
        ("yaw_degrees", input.yaw_degrees),
        ("pitch_degrees", input.pitch_degrees),
    ] {
        if !value.is_finite() {
            return Err(SimulationError::NonFiniteInput { field });
        }
    }
    if input
        .liquid_attach_height
        .is_some_and(|height| !height.is_finite())
    {
        return Err(SimulationError::NonFiniteInput {
            field: "liquid_attach_height",
        });
    }
    if input
        .liquid_contact_height
        .is_some_and(|height| !height.is_finite() || height <= 0.0)
    {
        return Err(SimulationError::InvalidLiquidContactHeight);
    }
    if input
        .item_use_movement_modifier
        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return Err(SimulationError::InvalidItemUseMovementModifier);
    }
    if input
        .movement_speed
        .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err(SimulationError::InvalidMovementSpeed);
    }
    if [input.fly_speed, input.vertical_fly_speed]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite())
        || input.fly_speed.is_some_and(|value| value < 0.0)
    {
        return Err(SimulationError::InvalidMovementSpeed);
    }
    Ok(())
}
