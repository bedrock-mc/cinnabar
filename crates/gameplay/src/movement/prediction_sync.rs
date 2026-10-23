//! Sends `ClientMovementPredictionSync` after the server corrected the local player.

use protocol_ids::{ActorDataKey, ActorFlag, Attribute};
mod countdown;
#[cfg(test)]
mod presentation_tests;

pub(super) use countdown::PredictionSyncCountdown;

use std::collections::HashMap;

use protocol::{ActorMetadataValue, MovementPredictionSync, client_movement_prediction_sync};

use super::LocalPhysicsController;

/// Attribute names in wire order; undefined values are sent as zero.
const ATTRIBUTE_NAMES: [&str; 6] = [
    Attribute::Movement.wire_name(),
    Attribute::UnderwaterMovement.wire_name(),
    Attribute::LavaMovement.wire_name(),
    Attribute::HorseJumpStrength.wire_name(),
    Attribute::Health.wire_name(),
    Attribute::PlayerHunger.wire_name(),
];
/// Optional modifier attributes in wire order, with the value sent while one is undefined.
const MODIFIER_ATTRIBUTES: [(&str, f32); 2] = [
    (Attribute::FrictionModifier.wire_name(), 1.0),
    (Attribute::Bounciness.wire_name(), 0.0),
];
/// Air drag modifier sent while the attribute is undefined.
const UNDEFINED_AIR_DRAG_MODIFIER: f32 = 1.0;

/// Sends a due correction sync and clears its countdown only after admission.
/// `air_drag_modifier` is the session's last accepted finite value, which
/// outlives the replay history that dimension waits and hard corrections clear.
/// `movement_speed` is the accepted attribute current after local sprint and effect changes.
/// `using_item` is the accepted local item use.
pub fn send_movement_prediction_sync(
    physics: &mut LocalPhysicsController,
    stream: &impl crate::GameplayWorld,
    air_drag_modifier: Option<f32>,
    movement_speed: Option<f64>,
    using_item: Option<bool>,
    send: impl FnOnce(protocol::Packet) -> bool,
) {
    if !physics.prediction_sync.due() {
        return;
    }
    let Some(actor) = stream.actor(stream.local_player_runtime_id()) else {
        return;
    };
    physics.note_server_dimensions(actor.explicit_dimensions_revision);
    let mut attributes = attributes(
        |name| {
            actor
                .attributes
                .get(name)
                .map(|attribute| attribute.current)
        },
        air_drag_modifier,
    );
    if let Some(current) = movement_speed {
        attributes[0] = current as f32;
    }
    let actor_flags =
        predicted_flag_words(&actor.metadata, physics.prediction_sync_flags(), using_item);
    let sync = MovementPredictionSync {
        actor_flags,
        bounding_box: predicted_bounding_box(
            &actor.metadata,
            physics.prediction_sync_size(),
            physics.prediction_sync_explicit_size(),
            actor_flags,
        ),
        attributes,
        unique_id: stream.local_player_unique_id(),
        flying: physics.prediction_sync_flying(),
    };
    if send(client_movement_prediction_sync(sync)) {
        physics.prediction_sync.clear();
    }
}

/// Applies accepted individual writes to the remaining actor metadata.
fn predicted_flag_words(
    metadata: &HashMap<u32, ActorMetadataValue>,
    flags: client_world::MovementFlagUpdate,
    using_item: Option<bool>,
) -> [u64; 3] {
    let mut words = flag_words(metadata);
    for (flag, value) in [
        (ActorFlag::UsingItem, using_item),
        (ActorFlag::Sneaking, flags.sneaking),
        (ActorFlag::Sprinting, flags.sprinting),
        (ActorFlag::Moving, flags.moving),
        (ActorFlag::HasCollision, flags.has_collision),
        (ActorFlag::Swimming, flags.swimming),
        (ActorFlag::Crawling, flags.crawling),
        (ActorFlag::Gliding, flags.gliding),
    ] {
        if let Some(value) = value {
            set_flag(&mut words, flag, value);
        }
    }
    if let Some(climber) = flags.extended_flags {
        let mask = super::physics::ClimberFlags::mask();
        words[1] = (words[1] & !mask) | (climber & mask);
    }
    words
}

