//! Producers for the camera's gameplay facts: on-fire, in-portal, flying, walk speed, bow draw and spyglass scoping.

use crate::local_player::LocalViewPose;

use std::sync::Arc;

use bevy::prelude::{Res, ResMut, Resource};
use protocol::{AbilitiesUpdate, AbilityLayerEvidence, AbilityLayersEvidence};

use super::{
    fov::{CameraFovInputs, DEFAULT_WALK_SPEED_ABILITY},
    presentation::ScreenEffectFacts,
};

mod portal;
const ABILITY_FLYING_BIT: u32 = 1 << 9;
const ABILITY_WALK_SPEED_BIT: u32 = 1 << 14;
const BOW_IDENTIFIER: &str = "minecraft:bow";
const SPYGLASS_IDENTIFIER: &str = "minecraft:spyglass";

/// How long the current item has been held in use with the same stack identity.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct ItemUseClock {
    item: Option<Arc<str>>,
    held_seconds: f32,
}

impl ItemUseClock {
    /// Advances the clock; releasing use or switching item restarts it.
    pub fn advance(&mut self, item: Option<&Arc<str>>, use_held: bool, delta_seconds: f32) {
        if !use_held || self.item.as_ref() != item {
            self.held_seconds = 0.0;
        }
        self.item = item.cloned();
        if use_held && delta_seconds.is_finite() && delta_seconds > 0.0 {
            self.held_seconds += delta_seconds;
        }
    }

    #[must_use]
    pub const fn held_seconds(&self) -> f32 {
        self.held_seconds
    }
}

/// The layer that decides `ability`: the highest layer type that sets it, later packets
/// winning a tie.
fn deciding_layer(update: &AbilitiesUpdate, ability: u32) -> Option<&AbilityLayerEvidence> {
    match &update.layers {
        AbilityLayersEvidence::Received(layers) => layers
            .iter()
            .filter(|layer| layer.abilities & ability != 0)
            .max_by_key(|layer| layer.layer_type),
        AbilityLayersEvidence::Unavailable { .. } => None,
    }
}

fn flying_from_abilities(update: &AbilitiesUpdate) -> bool {
    deciding_layer(update, ABILITY_FLYING_BIT)
        .is_some_and(|layer| layer.values & ABILITY_FLYING_BIT != 0)
}

fn walk_speed_from_abilities(update: Option<&AbilitiesUpdate>) -> f32 {
    update
        .and_then(|update| deciding_layer(update, ABILITY_WALK_SPEED_BIT))
        .map(|layer| f32::from_bits(layer.walk_speed_bits))
        .filter(|speed| speed.is_finite())
        .unwrap_or(DEFAULT_WALK_SPEED_ABILITY)
}

#[allow(clippy::too_many_arguments)]
pub fn collect_screen_effect_facts(
    player_runtime: &player_state::PlayerState,
    time: Res<bevy::prelude::Time>,
    item_use: Option<&dyn crate::observations::ItemUseObservation>,
    ui: Option<&client_ui::ui_runtime::UiRuntime>,
    client_world: Option<crate::observations::WorldObservation<'_>>,
    mut clock: ResMut<ItemUseClock>,
    mut facts: ResMut<ScreenEffectFacts>,
    mut fov: ResMut<CameraFovInputs>,
) {
    let stream = client_world
        .as_ref()
        .and_then(|world| world.stream.as_ref());

    facts.on_fire = stream
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        .is_some_and(client_world::ActorSnapshot::is_on_fire);

    let selected = ui.and_then(|_| {
        let stack = player_runtime.selected_stack()?;
        stream?.authority().canonical_item_stack(stack)?.identifier
    });
    let use_held = item_use.is_some_and(|item_use| item_use.is_using());
    clock.advance(selected.as_ref(), use_held, time.delta_secs());
    let using =
        |identifier: &str| use_held && selected.as_deref().is_some_and(|item| item == identifier);
    fov.bow_draw_seconds = using(BOW_IDENTIFIER).then(|| clock.held_seconds());
    fov.spyglass_scoping = using(SPYGLASS_IDENTIFIER);
    let abilities = ui.and_then(|_| player_runtime.facts.local_abilities());
    fov.flying = abilities.is_some_and(flying_from_abilities);
    fov.walk_speed = walk_speed_from_abilities(abilities);
}

pub(super) fn portal_body(
    physics: Option<&dyn crate::observations::PhysicsObservation>,
    view: &LocalViewPose,
) -> sim::Aabb {
    physics
        .and_then(|physics| {
            let state = physics.state()?;
            let sneaking = physics
                .latest_sneak_sprint()
                .is_some_and(|(sneak, _)| sneak);
            Some(sim::Aabb::player_with_height_at(
                state.position,
                physics.mode().hitbox_height(sneaking),
            ))
        })
        .unwrap_or_else(|| {
            let feet = view.feet_translation();
            sim::Aabb::player_at(sim::Vec3::new(
                f64::from(feet.x),
                f64::from(feet.y),
                f64::from(feet.z),
            ))
        })
}

