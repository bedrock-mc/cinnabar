//! The fall distance a movement tick keeps for powder snow's fall-into shape.

use super::MovementEnvironment;

/// Applies the liquid sensing that vanilla's fall check runs after the
/// previous move, from this tick's sample of that same position: water clears
/// the distance and lava halves it. Incomplete: vanilla halves in lava before
/// adding the previous move's drop, while this halves the sum.
pub(super) fn sense_liquid(fall_distance: &mut f32, environment: &MovementEnvironment) {
    if environment.in_water {
        *fall_distance = 0.0;
    } else if environment.in_lava {
        *fall_distance *= 0.5;
    }
}

/// The fall check after the move: a downward move in the air adds its drop,
/// while landing or any other move clears the distance.
pub(super) fn after_move(fall_distance: &mut f32, on_ground: bool, dy: f64) {
    *fall_distance = if !on_ground && dy < 0.0 {
        *fall_distance - dy as f32
    } else {
        0.0
    };
}
