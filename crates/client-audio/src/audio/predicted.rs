//! Client-predicted sounds tied to local interaction: block place/hit/break, eating and drinking,
//! actor hurt/death status events, and dropped-item pickup.

use std::sync::Arc;

use bevy::prelude::{Local, Message, MessageReader, MessageWriter, Res, ResMut, Time};
use client_world::ActorStatusNotice;
use protocol::{ActorKind, ActorStatusKind};
use sim::PaletteWorld;

use super::{
    echo::{EchoOrigin, EchoSubject},
    engine::{AudioEngine, SoundRequest},
    systems::{ACTOR_ECHO_SECONDS, BLOCK_ECHO_SECONDS, block_lookup, identifier_ref},
};

const PLAYER: &str = assets::facts::VanillaEntity::Player.identifier();
/// Height fraction of an actor's box standing in for its head attach point.
const HEAD_HEIGHT_FRACTION: f32 = 0.9;
/// Seconds between block hit sounds while mining (vanilla 200 ms).
const HIT_INTERVAL: f32 = 0.2;
/// Seconds between eating/drinking sounds while an item is in use; needs native measurement.
const CONSUME_INTERVAL: f32 = 0.25;
const SECONDS_PER_TICK: f32 = simulation_clock::TICK_DURATION.as_secs_f32();

/// One bite of the local player's food, in step with its eating sound; particles scatter the
/// item's crumbs from it.
#[derive(Debug, Clone, PartialEq, Eq, Message)]
pub struct LocalEatingCue {
    pub identifier: Arc<str>,
    pub metadata: u32,
}

/// A local block interaction the audio runtime should voice before the server confirms it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Message)]
pub enum LocalBlockCue {
    /// A locally completed destroy; preserve its id before prediction replaces it with air.
    Break {
        position: [i32; 3],
        block_runtime_id: i32,
    },
    /// A block item was placed at `position`; the id uses the local store palette.
    Place {
        position: [i32; 3],
        block_runtime_id: i32,
    },
}

#[derive(Debug, Default)]
pub struct MiningAudio {
    target: Option<([i32; 3], Option<String>, Option<String>)>,
    hit_timer: f32,
}

/// A predicted block sound with the material captured before the world changes.
struct BlockSoundCue {
    event: &'static str,
    identifier: Option<String>,
    component_material: Option<String>,
    cell: [i32; 3],
}

fn center(cell: [i32; 3]) -> [f32; 3] {
    cell.map(|axis| axis as f32 + 0.5)
}

/// Selects the sound event from the same admitted consumption fact as first-person animation.
pub fn consume_event(consumption: assets::item_use::ItemConsumption) -> Option<&'static str> {
    match consumption {
        assets::item_use::ItemConsumption::Eat => Some("eat"),
        assets::item_use::ItemConsumption::Drink => Some("drink"),
        assets::item_use::ItemConsumption::Honey => Some("drink.honey"),
        assets::item_use::ItemConsumption::Unresolved | assets::item_use::ItemConsumption::None => {
            None
        }
    }
}

/// Whether the admitted use scatters food crumbs through local eating cues.
pub fn predicts_bites(consumption: assets::item_use::ItemConsumption) -> bool {
    matches!(
        actor_animation::consumables::consume_animation(consumption),
        Some(actor_animation::consumables::ConsumeAnimation::Eat)
    )
}