/// Samples body contact after this frame's physics and camera-pose resolution.
pub fn collect_portal_contact(
    player_runtime: &player_state::PlayerState,
    view: &LocalViewPose,
    physics: Option<&dyn crate::observations::PhysicsObservation>,
    client_world: Option<crate::observations::WorldObservation<'_>>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    facts: &mut ScreenEffectFacts,
) {
    let stream = client_world.as_ref().and_then(|world| world.stream);
    facts.in_portal = match (stream, collisions) {
        (Some(stream), Some(collisions))
            if player_runtime.facts.player_game_mode()
                != Some(protocol::PlayerGameMode::Spectator)
                && physics.is_none_or(|physics| physics.mode() != sim::MovementMode::Riding) =>
        {
            let world = sim::PaletteWorld::new(
                stream.collision_store(),
                collisions.registry(stream.network_id_mode()),
                stream.current_dimension(),
            );
            portal::touches_portal(portal_body(physics, view), |block| {
                world.primary_runtime_id(block).is_ok_and(|runtime_id| {
                    collisions.block_identifier(stream.network_id_mode(), runtime_id)
                        == Some(assets::NETHER_PORTAL_IDENTIFIER)
                })
            })
        }
        _ => false,
    };
}

#[cfg(test)]
mod tests {
    use protocol::AbilityLayerEvidence;

    use super::*;

    fn layer(abilities: u32, values: u32) -> AbilityLayerEvidence {
        AbilityLayerEvidence {
            layer_type: 1,
            abilities,
            values,
            fly_speed_bits: 0,
            vertical_fly_speed_bits: 0,
            walk_speed_bits: 0,
        }
    }

    fn walk_layer(layer_type: u16, abilities: u32, walk_speed: f32) -> AbilityLayerEvidence {
        AbilityLayerEvidence {
            layer_type,
            walk_speed_bits: walk_speed.to_bits(),
            ..layer(abilities, 0)
        }
    }

    fn update(layers: AbilityLayersEvidence) -> AbilitiesUpdate {
        AbilitiesUpdate {
            actor_unique_id: 1,
            player_permission: 0,
            command_permission: 0,
            layers,
        }
    }

    #[test]
    fn flying_needs_the_bit_defined_and_enabled() {
        let flying = |layers: Vec<AbilityLayerEvidence>| {
            flying_from_abilities(&update(AbilityLayersEvidence::Received(layers.into())))
        };
        assert!(flying(vec![layer(ABILITY_FLYING_BIT, ABILITY_FLYING_BIT)]));
        assert!(!flying(vec![layer(ABILITY_FLYING_BIT, 0)]));
        assert!(!flying(vec![layer(0, ABILITY_FLYING_BIT)]));
        assert!(!flying_from_abilities(&update(
            AbilityLayersEvidence::Unavailable {
                declared_layers: 99
            }
        )));
    }

    /// The highest layer that sets an ability decides it; unset layers never do.
    #[test]
    fn walk_speed_comes_from_the_highest_layer_that_sets_it() {
        let walk = |layers: Vec<AbilityLayerEvidence>| {
            walk_speed_from_abilities(Some(&update(AbilityLayersEvidence::Received(
                layers.into(),
            ))))
        };
        assert_eq!(
            walk(vec![
                walk_layer(3, ABILITY_WALK_SPEED_BIT, 0.3),
                walk_layer(1, ABILITY_WALK_SPEED_BIT, 0.1),
            ]),
            0.3
        );
        assert_eq!(
            walk(vec![
                walk_layer(1, ABILITY_WALK_SPEED_BIT, 0.2),
                walk_layer(3, 0, 0.5),
            ]),
            0.2
        );
        assert_eq!(walk(vec![]), DEFAULT_WALK_SPEED_ABILITY);
        assert_eq!(walk_speed_from_abilities(None), DEFAULT_WALK_SPEED_ABILITY);
        let flying = |layers: Vec<AbilityLayerEvidence>| {
            flying_from_abilities(&update(AbilityLayersEvidence::Received(layers.into())))
        };
        assert!(!flying(vec![
            AbilityLayerEvidence {
                layer_type: 3,
                ..layer(ABILITY_FLYING_BIT, 0)
            },
            layer(ABILITY_FLYING_BIT, ABILITY_FLYING_BIT),
        ]));
    }

    #[test]
    fn use_clock_restarts_on_release_or_item_change() {
        let bow: Arc<str> = Arc::from("minecraft:bow");
        let other: Arc<str> = Arc::from("minecraft:stick");
        let mut clock = ItemUseClock::default();
        clock.advance(Some(&bow), true, 0.25);
        clock.advance(Some(&bow), true, 0.25);
        assert!((clock.held_seconds() - 0.5).abs() < 1e-6);
        clock.advance(Some(&other), true, 0.25);
        assert!((clock.held_seconds() - 0.25).abs() < 1e-6);
        clock.advance(Some(&other), false, 0.25);
        assert_eq!(clock.held_seconds(), 0.0);
    }
}
