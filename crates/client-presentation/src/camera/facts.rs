//! Producers for the camera's gameplay facts: on-fire, in-portal, flying, bow draw and spyglass scoping.

use crate::local_player::LocalViewPose;

use std::{collections::HashMap, sync::Arc};

use bevy::prelude::{Res, ResMut, Resource};
use protocol::{AbilitiesUpdate, AbilityLayersEvidence, ActorMetadataValue};

use super::{fov::CameraFovInputs, presentation::ScreenEffectFacts};

const FLAGS_METADATA_KEY: u32 = 0;
const ACTOR_FLAG_ON_FIRE: u32 = 0;
const ABILITY_FLYING_BIT: u32 = 1 << 9;
const BOW_IDENTIFIER: &str = "minecraft:bow";
const SPYGLASS_IDENTIFIER: &str = "minecraft:spyglass";
const NETHER_PORTAL_IDENTIFIER: &str = "minecraft:portal";

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

fn metadata_flag(metadata: &HashMap<u32, ActorMetadataValue>, bit: u32) -> bool {
    matches!(
        metadata.get(&FLAGS_METADATA_KEY),
        Some(ActorMetadataValue::Flags(flags)) if flags & (1_u64 << bit) != 0
    )
}

/// True when some received ability layer both defines and enables flying.
fn flying_from_abilities(update: &AbilitiesUpdate) -> bool {
    match &update.layers {
        AbilityLayersEvidence::Received(layers) => layers.iter().any(|layer| {
            layer.abilities & ABILITY_FLYING_BIT != 0 && layer.values & ABILITY_FLYING_BIT != 0
        }),
        AbilityLayersEvidence::Unavailable { .. } => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn collect_screen_effect_facts(
    player_runtime: &player_state::PlayerState,
    time: Res<bevy::prelude::Time>,
    view: Res<LocalViewPose>,
    item_use: Option<&dyn crate::observations::ItemUseObservation>,
    ui: Option<&client_ui::ui_runtime::UiRuntime>,
    client_world: Option<crate::observations::WorldObservation<'_>>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    mut clock: ResMut<ItemUseClock>,
    mut facts: ResMut<ScreenEffectFacts>,
    mut fov: ResMut<CameraFovInputs>,
) {
    let stream = client_world
        .as_ref()
        .and_then(|world| world.stream.as_ref());

    facts.on_fire = stream
        .and_then(|stream| stream.actor(stream.local_player_runtime_id()))
        .is_some_and(|actor| metadata_flag(&actor.metadata, ACTOR_FLAG_ON_FIRE));

    facts.in_portal = match (stream, collisions) {
        (Some(stream), Some(collisions)) => {
            let world = sim::PaletteWorld::new(
                stream.collision_store(),
                collisions.registry(stream.network_id_mode()),
                stream.current_dimension(),
            );
            let eye = view.eye_translation();
            [eye.y, view.feet_translation().y].into_iter().any(|y| {
                let block = [eye.x.floor() as i32, y.floor() as i32, eye.z.floor() as i32];
                world.primary_runtime_id(block).is_ok_and(|runtime_id| {
                    collisions.block_identifier(stream.network_id_mode(), runtime_id)
                        == Some(NETHER_PORTAL_IDENTIFIER)
                })
            })
        }
        _ => false,
    };

    let selected = ui.and_then(|ui| {
        let stack = ui.selected_stack(player_runtime)?;
        stream?.canonical_item_stack(stack)?.identifier
    });
    let use_held = item_use.is_some_and(|item_use| item_use.is_using());
    clock.advance(selected.as_ref(), use_held, time.delta_secs());
    let using =
        |identifier: &str| use_held && selected.as_deref().is_some_and(|item| item == identifier);
    fov.bow_draw_seconds = using(BOW_IDENTIFIER).then(|| clock.held_seconds());
    fov.spyglass_scoping = using(SPYGLASS_IDENTIFIER);
    fov.flying = ui
        .and_then(|ui| ui.local_abilities(player_runtime))
        .is_some_and(flying_from_abilities);
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

    #[test]
    fn on_fire_reads_the_primary_flag_word() {
        let mut metadata = HashMap::new();
        assert!(!metadata_flag(&metadata, ACTOR_FLAG_ON_FIRE));
        metadata.insert(0, ActorMetadataValue::Flags(1));
        assert!(metadata_flag(&metadata, ACTOR_FLAG_ON_FIRE));
        metadata.insert(0, ActorMetadataValue::Flags(2));
        assert!(!metadata_flag(&metadata, ACTOR_FLAG_ON_FIRE));
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