/// Voices local block placement, mining hits and the predicted break of the mined block.
#[allow(clippy::too_many_arguments)]
pub fn drive_block_cues(
    mut cues: MessageReader<LocalBlockCue>,
    time: Res<Time>,
    world: crate::observations::WorldObservation<'_>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    survival: Option<&dyn crate::observations::MiningObservation>,
    mut engine: ResMut<AudioEngine>,
    mut mining: Local<MiningAudio>,
) {
    let (Some(stream), Some(collisions)) = (world.stream.as_ref(), collisions) else {
        cues.clear();
        *mining = MiningAudio::default();
        return;
    };
    if !engine.has_bank() {
        cues.clear();
        return;
    }
    let mode = stream.network_id_mode();
    let lookup = block_lookup(Some(collisions), mode);
    let palette = PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
        Default::default(),
    );
    let mut requests = Vec::new();
    for cue in cues.read() {
        let (event, position, block_runtime_id) = match *cue {
            LocalBlockCue::Place {
                position,
                block_runtime_id,
            } => ("place", position, block_runtime_id),
            LocalBlockCue::Break {
                position,
                block_runtime_id,
            } => ("break", position, block_runtime_id),
        };
        let runtime_id = block_runtime_id as u32;
        requests.push(BlockSoundCue {
            event,
            identifier: lookup(runtime_id),
            component_material: collisions
                .block_sound_material(mode, runtime_id)
                .map(str::to_owned),
            cell: position,
        });
    }
    let target = survival
        .and_then(|survival| survival.destroying_target())
        .map(|(cell, _face)| cell);
    let previous = mining.target.as_ref().map(|(cell, _, _)| *cell);
    if target != previous {
        mining.hit_timer = 0.0;
        mining.target = target.map(|cell| {
            let runtime_id = palette.primary_runtime_id(cell).ok();
            let identifier = identifier_ref(&palette, collisions, mode, cell).map(str::to_owned);
            let component_material = runtime_id
                .and_then(|runtime_id| collisions.block_sound_material(mode, runtime_id))
                .map(str::to_owned);
            (cell, identifier, component_material)
        });
    }
    if let Some(cell) = mining.target.as_ref().map(|(cell, _, _)| *cell) {
        mining.hit_timer -= time.delta_secs();
        if mining.hit_timer <= 0.0 {
            mining.hit_timer = HIT_INTERVAL;
            let (identifier, component_material) = mining
                .target
                .as_ref()
                .map(|(_, identifier, component_material)| {
                    (identifier.clone(), component_material.clone())
                })
                .unwrap_or_default();
            requests.push(BlockSoundCue {
                event: "hit",
                identifier,
                component_material,
                cell,
            });
        }
    }
    let built: Vec<(&'static str, [i32; 3], SoundRequest)> = {
        let Some(bank) = engine.bank() else { return };
        let tables = bank.tables();
        requests
            .into_iter()
            .filter_map(
                |BlockSoundCue {
                     event,
                     identifier,
                     component_material,
                     cell,
                 }| {
                    let identifier = identifier?;
                    let material = tables
                        .material_of_with_component(&identifier, component_material.as_deref());
                    let route = tables.block(material, event)?;
                    let request = SoundRequest::new(route.sound)
                        .with_ranges(route.volume, route.pitch)
                        .at(center(cell));
                    Some((event, cell, request))
                },
            )
            .collect()
    };
    for (event, cell, request) in built {
        if event == "hit"
            || engine.admit_echo(
                EchoOrigin::Client,
                event,
                EchoSubject::Cell(cell),
                BLOCK_ECHO_SECONDS,
            )
        {
            engine.enqueue(request);
        }
    }
}

