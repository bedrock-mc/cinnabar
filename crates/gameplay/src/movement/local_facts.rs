//! Local ability, hunger and equipment facts that gate movement modes.

use client_world::game_mode_capabilities::{FlightAuthority, ability_bit, resolved_layer};
use protocol::AbilitiesUpdate;
use protocol_ids::Enchantment;

use super::control_modes::SPRINT_HUNGER_FLOOR;
use player_state::PlayerState;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LocalMovementFacts {
    /// The committed mode forces flight and skips terrain contact.
    pub spectator: bool,
    /// Collision bypass does not select a travel mode or grant other permissions.
    pub no_clip: bool,
    pub immobile: bool,
    pub input_locks: protocol::ClientInputLocks,
    pub vertical_physics: sim::VerticalPhysics,
    pub ride: Option<super::RideKind>,
    /// Runtime identity of the retained mount, when its actor is available.
    pub mount_runtime_id: Option<u64>,
    pub ride_seat: Option<[f32; 3]>,
    /// The boat the player steers from its controlling seat.
    pub vehicle: Option<super::vehicle::SteeredVehicle>,
    pub can_fly: bool,
    pub flight: FlightAuthority,
    pub ability_sequence: Option<u64>,
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
            .is_some_and(|stack| {
                elytra_flies(
                    &stack,
                    player
                        .inventory
                        .item_facts()
                        .get(chestplate.network_id)
                        .traits,
                    chestplate_damage,
                )
            });
    let can_stand_on_snow = !boots.is_empty()
        && player
            .inventory
            .item_facts()
            .get(boots.network_id)
            .traits
            .contains(assets::facts::ItemTraits::POWDER_SNOW_WALKER);
    let capabilities = player.facts.game_mode_capabilities();
    let can_fly = capabilities.is_some_and(|capabilities| capabilities.can_fly);
    let spectator = player.facts.player_game_mode() == Some(protocol::PlayerGameMode::Spectator);
    let no_clip = capabilities.is_some_and(|caps| !caps.has_collision);
    let hunger_exempt = can_fly || spectator;
    let hunger_below_floor = player.facts.hunger().map(|hunger| {
        u32::from(hunger.current()) <= u32::from(SPRINT_HUNGER_FLOOR) * u32::from(hunger.scale())
    });
    let boots_level = |id| protocol::item_enchantment_level(&boots.extra_data, id).unwrap_or(0);
    let ride = player.facts.mount_unique_id().map(|unique| {
        stream
            .actor_by_unique_id(unique)
            .map(|actor| super::RideKind::from_native_class(actor.entity_fact.native_class))
            .unwrap_or(super::RideKind::Other)
    });
    LocalMovementFacts {
        spectator,
        no_clip,
        immobile: player.facts.is_immobile(),
        input_locks: player.facts.input_locks(),
        vertical_physics: sim::VerticalPhysics {
            has_gravity: player.facts.has_gravity(),
            uniform_air_drag: player.facts.uses_uniform_air_drag(),
            air_drag_modifier: player.facts.air_drag_modifier().map(f64::from),
        },
        ride,
        mount_runtime_id: player
            .facts
            .mount_unique_id()
            .and_then(|unique| stream.actor_by_unique_id(unique))
            .map(|actor| actor.runtime_id),
        ride_seat: ride
            .and(stream.local_rider_seat_pose())
            .map(|(position, _)| position),
        vehicle: (ride == Some(super::RideKind::Boat))
            .then(|| steered_vehicle(stream))
            .flatten(),
        can_fly,
        flight: capabilities.map(|caps| caps.flight).unwrap_or_default(),
        ability_sequence: player.facts.local_abilities_sequence(),
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
        depth_strider: boots_level(i16::from(Enchantment::Depthstrider.wire_value())),
        soul_speed: boots_level(i16::from(Enchantment::Soulspeed.wire_value())),
        swift_sneak: protocol::item_enchantment_level(
            &leggings.extra_data,
            i16::from(Enchantment::Swiftsneak.wire_value()),
        )
        .unwrap_or(0),
        swim_hunger_blocked: !hunger_exempt && hunger_below_floor.unwrap_or(true),
        sprint_blocked: !hunger_exempt && hunger_below_floor.unwrap_or(true),
        sprint_start_blocked: item_in_use,
    }
}

/// The boat the local player steers, with its seat in the boat's frame.
fn steered_vehicle(stream: &impl crate::GameplayWorld) -> Option<super::vehicle::SteeredVehicle> {
    let vehicle = stream.local_controlled_vehicle()?;
    let (seat, _) = stream.local_rider_seat_on([0.0; 3], 0.0)?;
    let (min, max) = vehicle.bounding_box()?;
    Some(super::vehicle::SteeredVehicle {
        runtime_id: vehicle.runtime_id,
        unique_id: vehicle.unique_id,
        body: sim::BoatBody {
            width: max[0] - min[0],
            height: max[1] - min[1],
            height_offset: vehicle.network_position_offset(),
        },
        position: vehicle.last_received_native_position(),
        yaw: vehicle.last_received_pose().yaw,
        velocity: vehicle.native_velocity(),
        seat,
        server_moves: stream.local_vehicle_server_moves(),
    })
}

