//! Sends `ClientMovementPredictionSync` after the server corrected the local player.

use protocol_ids::{ActorDataKey, ActorFlag, Attribute};
mod countdown;
#[cfg(test)]
mod presentation_tests;

pub(super) use countdown::PredictionSyncCountdown;

use std::collections::HashMap;

use protocol::{ActorMetadataValue, MovementPredictionSync, client_movement_prediction_sync};

use super::LocalPhysicsController;
use super::physics::SyncPose;

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
    using_item: bool,
    send: impl FnOnce(protocol::Packet) -> bool,
) {
    if !physics.prediction_sync.due() {
        return;
    }
    let Some(actor) = stream.actor(stream.local_player_runtime_id()) else {
        return;
    };
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
        predicted_flag_words(&actor.metadata, physics.prediction_sync_pose(), using_item);
    let sync = MovementPredictionSync {
        actor_flags,
        bounding_box: predicted_bounding_box(
            &actor.metadata,
            physics.prediction_sync_walking_height(),
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

/// Overlays the flags the client owns onto the server's flag words.
///
/// Vanilla sends its own actor flag words. The client itself sets sneaking,
/// sprinting, item use, movement, collision and the swimming, crawling and
/// gliding poses, and adopts the server's changes to them. Cinnabar's local
/// state adopts server sneak, sprint and pose ends but not a server-started
/// pose, item use or riptide spin; the riptide spin, block-climber and every
/// other flag remain the server's.
fn predicted_flag_words(
    metadata: &HashMap<u32, ActorMetadataValue>,
    pose: Option<SyncPose>,
    using_item: bool,
) -> [u64; 3] {
    let mut words = flag_words(metadata);
    set_flag(&mut words, ActorFlag::UsingItem, using_item);
    let Some(pose) = pose else {
        return words;
    };
    for (flag, enabled) in [
        (ActorFlag::Sneaking, pose.sneaking),
        (ActorFlag::Sprinting, pose.sprinting),
        (ActorFlag::Moving, pose.moving),
        (
            ActorFlag::Swimming,
            pose.mode == sim::MovementMode::Swimming,
        ),
        (
            ActorFlag::Crawling,
            pose.mode == sim::MovementMode::Crawling,
        ),
        (ActorFlag::Gliding, pose.mode == sim::MovementMode::Gliding),
    ] {
        set_flag(&mut words, flag, enabled);
    }
    if let Some(has_collision) = pose.has_collision {
        set_flag(&mut words, ActorFlag::HasCollision, has_collision);
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

/// Flags that put a player in a horizontal pose: gliding, a riptide spin, swimming and crawling.
const HORIZONTAL_POSE_FLAGS: [ActorFlag; 4] = [
    ActorFlag::Gliding,
    ActorFlag::DamageNearbyMobs,
    ActorFlag::Swimming,
    ActorFlag::Crawling,
];

/// Uses the unscaled width as the height for every horizontal player pose.
/// Completed walking size applies to the standard player model; other upright
/// models retain their supplied dimensions.
fn predicted_bounding_box(
    metadata: &HashMap<u32, ActorMetadataValue>,
    walking_height: Option<f32>,
    flags: [u64; 3],
) -> [f32; 3] {
    let mut size = bounding_box(metadata);
    if HORIZONTAL_POSE_FLAGS
        .into_iter()
        .any(|flag| flag_set(&flags, flag))
    {
        size[2] = size[1];
        return size;
    }
    let width = sim::PLAYER_WIDTH as f32;
    let pose_heights = [
        sim::PLAYER_HEIGHT as f32,
        sim::MovementMode::Walking.hitbox_height(true) as f32,
        width,
    ];
    if size[1] != width || !pose_heights.contains(&size[2]) {
        return size;
    }
    if let Some(height) = walking_height {
        size[2] = height;
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
