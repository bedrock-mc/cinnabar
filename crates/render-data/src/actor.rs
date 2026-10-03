//! Dependency-free actor observations shared by transport and animation.
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorKind {
    Player { uuid: [u8; 16], username: Arc<str> },
    Entity { identifier: Arc<str> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActorMetadataValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Float(f32),
    String(Arc<str>),
    Compound(Arc<[u8]>),
    BlockPosition([i32; 3]),
    Long(i64),
    Vector([f32; 3]),
    Flags(u64),
    FlagsExtended(u64),
}

/// How a property's stored number reads: enums store the index of their value name.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyKind {
    Number,
    Enum(Arc<[Arc<str>]>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropertyDefinition {
    pub name: Arc<str>,
    pub kind: PropertyKind,
    /// Stored number an actor reads before the server sets it (enums: the value index).
    pub default: f32,
}

/// Reads the primary or overflow actor flag word without transport dependencies.
pub fn actor_flag(metadata: &HashMap<u32, ActorMetadataValue>, bit: u32) -> bool {
    let (key, bit) = if bit < 64 { (0, bit) } else { (92, bit - 64) };
    match metadata.get(&key) {
        Some(ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags)) => {
            flags & (1_u64 << bit) != 0
        }
        _ => false,
    }
}

/// A player's metadata sleeping flag or the generic sleeping actor flag.
pub fn player_is_sleeping(metadata: &HashMap<u32, ActorMetadataValue>) -> bool {
    let player_flags = metadata.get(&26).is_some_and(
        |value| matches!(value, ActorMetadataValue::Byte(flags) if (*flags as u8) & (1 << 1) != 0),
    );
    player_flags || actor_flag(metadata, 76)
}

/// Server render scale; absent, non-finite or non-positive scales read 1.
pub fn actor_render_scale(metadata: &HashMap<u32, ActorMetadataValue>) -> f32 {
    match metadata.get(&38) {
        Some(ActorMetadataValue::Float(scale)) if scale.is_finite() && *scale > 0.0 => *scale,
        _ => 1.0,
    }
}

/// Actors whose target-rotation query animates their entire streamed yaw.
pub fn target_rotation_is_absolute(kind: &ActorKind) -> bool {
    matches!(kind, ActorKind::Entity { identifier } if matches!(identifier.as_ref(),
        "minecraft:arrow" | "minecraft:fireworks_rocket" | "minecraft:wither_skull" | "minecraft:wither_skull_dangerous"))
}

/// Entity rigs whose authored billboard bones already carry the camera rotation.
/// Shared by native and browser placement, avoiding a second actor yaw.
pub fn actor_is_billboard(kind: &ActorKind) -> bool {
    matches!(kind, ActorKind::Entity { identifier } if matches!(identifier.as_ref(),
        "minecraft:xp_bottle" | "minecraft:ender_pearl" | "minecraft:xp_orb"
        | "minecraft:dragon_fireball" | "minecraft:fireball" | "minecraft:snowball"
        | "minecraft:small_fireball" | "minecraft:splash_potion" | "minecraft:egg"
        | "minecraft:eye_of_ender_signal" | "minecraft:lingering_potion"))
}

/// Movement flags retained from Bedrock's primary actor flag word.
pub const ACTOR_FLAG_SNEAKING: u32 = 1;
pub const ACTOR_FLAG_SPRINTING: u32 = 3;
pub const ACTOR_FLAG_USING_ITEM: u32 = 4;
pub const ACTOR_FLAG_SWIMMING: u32 = 57;
