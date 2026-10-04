//! Client-predicted sounds tied to local interaction: block place/hit/break, eating and drinking,
//! actor hurt/death status events, and dropped-item pickup.

use crate::local_player::LocalViewPose;

use std::{collections::HashSet, sync::Arc};

use bevy::prelude::{Local, Message, MessageReader, Res, ResMut, Time};
use client_world::ActorStatusNotice;
use protocol::{ActorKind, ActorStatusKind};
use sim::PaletteWorld;

use super::{
    echo::{EchoOrigin, EchoSubject},
    engine::{AudioEngine, SoundRequest},
    systems::{ACTOR_ECHO_SECONDS, BLOCK_ECHO_SECONDS, block_lookup, identifier_at},
};

const PLAYER: &str = "minecraft:player";
/// Height fraction of an actor's box standing in for its head attach point.
const HEAD_HEIGHT_FRACTION: f32 = 0.9;
/// Seconds between block hit sounds while mining (`GameMode` 200 ms).
const HIT_INTERVAL: f32 = 0.2;
/// Seconds between eating/drinking sounds while an item is in use; needs native measurement.
const CONSUME_INTERVAL: f32 = 0.25;
const SECONDS_PER_TICK: f32 = 0.05;

const DRINKS: [&str; 3] = [
    "minecraft:potion",
    "minecraft:milk_bucket",
    "minecraft:ominous_bottle",
];
const FOODS: &[&str] = &[
    "minecraft:apple",
    "minecraft:golden_apple",
    "minecraft:enchanted_golden_apple",
    "minecraft:baked_potato",
    "minecraft:potato",
    "minecraft:poisonous_potato",
    "minecraft:carrot",
    "minecraft:golden_carrot",
    "minecraft:beetroot",
    "minecraft:beetroot_soup",
    "minecraft:bread",
    "minecraft:cookie",
    "minecraft:melon_slice",
    "minecraft:pumpkin_pie",
    "minecraft:mushroom_stew",
    "minecraft:rabbit_stew",
    "minecraft:suspicious_stew",
    "minecraft:beef",
    "minecraft:cooked_beef",
    "minecraft:porkchop",
    "minecraft:cooked_porkchop",
    "minecraft:chicken",
    "minecraft:cooked_chicken",
    "minecraft:mutton",
    "minecraft:cooked_mutton",
    "minecraft:rabbit",
    "minecraft:cooked_rabbit",
    "minecraft:cod",
    "minecraft:cooked_cod",
    "minecraft:salmon",
    "minecraft:cooked_salmon",
    "minecraft:tropical_fish",
    "minecraft:pufferfish",
    "minecraft:rotten_flesh",
    "minecraft:spider_eye",
    "minecraft:dried_kelp",
    "minecraft:sweet_berries",
    "minecraft:glow_berries",
    "minecraft:chorus_fruit",
];

/// A local block interaction the audio runtime should voice before the server confirms it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Message)]
pub enum LocalBlockCue {
    /// A locally completed destroy; preserve its id before prediction replaces it with air.
    Break {
        position: [i32; 3],
        block_runtime_id: i32,
    },
    /// A block item was placed at `position`; the id is the item's block runtime id.
    Place {
        position: [i32; 3],
        block_runtime_id: i32,
    },
}

#[derive(Debug, Default)]
pub struct MiningAudio {
    target: Option<([i32; 3], Option<String>)>,
    hit_timer: f32,
}

fn center(cell: [i32; 3]) -> [f32; 3] {
    cell.map(|axis| axis as f32 + 0.5)
}