/// An elytra flies until its damage reaches one below its maximum. An accepted
/// inventory response's damage overrides the stack's stale Damage tag.
fn elytra_flies(
    stack: &client_world::CanonicalItemStack,
    traits: assets::facts::ItemTraits,
    corrected_damage: Option<u32>,
) -> bool {
    let damage = corrected_damage.or(stack.damage).unwrap_or(0);
    traits.contains(assets::facts::ItemTraits::GLIDER)
        && stack
            .durability
            .maximum()
            .is_some_and(|maximum| damage < maximum.saturating_sub(1))
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
    const ELYTRA_IDENTIFIER: &str = "minecraft:elytra";
    const LEATHER_BOOTS_IDENTIFIER: &str = "minecraft:leather_boots";
    use protocol::{AbilityLayerEvidence, AbilityLayersEvidence};

    use super::*;

    /// Resolves the fixture's movement traits from the pinned carrier.
    fn flies(stack: &client_world::CanonicalItemStack, corrected_damage: Option<u32>) -> bool {
        let traits = assets::facts::ItemFacts::builtin()
            .unwrap()
            .by_identifier(stack.identifier.as_deref().unwrap())
            .unwrap()
            .traits;
        elytra_flies(stack, traits, corrected_damage)
    }

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
            use_animation: assets::item_use::ItemUseAnimation::None,
            charged_projectile_role: assets::item_use::ItemProjectileRole::None,
            grip: assets::item::grip::ItemGrip::Unresolved,
            glint: assets::item::glint::ItemGlint::Unresolved,
            durability: assets::item::durability::builtin_item_durability(ELYTRA_IDENTIFIER),
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
        let maximum = assets::item::durability::builtin_item_durability(ELYTRA_IDENTIFIER)
            .maximum()
            .unwrap();
        assert!(flies(&elytra(ELYTRA_IDENTIFIER, None), None));
        assert!(flies(&elytra(ELYTRA_IDENTIFIER, Some(maximum - 2)), None));
        assert!(!flies(&elytra(ELYTRA_IDENTIFIER, Some(maximum - 1)), None));
        assert!(!flies(&elytra(LEATHER_BOOTS_IDENTIFIER, None), None));
    }

    /// A response-corrected repair or break wins over the stack's unchanged Damage tag.
    #[test]
    fn corrected_elytra_damage_overrides_the_stale_tag() {
        let maximum = assets::item::durability::builtin_item_durability(ELYTRA_IDENTIFIER)
            .maximum()
            .unwrap();
        let elytra = |damage| client_world::CanonicalItemStack {
            use_animation: assets::item_use::ItemUseAnimation::None,
            charged_projectile_role: assets::item_use::ItemProjectileRole::None,
            grip: assets::item::grip::ItemGrip::Unresolved,
            glint: assets::item::glint::ItemGlint::Unresolved,
            durability: assets::item::durability::builtin_item_durability(ELYTRA_IDENTIFIER),
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
        assert!(flies(&elytra(Some(maximum - 1)), Some(0)));
        assert!(!flies(&elytra(Some(0)), Some(maximum - 1)));
    }

    #[test]
    fn admitted_session_durability_controls_the_elytra_break_boundary() {
        let mut authority = client_world::WorldAuthority::new(
            protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                local_player_unique_id: 1,
                player_position: [0.; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            std::sync::Arc::new(assets::RuntimeAssets::diagnostic()),
            None,
            [0.; 3],
            None,
        );
        let mut entry = protocol::vanilla_item_registry()
            .iter()
            .find(|entry| entry.identifier.as_ref() == ELYTRA_IDENTIFIER)
            .unwrap()
            .clone();
        entry.components = Some(std::sync::Arc::new(protocol::ItemComponents {
            durability_declared: true,
            max_durability: Some(50),
            ..Default::default()
        }));
        assert!(authority.seed_item_registry(protocol::ItemRegistryEvent {
            entries: vec![entry.clone()].into()
        }));
        let stack = protocol::NetworkItemStack {
            network_id: entry.network_id,
            count: 1,
            ..protocol::NetworkItemStack::empty()
        };
        let canonical = authority.canonical_item_stack(&stack).unwrap();
        assert!(flies(&canonical, Some(48)));
        assert!(!flies(&canonical, Some(49)));
        std::sync::Arc::make_mut(entry.components.as_mut().unwrap()).max_durability = None;
        assert!(authority.seed_item_registry(protocol::ItemRegistryEvent {
            entries: vec![entry].into()
        }));
        assert!(!flies(
            &authority.canonical_item_stack(&stack).unwrap(),
            Some(0)
        ));
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
