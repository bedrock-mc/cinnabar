//! Local ability, hunger and equipment facts that gate movement modes.

use crate::game_mode_capabilities::{ability_bit, resolved_layer};
use protocol::AbilitiesUpdate;

use super::control_modes::SPRINT_HUNGER_FLOOR;
use crate::player_runtime::PlayerRuntime;

const ELYTRA_IDENTIFIER: &str = "minecraft:elytra";
/// Bedrock enchantment ids; provisional until checked against a native item.
const DEPTH_STRIDER_ENCHANTMENT_ID: i16 = 7;
const SOUL_SPEED_ENCHANTMENT_ID: i16 = 36;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct LocalMovementFacts {
    pub ride: Option<super::RideKind>,
    pub ride_seat: Option<[f32; 3]>,
    pub can_fly: bool,
    pub server_flying: bool,
    pub fly_speed: Option<f64>,
    pub vertical_fly_speed: Option<f64>,
    pub creative_flight: bool,
    pub elytra_ready: bool,
    pub depth_strider: u8,
    pub soul_speed: u8,
    pub sprint_blocked: bool,
}

pub(super) fn read(
    player: Option<&PlayerRuntime>,
    stream: &client_world::WorldStream,
    item_in_use: bool,
) -> LocalMovementFacts {
    let Some(player) = player else {
        return LocalMovementFacts::default();
    };
    let [_, chestplate, _, boots] = player.inventory.local_armor();
    let elytra_ready = !chestplate.is_empty()
        && stream
            .canonical_item_stack(&chestplate)
            .and_then(|stack| stack.identifier)
            .is_some_and(|identifier| &*identifier == ELYTRA_IDENTIFIER);
    let capabilities = player.facts.game_mode_capabilities();
    let boots_level = |id| protocol::item_enchantment_level(&boots.extra_data, id).unwrap_or(0);
    let ride = player.facts.mount_unique_id().map(|unique| {
        stream
            .actor_by_unique_id(unique)
            .and_then(|actor| match &actor.kind {
                protocol::ActorKind::Entity { identifier } => {
                    Some(super::RideKind::from_identifier(identifier))
                }
                protocol::ActorKind::Player { .. } => None,
            })
            .unwrap_or(super::RideKind::Other)
    });
    LocalMovementFacts {
        ride,
        ride_seat: ride
            .and(stream.local_rider_seat_pose())
            .map(|(position, _)| position),
        can_fly: capabilities.is_some_and(|capabilities| capabilities.can_fly),
        server_flying: capabilities.is_some_and(|capabilities| capabilities.flying),
        fly_speed: player.facts.local_abilities().and_then(|update| {
            flight_speed(update, ability_bit::FLY_SPEED, |layer| layer.fly_speed_bits)
        }),
        vertical_fly_speed: player.facts.local_abilities().and_then(|update| {
            flight_speed(update, ability_bit::VERTICAL_FLY_SPEED, |layer| {
                layer.vertical_fly_speed_bits
            })
        }),
        creative_flight: capabilities.is_some_and(|capabilities| capabilities.creative_inventory),
        elytra_ready,
        depth_strider: boots_level(DEPTH_STRIDER_ENCHANTMENT_ID),
        soul_speed: boots_level(SOUL_SPEED_ENCHANTMENT_ID),
        sprint_blocked: (player.facts.survival_stats_visible()
            && player
                .facts
                .hunger()
                .is_some_and(|hunger| hunger.current() <= SPRINT_HUNGER_FLOOR))
            || item_in_use,
    }
}

/// Reads a defined finite flight speed, including an authoritative zero.
fn flight_speed(
    update: &AbilitiesUpdate,
    bit: u32,
    bits: impl Fn(&protocol::AbilityLayerEvidence) -> u32,
) -> Option<f64> {
    let speed = f32::from_bits(bits(resolved_layer(update, bit)?));
    (speed.is_finite() && (bit != ability_bit::FLY_SPEED || speed >= 0.0))
        .then_some(f64::from(speed))
}

#[cfg(test)]
mod tests {
    use protocol::{AbilityLayerEvidence, AbilityLayersEvidence};

    use super::*;

    fn update(speeds: &[f32]) -> AbilitiesUpdate {
        let layers: Vec<AbilityLayerEvidence> = speeds
            .iter()
            .map(|speed| AbilityLayerEvidence {
                layer_type: 1,
                abilities: ability_bit::FLY_SPEED,
                values: 0,
                fly_speed_bits: speed.to_bits(),
                vertical_fly_speed_bits: 0,
                walk_speed_bits: 0,
            })
            .collect();
        AbilitiesUpdate {
            actor_unique_id: 1,
            player_permission: 0,
            command_permission: 0,
            layers: AbilityLayersEvidence::Received(layers.into()),
        }
    }

    #[test]
    fn flight_speed_takes_the_last_usable_layer() {
        assert_eq!(
            flight_speed(&update(&[0.05, 0.1]), ability_bit::FLY_SPEED, |layer| layer
                .fly_speed_bits),
            Some(f64::from(0.1_f32))
        );
        assert_eq!(
            flight_speed(&update(&[0.05, 0.0]), ability_bit::FLY_SPEED, |layer| layer
                .fly_speed_bits),
            Some(0.0)
        );
        assert_eq!(
            flight_speed(
                &update(&[f32::NAN, -1.0]),
                ability_bit::FLY_SPEED,
                |layer| layer.fly_speed_bits
            ),
            None
        );
        assert_eq!(
            flight_speed(&update(&[]), ability_bit::FLY_SPEED, |layer| layer
                .fly_speed_bits),
            None
        );
    }

    /// Placeholder floats in layers without FlySpeed must not override it.
    #[test]
    fn flight_speed_ignores_undefined_layer_float() {
        let mut update = update(&[0.1, 0.05]);
        let AbilityLayersEvidence::Received(layers) = &mut update.layers else {
            unreachable!()
        };
        let layers = std::sync::Arc::make_mut(layers);
        layers[0].layer_type = 1;
        layers[1].layer_type = 2;
        layers[1].abilities = 0;
        assert_eq!(
            flight_speed(&update, ability_bit::FLY_SPEED, |layer| layer
                .fly_speed_bits),
            Some(f64::from(0.1_f32))
        );
    }

    /// Layer type, not serialization order, sets ability precedence.
    #[test]
    fn flight_speed_respects_typed_layer_priority() {
        let mut update = update(&[0.1, 0.05]);
        let AbilityLayersEvidence::Received(layers) = &mut update.layers else {
            unreachable!()
        };
        let layers = std::sync::Arc::make_mut(layers);
        layers[0].layer_type = 2;
        layers[1].layer_type = 1;
        assert_eq!(
            flight_speed(&update, ability_bit::FLY_SPEED, |layer| layer
                .fly_speed_bits),
            Some(f64::from(0.1_f32))
        );
    }
}

#[cfg(test)]
mod owner_tests;
