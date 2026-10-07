//! Sends `ClientMovementPredictionSync` after the server corrected the local player.

mod countdown;

pub(super) use countdown::PredictionSyncCountdown;

use std::collections::HashMap;

use protocol::{ActorMetadataValue, MovementPredictionSync, client_movement_prediction_sync};

use super::LocalPhysicsController;

const FLAGS_KEY: u32 = 0;
const EXTENDED_FLAGS_KEY: u32 = 92;
const SCALE_KEY: u32 = 38;
const WIDTH_KEY: u32 = 53;
const HEIGHT_KEY: u32 = 54;

/// Attribute names in wire order of the sync's attribute block; all must be set to send.
const ATTRIBUTE_NAMES: [&str; 6] = [
    "minecraft:movement",
    "minecraft:underwater_movement",
    "minecraft:lava_movement",
    "minecraft:horse.jump_strength",
    "minecraft:health",
    "minecraft:player.hunger",
];
/// Optional modifier attributes in wire order, with the value sent while one is undefined.
const MODIFIER_ATTRIBUTES: [(&str, f32); 3] = [
    ("minecraft:friction_modifier", 1.0),
    ("minecraft:bounciness", 0.0),
    (client_world::AIR_DRAG_MODIFIER_ATTRIBUTE, 1.0),
];

#[derive(Default)]
pub struct PredictionSyncState {
    /// Syncs withheld because a required attribute was unset.
    skipped: u64,
    skip_logged: bool,
}

/// Sends a due correction sync and clears its countdown only after admission.
pub fn send_movement_prediction_sync(
    physics: &mut LocalPhysicsController,
    stream: &impl crate::GameplayWorld,
    state: &mut PredictionSyncState,
    send: impl FnOnce(protocol::Packet) -> bool,
) {
    if !physics.prediction_sync.due() {
        return;
    }
    let Some(actor) = stream.actor(stream.local_player_runtime_id()) else {
        return;
    };
    let Some(attributes) = attributes(|name| {
        actor
            .attributes
            .get(name)
            .map(|attribute| attribute.current)
    }) else {
        // An unset attribute would read as zero, which a server may treat as a cheat; retry later.
        if !state.skip_logged {
            state.skipped = state.skipped.saturating_add(1);
            state.skip_logged = true;
            tracing::debug!(
                skipped = state.skipped,
                "prediction sync withheld: attribute unset"
            );
        }
        return;
    };
    state.skip_logged = false;
    let sync = MovementPredictionSync {
        actor_flags: flag_words(&actor.metadata),
        bounding_box: bounding_box(&actor.metadata),
        attributes,
        unique_id: stream.local_player_unique_id(),
        flying: physics.mode() == sim::MovementMode::Flying,
    };
    if send(client_movement_prediction_sync(sync)) {
        physics.prediction_sync.clear();
    }
}

fn flag_words(metadata: &HashMap<u32, ActorMetadataValue>) -> [u64; 3] {
    let word = |key| match metadata.get(&key) {
        Some(ActorMetadataValue::Flags(bits) | ActorMetadataValue::FlagsExtended(bits)) => *bits,
        _ => 0,
    };
    [
        word(FLAGS_KEY),
        word(EXTENDED_FLAGS_KEY),
        word(protocol::ACTOR_DATA_ID_FLAGS_THIRD),
    ]
}

fn bounding_box(metadata: &HashMap<u32, ActorMetadataValue>) -> [f32; 3] {
    let float = |key, default| match metadata.get(&key) {
        Some(ActorMetadataValue::Float(value)) if value.is_finite() => *value,
        _ => default,
    };
    [
        float(SCALE_KEY, 1.0),
        float(WIDTH_KEY, sim::PLAYER_WIDTH as f32),
        float(HEIGHT_KEY, sim::PLAYER_HEIGHT as f32),
    ]
}

fn attributes(current: impl Fn(&str) -> Option<f32>) -> Option<[f32; 9]> {
    let value = |index: usize| current(ATTRIBUTE_NAMES[index]);
    let modifier = |index: usize| {
        let (name, undefined) = MODIFIER_ATTRIBUTES[index];
        current(name).unwrap_or(undefined)
    };
    Some([
        value(0)?,
        value(1)?,
        value(2)?,
        value(3)?,
        value(4)?,
        value(5)?,
        modifier(0),
        modifier(1),
        modifier(2),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_read_all_three_words_and_missing_ones_are_zero() {
        let mut metadata = HashMap::new();
        assert_eq!(flag_words(&metadata), [0; 3]);
        metadata.insert(FLAGS_KEY, ActorMetadataValue::Flags(0b1010));
        metadata.insert(EXTENDED_FLAGS_KEY, ActorMetadataValue::FlagsExtended(1));
        assert_eq!(flag_words(&metadata), [0b1010, 1, 0]);
        metadata.insert(
            protocol::ACTOR_DATA_ID_FLAGS_THIRD,
            ActorMetadataValue::FlagsExtended(1),
        );
        assert_eq!(flag_words(&metadata), [0b1010, 1, 1]);
    }

    #[test]
    fn bounding_box_falls_back_to_the_player_box() {
        let mut metadata = HashMap::new();
        assert_eq!(bounding_box(&metadata), [1.0, 0.6, 1.8]);
        metadata.insert(HEIGHT_KEY, ActorMetadataValue::Float(0.6));
        metadata.insert(WIDTH_KEY, ActorMetadataValue::Float(f32::NAN));
        assert_eq!(bounding_box(&metadata), [1.0, 0.6, 0.6]);
    }

    #[test]
    fn any_unset_attribute_withholds_the_sync_and_undefined_modifiers_use_their_defaults() {
        assert_eq!(attributes(|_| None), None);
        assert_eq!(
            attributes(|name| (name != "minecraft:player.hunger").then_some(0.1)),
            None
        );
        let required = |name: &str| ATTRIBUTE_NAMES.contains(&name).then_some(0.1);
        let block = attributes(required).unwrap();
        assert_eq!(&block[..6], &[0.1; 6]);
        assert_eq!(&block[6..], &[1.0, 0.0, 1.0]);
        let block = attributes(|_| Some(0.1)).unwrap();
        assert_eq!(
            &block[6..],
            &[0.1; 3],
            "defined modifiers send their current"
        );
    }
}