#[derive(Debug, Default)]
pub struct ConsumeAudio {
    item: Option<Arc<str>>,
    elapsed: f32,
    timer: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsumeCue {
    /// The player sound event of one eating/drinking step.
    Step(&'static str),
    /// The use ran its full duration; food finishes with a burp.
    Finished,
}

impl ConsumeAudio {
    /// Emits crumbs only for an admitted food use with an eating step in this frame.
    fn eating_cue(
        &self,
        consumption: assets::item_use::ItemConsumption,
        metadata: u32,
        cues: &[ConsumeCue],
    ) -> Option<LocalEatingCue> {
        if !predicts_bites(consumption) || !cues.contains(&ConsumeCue::Step("eat")) {
            return None;
        }
        Some(LocalEatingCue {
            identifier: Arc::clone(self.item.as_ref()?),
            metadata,
        })
    }

    /// Advances by `dt` while `item` (identifier, sound event) is in admitted use, completing
    /// every `duration` seconds of continuous use.
    fn advance(
        &mut self,
        item: Option<(Arc<str>, &'static str)>,
        duration: Option<f32>,
        dt: f32,
    ) -> Vec<ConsumeCue> {
        let identifier = item.as_ref().map(|(identifier, _)| Arc::clone(identifier));
        if self.item != identifier {
            *self = Self {
                item: identifier,
                ..Self::default()
            };
        }
        let Some((_, event)) = item else {
            return Vec::new();
        };
        let mut cues = Vec::new();
        self.elapsed += dt;
        self.timer -= dt;
        if self.timer <= 0.0 {
            self.timer = CONSUME_INTERVAL;
            cues.push(ConsumeCue::Step(event));
        }
        if let Some(duration) = duration.filter(|duration| *duration > 0.0)
            // Tolerates frame-sum rounding at an exact tick boundary.
            && self.elapsed >= duration - 1.0e-4
        {
            self.elapsed = (self.elapsed - duration).max(0.0);
            if event == "eat" {
                cues.push(ConsumeCue::Finished);
            }
        }
        cues
    }
}

/// Voices eating and drinking while the server admits the held consumable in use, finishing
/// food with a burp once its pack use duration elapses; each bite of food is also an
/// `eating` cue.
#[allow(clippy::too_many_arguments)]
pub fn drive_consume_audio(
    player_runtime: &player_state::PlayerState,
    time: Res<Time>,
    ui: Option<&client_ui::ui_runtime::UiRuntime>,
    world: crate::observations::WorldObservation<'_>,
    eye: bevy::prelude::Vec3,
    mut engine: ResMut<AudioEngine>,
    mut state: Local<ConsumeAudio>,
    mut eating: MessageWriter<LocalEatingCue>,
) {
    let stream = world.stream.as_ref();
    let using = stream
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        .is_some_and(|actor| actor.is_using_item());
    let admitted = stream.zip(ui).filter(|_| using).and_then(|(stream, _)| {
        let stack = player_runtime.selected_stack()?;
        let profile = stream.authority().item_use_profile(stack.network_id);
        let identifier = stream.authority().canonical_item_stack(stack)?.identifier?;
        Some((
            identifier,
            consume_event(profile.consumption)?,
            profile.ticks?,
            profile.consumption,
        ))
    });
    let duration = admitted
        .as_ref()
        .map(|(_, _, ticks, _)| *ticks as f32 * SECONDS_PER_TICK);
    let consumption = admitted
        .as_ref()
        .map_or_else(Default::default, |(_, _, _, consumption)| *consumption);
    let item = admitted.map(|(identifier, event, _, _)| (identifier, event));
    let cues = state.advance(item, duration, time.delta_secs());
    if cues.is_empty() {
        return;
    }
    if let Some(cue) = state.eating_cue(
        consumption,
        player_runtime
            .selected_stack()
            .map_or(0, |stack| stack.metadata),
        &cues,
    ) {
        eating.write(cue);
    }
    let requests: Vec<SoundRequest> = {
        let Some(bank) = engine.bank() else { return };
        let tables = bank.tables();
        cues.iter()
            .filter_map(|cue| match cue {
                ConsumeCue::Step(event) => tables.entity(PLAYER, event, None),
                ConsumeCue::Finished => tables.individual("burp").cloned(),
            })
            .map(|route| {
                SoundRequest::new(route.sound)
                    .with_ranges(route.volume, route.pitch)
                    .at([eye.x, eye.y, eye.z])
            })
            .collect()
    };
    for request in requests {
        engine.enqueue(request);
    }
}

/// Hurt or death sound of `identifier` for a status `notice`, from the actor's head.
fn status_request(
    tables: &assets::SoundEventTables,
    identifier: &str,
    class: assets::facts::entity::NativeEntityClass,
    notice: &ActorStatusNotice,
) -> Option<(&'static str, SoundRequest)> {
    let event = match notice.kind {
        ActorStatusKind::Hurt | ActorStatusKind::HurtWithoutDamage => "hurt",
        ActorStatusKind::Death
            if class == assets::facts::entity::NativeEntityClass::EnderDragon =>
        {
            return None;
        }
        ActorStatusKind::Death => "death",
        _ => return None,
    };
    let route = tables.entity(identifier, event, None)?;
    let mut head = notice.position;
    head[1] += notice.height.unwrap_or(0.0).max(0.0) * HEAD_HEIGHT_FRACTION;
    let request = SoundRequest::new(route.sound)
        .with_ranges(route.volume, route.pitch)
        .at(head);
    Some((event, request))
}

/// Voices every actor's hurt/death status.
pub fn drive_actor_audio(
    world: crate::observations::WorldObservation<'_>,
    inbox: Option<&mut dyn crate::observations::ParticleAudioObservation>,
    mut engine: ResMut<AudioEngine>,
) {
    let Some(stream) = world.stream.as_ref() else {
        if let Some(inbox) = inbox {
            inbox.take_status_audio();
        }
        return;
    };
    let notices = inbox
        .map(|inbox| inbox.take_status_audio())
        .unwrap_or_default();
    if !engine.has_bank() {
        return;
    }
    let local = stream.local_player_runtime_id();
    for notice in &notices {
        let (identifier, unique_id, class) = if notice.runtime_id == local {
            (
                PLAYER,
                stream.authority().local_player_unique_id(),
                assets::facts::entity::NativeEntityClass::Player,
            )
        } else {
            let Some(actor) = stream.authority().actor(notice.runtime_id) else {
                continue;
            };
            let identifier = match &actor.kind {
                ActorKind::Player { .. } => PLAYER,
                ActorKind::Entity { identifier } => identifier.as_ref(),
            };
            (identifier, actor.unique_id, actor.entity_fact.native_class)
        };
        let Some((event, request)) = engine
            .bank()
            .and_then(|bank| status_request(bank.tables(), identifier, class, notice))
        else {
            continue;
        };
        let subject = EchoSubject::Actor(unique_id);
        if engine.admit_echo(EchoOrigin::Client, event, subject, ACTOR_ECHO_SECONDS) {
            engine.enqueue(request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resolves an original compatibility fixture before checking its consumption sound.
    fn fixture_consume_event(identifier: &str) -> Option<&'static str> {
        let profile = assets::item_use::diagnostic_profile(identifier);
        consume_event(client_world::resolve_item_use(&profile, Some(32), None).consumption)
    }

    #[test]
    fn consumables_are_classified() {
        assert_eq!(fixture_consume_event("minecraft:bread"), Some("eat"));
        assert_eq!(fixture_consume_event("minecraft:potion"), Some("drink"));
        assert_eq!(
            fixture_consume_event("minecraft:honey_bottle"),
            Some("drink.honey")
        );
        assert_eq!(
            fixture_consume_event("minecraft:experience_bottle"),
            None,
            "thrown"
        );
        assert_eq!(fixture_consume_event("minecraft:stone"), None);
    }

    #[test]
    fn only_admitted_eating_uses_predict_local_bites() {
        use assets::item_use::ItemConsumption;
        assert!(predicts_bites(ItemConsumption::Eat));
        for consumption in [
            ItemConsumption::Drink,
            ItemConsumption::Honey,
            ItemConsumption::None,
            ItemConsumption::Unresolved,
        ] {
            assert!(!predicts_bites(consumption));
        }
    }

    #[test]
    fn vanilla_food_overridden_to_drink_emits_no_crumbs() {
        use assets::item_use::ItemConsumption;
        use chunk_pipeline::WorldStream;

        for server_override in [false, true] {
            let mut stream = WorldStream::new(protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                local_player_unique_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
                block_network_ids_are_hashes: false,
            });
            let identifier: Arc<str> = "minecraft:bread".into();
            let network_id = 15000;
            assert!(stream.seed_item_registry(protocol::ItemRegistryEvent {
                entries: Arc::from([protocol::ItemRegistryEntry {
                    identifier: Arc::clone(&identifier),
                    network_id,
                    component_based: true,
                    version: protocol::ItemRegistryVersion::DataDriven,
                    component_digest: [0; 32],
                    components: Some(Arc::new(protocol::ItemComponents {
                        use_duration_ticks: Some(32),
                        use_animation: server_override.then(|| "drink".into()),
                        ..Default::default()
                    })),
                    negotiated_max_stack_size: None,
                    canonical_empty_component_data: false,
                    item_tags: Arc::from([]),
                }]),
            }));
            let mut state = ConsumeAudio::default();
            if !server_override {
                let base = stream.authority().item_use_profile(network_id);
                assert_eq!(base.consumption, ItemConsumption::Eat);
                let cues = state.advance(
                    Some((
                        Arc::clone(&identifier),
                        consume_event(base.consumption).unwrap(),
                    )),
                    Some(base.ticks.unwrap() as f32 * SECONDS_PER_TICK),
                    0.05,
                );
                assert!(state.eating_cue(base.consumption, 0, &cues).is_some());
                stream.set_item_use_overlays(Arc::new(std::collections::BTreeMap::from([(
                    identifier.as_ref().into(),
                    assets::equipment::CompiledItemUseOverlay {
                        identifier: identifier.as_ref().into(),
                        consumption: Some(ItemConsumption::Drink),
                        ..Default::default()
                    },
                )])));
            }
            let effective = stream.authority().item_use_profile(network_id);
            assert_eq!(effective.consumption, ItemConsumption::Drink);
            let cues = state.advance(
                Some((
                    Arc::clone(&identifier),
                    consume_event(effective.consumption).unwrap(),
                )),
                Some(effective.ticks.unwrap() as f32 * SECONDS_PER_TICK),
                CONSUME_INTERVAL,
            );
            assert!(cues.contains(&ConsumeCue::Step("drink")));
            assert!(state.eating_cue(effective.consumption, 0, &cues).is_none());
        }
    }

    // Food only burped after releasing Use past a fixed 1.6 s, whatever the item's duration.
    #[test]
    fn food_finishes_at_its_own_duration_while_still_in_use() {
        let kelp = || Some((Arc::from("minecraft:dried_kelp"), "eat"));
        let mut state = ConsumeAudio::default();
        let mut finished_at = None;
        for frame in 1..=40 {
            let cues = state.advance(kelp(), Some(16.0 * SECONDS_PER_TICK), 0.05);
            if cues.contains(&ConsumeCue::Finished) {
                finished_at.get_or_insert(frame);
            }
        }
        assert_eq!(finished_at, Some(16));
        assert!(
            state.advance(None, None, 0.05).is_empty(),
            "no admitted use"
        );
        let honey = Some((Arc::from("minecraft:honey_bottle"), "drink.honey"));
        let cues = state.advance(honey, Some(0.05), 0.05);
        assert_eq!(cues, vec![ConsumeCue::Step("drink.honey")]);
    }

    // Only the local player's status was voiced; remote players and mobs stayed silent.
    #[test]
    fn remote_actor_status_voices_its_own_hurt_sound_from_the_head() {
        let tables = assets::SoundEventTables::from_json(
            &serde_json::json!({"entity_sounds": {
                "defaults": {"events": {"hurt": "game.hurt", "death": "game.death"}},
                "entities": {"zombie": {"events": {"hurt": "mob.zombie.hurt"}},
                    "armor_stand": {"events": {"hurt": ""}}}}}),
            &serde_json::json!({}),
        );
        let notice = |kind| ActorStatusNotice {
            runtime_id: 9,
            kind,
            data: 0,
            position: [1.0, 64.0, 1.0],
            height: Some(2.0),
            eating_position: [0.0; 3],
        };
        let (event, hurt) = status_request(
            &tables,
            "minecraft:zombie",
            assets::facts::entity::EntityFacts::pinned()
                .get("minecraft:zombie")
                .native_class,
            &notice(ActorStatusKind::Hurt),
        )
        .unwrap();
        assert_eq!((event, &*hurt.name), ("hurt", "mob.zombie.hurt"));
        assert_eq!(hurt.position, Some([1.0, 65.8, 1.0]));
        let (_, death) = status_request(
            &tables,
            "minecraft:zombie",
            assets::facts::entity::EntityFacts::pinned()
                .get("minecraft:zombie")
                .native_class,
            &notice(ActorStatusKind::Death),
        )
        .unwrap();
        assert_eq!(&*death.name, "game.death");
        let silent = status_request(
            &tables,
            "minecraft:armor_stand",
            assets::facts::entity::EntityFacts::pinned()
                .get("minecraft:armor_stand")
                .native_class,
            &notice(ActorStatusKind::Hurt),
        );
        assert!(silent.is_none());
    }

    #[test]
    fn dragon_death_audio_waits_for_the_server_sound_event() {
        let tables = assets::SoundEventTables::from_json(
            &serde_json::json!({"entity_sounds": {"entities": {
                "ender_dragon": {"volume": 80.0, "pitch": [0.8, 1.2], "events": {
                    "death": "mob.enderdragon.death", "hurt": "mob.enderdragon.hit"
                }}
            }}}),
            &serde_json::json!({}),
        );
        let notice = ActorStatusNotice {
            runtime_id: 9,
            kind: ActorStatusKind::Death,
            data: 0,
            position: [1.0, 64.0, 1.0],
            height: Some(4.0),
            eating_position: [0.0; 3],
        };
        assert!(
            status_request(
                &tables,
                "minecraft:ender_dragon",
                assets::facts::entity::EntityFacts::pinned()
                    .get("minecraft:ender_dragon")
                    .native_class,
                &notice
            )
            .is_none(),
            "the death status starts the visual sequence without an early voice"
        );
        let hurt = ActorStatusNotice {
            kind: ActorStatusKind::Hurt,
            ..notice
        };
        assert!(
            status_request(
                &tables,
                "minecraft:ender_dragon",
                assets::facts::entity::EntityFacts::pinned()
                    .get("minecraft:ender_dragon")
                    .native_class,
                &hurt
            )
            .is_some()
        );
        let event = protocol::LevelAudioEvent {
            sound_event: "death".into(),
            position: [1.0, 64.2, 1.0],
            data: -1,
            actor_identifier: "minecraft:ender_dragon".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id: -1,
            fire_at_position: None,
        };
        let request = super::super::route::level_sound_request(&tables, &event, &|_| None)
            .expect("the explicit server event owns the death voice");
        assert_eq!(&*request.name, "mob.enderdragon.death");
        assert_eq!(request.position, Some(event.position));
        assert_eq!(
            request.volume,
            assets::FloatRange {
                min: 80.0,
                max: 80.0
            }
        );
        assert_eq!(request.pitch, assets::FloatRange { min: 0.8, max: 1.2 });
    }

    /// Vanilla spaces mining hit sounds 200 ms apart.
    #[test]
    fn mining_hits_follow_the_reference_interval() {
        assert_eq!(HIT_INTERVAL, 0.2);
    }

    #[test]
    fn cell_centers_offset_by_half_a_block() {
        assert_eq!(center([1, -2, 3]), [1.5, -1.5, 3.5]);
    }
}