/// Word index and mask of one actor flag in the three wire flag words.
fn flag_slot(flag: ActorFlag) -> (usize, u64) {
    let bit = flag.wire_value() as usize;
    (
        bit / u64::BITS as usize,
        1_u64 << (bit % u64::BITS as usize),
    )
}

/// Sets or clears one actor flag across the three wire flag words.
fn set_flag(words: &mut [u64; 3], flag: ActorFlag, enabled: bool) {
    let (word, mask) = flag_slot(flag);
    words[word] = (words[word] & !mask) | if enabled { mask } else { 0 };
}

/// Whether one actor flag is set in the three wire flag words.
fn flag_set(words: &[u64; 3], flag: ActorFlag) -> bool {
    let (word, mask) = flag_slot(flag);
    words[word] & mask != 0
}

/// Reads all server-owned flag words, defaulting absent words to zero.
fn flag_words(metadata: &HashMap<u32, ActorMetadataValue>) -> [u64; 3] {
    let word = |key| match metadata.get(&key) {
        Some(ActorMetadataValue::Flags(bits) | ActorMetadataValue::FlagsExtended(bits)) => *bits,
        _ => 0,
    };
    [
        word(ActorDataKey::Flags.wire_value()),
        word(ActorDataKey::FlagsTwo.wire_value()),
        word(ActorDataKey::FlagsThird.wire_value()),
    ]
}

/// Actor flags that request a horizontal player size.
const HORIZONTAL_POSE_FLAGS: [ActorFlag; 4] = [
    ActorFlag::Gliding,
    ActorFlag::DamageNearbyMobs,
    ActorFlag::Swimming,
    ActorFlag::Crawling,
];

/// Uses the last owning resize, retaining explicit dimensions until it is replaced.
fn predicted_bounding_box(
    metadata: &HashMap<u32, ActorMetadataValue>,
    local_size: Option<[f32; 2]>,
    explicit: bool,
    flags: [u64; 3],
) -> [f32; 3] {
    let mut size = bounding_box(metadata);
    if explicit {
        return size;
    }
    if let Some([width, height]) = local_size {
        size[1] = width;
        size[2] = height;
    }
    if HORIZONTAL_POSE_FLAGS
        .into_iter()
        .any(|flag| flag_set(&flags, flag))
    {
        size[1] = local_size.map_or(sim::PLAYER_WIDTH as f32, |size| size[0]);
        size[2] = size[1];
    }
    size
}

/// Reads the server-supplied model, with defaults for absent or non-finite fields.
fn bounding_box(metadata: &HashMap<u32, ActorMetadataValue>) -> [f32; 3] {
    let float = |key, default| match metadata.get(&key) {
        Some(ActorMetadataValue::Float(value)) if value.is_finite() => *value,
        _ => default,
    };
    [
        float(ActorDataKey::Scale.wire_value(), 1.0),
        float(ActorDataKey::Width.wire_value(), sim::PLAYER_WIDTH as f32),
        float(ActorDataKey::Height.wire_value(), sim::PLAYER_HEIGHT as f32),
    ]
}

/// `air_drag_modifier` is the accepted value, so a skipped non-finite update
/// never reaches the sync; other modifiers skip it here.
fn attributes(current: impl Fn(&str) -> Option<f32>, air_drag_modifier: Option<f32>) -> [f32; 9] {
    let value = |index: usize| current(ATTRIBUTE_NAMES[index]).unwrap_or(0.0);
    let modifier = |index: usize| {
        let (name, undefined) = MODIFIER_ATTRIBUTES[index];
        current(name)
            .filter(|value| value.is_finite())
            .unwrap_or(undefined)
    };
    [
        value(0),
        value(1),
        value(2),
        value(3),
        value(4),
        value(5),
        modifier(0),
        modifier(1),
        air_drag_modifier.unwrap_or(UNDEFINED_AIR_DRAG_MODIFIER),
    ]
}

#[cfg(test)]
mod tests;