/// The player sound event voiced while `identifier` is being consumed.
pub fn is_consumable(identifier: &str) -> Option<&'static str> {
    if identifier == "minecraft:honey_bottle" {
        Some("drink.honey")
    } else if DRINKS.contains(&identifier) {
        Some("drink")
    } else if FOODS.contains(&identifier) {
        Some("eat")
    } else {
        None
    }
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
    );
    let mut requests: Vec<(&'static str, Option<String>, [i32; 3])> = Vec::new();
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
        requests.push((event, lookup(block_runtime_id as u32), position));
    }
    let target = survival
        .and_then(|survival| survival.destroying_target())
        .map(|(cell, _face)| cell);
    let previous = mining.target.as_ref().map(|(cell, _)| *cell);
    if target != previous {
        mining.hit_timer = 0.0;
        mining.target = target.map(|cell| (cell, identifier_at(&palette, collisions, mode, cell)));
    }
    if let Some(cell) = mining.target.as_ref().map(|(cell, _)| *cell) {
        mining.hit_timer -= time.delta_secs();
        if mining.hit_timer <= 0.0 {
            mining.hit_timer = HIT_INTERVAL;
            let identifier = mining.target.as_ref().and_then(|(_, id)| id.clone());
            requests.push(("hit", identifier, cell));
        }
    }
    let built: Vec<(&'static str, [i32; 3], SoundRequest)> = {
        let Some(bank) = engine.bank() else { return };
        let tables = bank.tables();
        requests
            .into_iter()
            .filter_map(|(event, identifier, cell)| {
                let material = tables.material_of(identifier.as_deref()?)?;
                let route = tables.block(material, event)?;
                let request = SoundRequest::new(route.sound)
                    .with_ranges(route.volume, route.pitch)
                    .at(center(cell));
                Some((event, cell, request))
            })
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
/// food with a burp once its pack use duration elapses.
pub fn drive_consume_audio(
    player_runtime: &player_state::PlayerState,
    time: Res<Time>,
    ui: Option<&client_ui::ui_runtime::UiRuntime>,
    world: crate::observations::WorldObservation<'_>,
    view: Res<LocalViewPose>,
    mut engine: ResMut<AudioEngine>,
    mut state: Local<ConsumeAudio>,
) {
    let stream = world.stream.as_ref();
    let using = stream
        .and_then(|stream| stream.actor(stream.local_player_runtime_id()))
        .is_some_and(|actor| actor.is_using_item());
    let item = stream.zip(ui).filter(|_| using).and_then(|(stream, ui)| {
        let identifier = stream
            .canonical_item_stack(ui.selected_stack(player_runtime)?)?
            .identifier?;
        let event = is_consumable(&identifier)?;
        Some((identifier, event))
    });
    let duration = stream
        .zip(item.as_ref())
        .and_then(|(stream, (identifier, _))| {
            Some(stream.item_max_use_ticks(identifier)? as f32 * SECONDS_PER_TICK)
        });
    let cues = state.advance(item, duration, time.delta_secs());
    if cues.is_empty() {
        return;
    }
    let eye = view.eye_translation();
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
    notice: &ActorStatusNotice,
) -> Option<(&'static str, SoundRequest)> {
    let event = match notice.kind {
        ActorStatusKind::Hurt | ActorStatusKind::HurtWithoutDamage => "hurt",
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

/// Voices every actor's hurt/death status and dropped-item pickups.
pub fn drive_actor_audio(
    world: crate::observations::WorldObservation<'_>,
    inbox: Option<&mut dyn crate::observations::ParticleAudioObservation>,
    mut engine: ResMut<AudioEngine>,
    mut popped: Local<HashSet<u64>>,
) {
    let Some(stream) = world.stream.as_ref() else {
        popped.clear();
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
        let (identifier, unique_id) = if notice.runtime_id == local {
            (PLAYER, stream.local_player_unique_id())
        } else {
            let Some(actor) = stream.actor(notice.runtime_id) else {
                continue;
            };
            let identifier = match &actor.kind {
                ActorKind::Player { .. } => PLAYER,
                ActorKind::Entity { identifier } => identifier.as_ref(),
            };
            (identifier, actor.unique_id)
        };
        let Some((event, request)) = engine
            .bank()
            .and_then(|bank| status_request(bank.tables(), identifier, notice))
        else {
            continue;
        };
        let subject = EchoSubject::Actor(unique_id);
        if engine.admit_echo(EchoOrigin::Client, event, subject, ACTOR_ECHO_SECONDS) {
            engine.enqueue(request);
        }
    }
    let items = stream.dropped_items(0.0);
    popped.retain(|id| items.iter().any(|item| item.runtime_id == *id));
    for item in &items {
        let collected = stream
            .actor(item.runtime_id)
            .is_some_and(|actor| actor.status.pickup.is_some());
        if collected && popped.insert(item.runtime_id) {
            let lookup = engine.bank().map_or(assets::RouteLookup::Absent, |bank| {
                bank.tables().individual_lookup("pop")
            });
            let request = match lookup {
                assets::RouteLookup::Route(route) => {
                    SoundRequest::new(route.sound).with_ranges(route.volume, route.pitch)
                }
                assets::RouteLookup::Absent => SoundRequest::new("random.pop"),
                assets::RouteLookup::Silent => continue,
            };
            engine.enqueue(request.at(item.position));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumables_are_classified() {
        assert_eq!(is_consumable("minecraft:bread"), Some("eat"));
        assert_eq!(is_consumable("minecraft:potion"), Some("drink"));
        assert_eq!(is_consumable("minecraft:honey_bottle"), Some("drink.honey"));
        assert_eq!(is_consumable("minecraft:experience_bottle"), None, "thrown");
        assert_eq!(is_consumable("minecraft:stone"), None);
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
        };
        let (event, hurt) =
            status_request(&tables, "minecraft:zombie", &notice(ActorStatusKind::Hurt)).unwrap();
        assert_eq!((event, &*hurt.name), ("hurt", "mob.zombie.hurt"));
        assert_eq!(hurt.position, Some([1.0, 65.8, 1.0]));
        let (_, death) =
            status_request(&tables, "minecraft:zombie", &notice(ActorStatusKind::Death)).unwrap();
        assert_eq!(&*death.name, "game.death");
        let silent = status_request(
            &tables,
            "minecraft:armor_stand",
            &notice(ActorStatusKind::Hurt),
        );
        assert!(silent.is_none());
    }

    /// `GameMode` spaces mining hit sounds 200 ms apart.
    #[test]
    fn mining_hits_follow_the_reference_interval() {
        assert_eq!(HIT_INTERVAL, 0.2);
    }

    #[test]
    fn cell_centers_offset_by_half_a_block() {
        assert_eq!(center([1, -2, 3]), [1.5, -1.5, 3.5]);
    }
}
