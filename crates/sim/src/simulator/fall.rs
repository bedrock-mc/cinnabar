//! The fall distance a movement tick keeps for powder snow's fall-into shape.

use crate::{CollisionWorld, WorldCollisionIdentity, WorldQueryError};

use super::{MovementEnvironment, MovementInput, PlayerState, environment::SampledEnvironment};

/// Checks new water contact at the resolved position before accumulating this move's drop.
/// Existing water contact already cleared the distance during pre-travel sensing.
pub(super) fn check(
    world: &impl CollisionWorld,
    state: &mut PlayerState,
    input: MovementInput,
    sampled: &mut SampledEnvironment,
    identity: &mut WorldCollisionIdentity,
    dy: f64,
) -> Result<(), WorldQueryError> {
    if !selected_water(&sampled.movement) {
        let body = state.collision_box(
            input
                .liquid_contact_height
                .unwrap_or(state.hitbox_height(input)),
        );
        let (water, _) = crate::fluid::sample_liquid_contacts(body, |block| {
            let (liquids, fresh) = sampled.liquids(world, block)?;
            if let Some(fresh) = fresh {
                *identity = identity.merge(&fresh)?;
            }
            Ok(liquids)
        })?;
        if water {
            state.clear_fall_distance();
            sampled.entered_water = true;
        }
    }
    after_move(&mut state.fall_distance, state.on_ground, dy);
    Ok(())
}

/// Water is selected only when the lava probe is clear.
pub(super) fn selected_water(environment: &MovementEnvironment) -> bool {
    environment.in_water && !environment.in_lava
}

/// Applies pre-travel liquid sensing: water clears retained distance and lava
/// halves it before the current move's downward displacement is added.
pub(super) fn sense_liquid(fall_distance: &mut f32, environment: &MovementEnvironment) {
    if environment.in_lava {
        *fall_distance *= 0.5;
    } else if selected_water(environment) {
        *fall_distance = 0.0;
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
