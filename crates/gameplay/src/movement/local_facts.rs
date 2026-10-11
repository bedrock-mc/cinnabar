//! Local ability, hunger and equipment facts that gate movement modes.

use client_world::game_mode_capabilities::{ability_bit, resolved_layer};
use protocol::AbilitiesUpdate;

use super::control_modes::SPRINT_HUNGER_FLOOR;
use player_state::PlayerState;

const ELYTRA_IDENTIFIER: &str = "minecraft:elytra";
const LEATHER_BOOTS_IDENTIFIER: &str = "minecraft:leather_boots";
/// Bedrock enchantment ids; provisional until checked against a native item.
pub(crate) const DEPTH_STRIDER_ENCHANTMENT_ID: i16 = 7;
const SOUL_SPEED_ENCHANTMENT_ID: i16 = 36;
const SWIFT_SNEAK_ENCHANTMENT_ID: i16 = 37;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LocalMovementFacts {
    /// The committed mode forces flight and skips terrain contact.
    pub spectator: bool,
    pub immobile: bool,
    pub vertical_physics: sim::VerticalPhysics,
    pub ride: Option<super::RideKind>,
    pub ride_seat: Option<[f32; 3]>,
    pub can_fly: bool,
    pub server_flying: bool,
    pub fly_speed: Option<f64>,
    pub vertical_fly_speed: Option<f64>,
    pub creative_flight: bool,
    pub elytra_ready: bool,
    pub can_stand_on_snow: bool,
    pub depth_strider: u8,
    pub soul_speed: u8,
    pub swift_sneak: u8,
    pub sprint_blocked: bool,
    pub sprint_start_blocked: bool,
    /// Native swim continuation stops without usable food above the sprint floor,
    /// unless the movement abilities permit flight.
    pub swim_hunger_blocked: bool,
}

pub fn read(
    player: Option<&PlayerState>,
    stream: &impl crate::GameplayWorld,
    item_in_use: bool,
) -> LocalMovementFacts {
    let Some(player) = player else {
        return LocalMovementFacts::default();
    };
    let [_, chestplate, leggings, boots] = player.inventory.local_armor();
    let chestplate_damage = player.inventory.local_armor_damage_corrections()[1];
    let elytra_ready = !chestplate.is_empty()
        && stream
            .canonical_item_stack(&chestplate)
            .is_some_and(|stack| elytra_flies(&stack, chestplate_damage));
    let can_stand_on_snow = !boots.is_empty()
        && stream
            .canonical_item_stack(&boots)
            .and_then(|stack| stack.identifier)
            .is_some_and(|identifier| &*identifier == LEATHER_BOOTS_IDENTIFIER);
    let capabilities = player.facts.game_mode_capabilities();
    let can_fly = capabilities.is_some_and(|capabilities| capabilities.can_fly);
    let spectator = player.facts.player_game_mode() == Some(protocol::PlayerGameMode::Spectator);
    let hunger_exempt = can_fly || spectator;
    let hunger_below_floor = player.facts.hunger().map(|hunger| {
        u32::from(hunger.current()) <= u32::from(SPRINT_HUNGER_FLOOR) * u32::from(hunger.scale())
    });
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
        spectator,
        immobile: player.facts.is_immobile(),
        vertical_physics: sim::VerticalPhysics {
            has_gravity: player.facts.has_gravity(),
            uniform_air_drag: player.facts.uses_uniform_air_drag(),
            air_drag_modifier: player.facts.air_drag_modifier().map(f64::from),
        },
        ride,
        ride_seat: ride
            .and(stream.local_rider_seat_pose())
            .map(|(position, _)| position),
        can_fly,
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
        can_stand_on_snow,
        depth_strider: boots_level(DEPTH_STRIDER_ENCHANTMENT_ID),
        soul_speed: boots_level(SOUL_SPEED_ENCHANTMENT_ID),
        swift_sneak: protocol::item_enchantment_level(
            &leggings.extra_data,
            SWIFT_SNEAK_ENCHANTMENT_ID,
        )
        .unwrap_or(0),
        swim_hunger_blocked: !hunger_exempt && hunger_below_floor.unwrap_or(true),
        sprint_blocked: !hunger_exempt && hunger_below_floor.unwrap_or(true),
        sprint_start_blocked: item_in_use,
    }
}

/// An elytra flies until its damage reaches one below its maximum. An accepted
/// inventory response's damage overrides the stack's stale Damage tag.
fn elytra_flies(stack: &client_world::CanonicalItemStack, corrected_damage: Option<u32>) -> bool {
    let damage = corrected_damage.or(stack.damage).unwrap_or(0);
    stack
        .identifier
        .as_deref()
        .is_some_and(|identifier| identifier == ELYTRA_IDENTIFIER)
        && client_world::vanilla_max_durability(ELYTRA_IDENTIFIER)
            .is_some_and(|maximum| damage < maximum - 1)
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

    /// A damaged elytra stops flying one point of damage before it breaks.
    #[test]
    fn elytra_flies_until_one_damage_below_its_maximum() {
        let elytra = |identifier: &str, damage| client_world::CanonicalItemStack {
            identity: assets::ItemStackIdentity {
                network_id: 1,
                metadata: 0,
                stack_network_id: 0,
                count: 1,
                nbt_digest: [0; 32],
                block_runtime_id: 0,
            },
            identifier: Some(identifier.into()),
            visual: assets::ItemVisualRoute::Missing,
            charged_projectile: None,
            damage,
            enchanted: false,
        };
        let maximum = client_world::vanilla_max_durability(ELYTRA_IDENTIFIER).unwrap();
        assert!(elytra_flies(&elytra(ELYTRA_IDENTIFIER, None), None));
        assert!(elytra_flies(
            &elytra(ELYTRA_IDENTIFIER, Some(maximum - 2)),
            None
        ));
        assert!(!elytra_flies(
            &elytra(ELYTRA_IDENTIFIER, Some(maximum - 1)),
            None
        ));
        assert!(!elytra_flies(&elytra(LEATHER_BOOTS_IDENTIFIER, None), None));
    }

    /// A response-corrected repair or break wins over the stack's unchanged Damage tag.
    #[test]
    fn corrected_elytra_damage_overrides_the_stale_tag() {
        let maximum = client_world::vanilla_max_durability(ELYTRA_IDENTIFIER).unwrap();
        let elytra = |damage| client_world::CanonicalItemStack {
            identity: assets::ItemStackIdentity {
                network_id: 1,
                metadata: 0,
                stack_network_id: 0,
                count: 1,
                nbt_digest: [0; 32],
                block_runtime_id: 0,
            },
            identifier: Some(ELYTRA_IDENTIFIER.into()),
            visual: assets::ItemVisualRoute::Missing,
            charged_projectile: None,
            damage,
            enchanted: false,
        };
        assert!(elytra_flies(&elytra(Some(maximum - 1)), Some(0)));
        assert!(!elytra_flies(&elytra(Some(0)), Some(maximum - 1)));
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
