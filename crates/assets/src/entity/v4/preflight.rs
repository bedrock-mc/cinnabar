use serde::{Deserialize, Deserializer, de};

use crate::item::{MAX_ITEM_VISUAL_ALIASES, MAX_ITEM_VISUALS};

use super::super::{
    MAX_ENTITY_ASSET_SOURCES, MAX_ENTITY_ASSET_SYMBOLS, MAX_ENTITY_DEPENDENCIES,
    MAX_ENTITY_GEOMETRIES, MAX_ENTITY_GEOMETRY_BONES, MAX_ENTITY_GEOMETRY_CUBES,
};
use super::{
    MAX_ENTITY_ANIMATION_CHANNELS, MAX_ENTITY_ANIMATION_CLIPS, MAX_ENTITY_ANIMATION_KEYFRAMES,
    MAX_ENTITY_CONTROLLER_ANIMATIONS, MAX_ENTITY_CONTROLLER_STATES,
    MAX_ENTITY_CONTROLLER_TRANSITIONS, MAX_ENTITY_CONTROLLERS, MAX_ENTITY_RIG_ANIMATIONS,
    MAX_ENTITY_RIG_BINDINGS, MAX_ENTITY_RIG_CONTROLLERS, MAX_ENTITY_RIG_GEOMETRIES,
    MAX_ENTITY_RIG_TEXTURES, MAX_MOLANG_COLLECTION_ITEMS_TOTAL, MAX_MOLANG_COLLECTIONS,
    MAX_MOLANG_EXPRESSIONS, MAX_MOLANG_OPS,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityCatalogCountProbe {
    #[serde(rename = "block_visual_count")]
    _block_visual_count: de::IgnoredAny,
    sources: SequenceCount,
    symbols: SymbolSequenceCount,
    geometries: GeometrySequenceCount,
    animation_clips: SequenceCount,
    animation_channels: SequenceCount,
    animation_keyframes: SequenceCount,
    molang_symbols: SequenceCount,
    molang_expressions: SequenceCount,
    molang_ops: SequenceCount,
    molang_collections: SequenceCount,
    molang_collection_items: SequenceCount,
    controllers: SequenceCount,
    controller_states: SequenceCount,
    controller_animations: SequenceCount,
    controller_transitions: SequenceCount,
    rig_bindings: SequenceCount,
    rig_geometries: SequenceCount,
    rig_animations: SequenceCount,
    rig_controllers: SequenceCount,
    rig_textures: SequenceCount,
    item_visuals: SequenceCount,
    item_visual_aliases: SequenceCount,
}

struct SequenceCount(usize);

impl<'de> Deserialize<'de> for SequenceCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = SequenceCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity carrier array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut count = 0usize;
                while sequence.next_element::<de::IgnoredAny>()?.is_some() {
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("entity carrier array count overflow"))?;
                }
                Ok(SequenceCount(count))
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct SymbolProbe {
    kind: de::IgnoredAny,
    identifier: de::IgnoredAny,
    source_index: de::IgnoredAny,
    dependencies: SequenceCount,
}

struct SymbolSequenceCount(usize);

impl<'de> Deserialize<'de> for SymbolSequenceCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = SymbolSequenceCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity symbol array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut count = 0usize;
                while let Some(symbol) = sequence.next_element::<SymbolProbe>()? {
                    let SymbolProbe {
                        kind: _,
                        identifier: _,
                        source_index: _,
                        dependencies,
                    } = symbol;
                    if dependencies.0 > MAX_ENTITY_DEPENDENCIES {
                        return Err(de::Error::custom(
                            "entity dependency count preflight exceeds bound",
                        ));
                    }
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("entity symbol count overflow"))?;
                }
                Ok(SymbolSequenceCount(count))
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct GeometryProbe {
    identifier: de::IgnoredAny,
    inherits: de::IgnoredAny,
    source_index: de::IgnoredAny,
    texture_width: de::IgnoredAny,
    texture_height: de::IgnoredAny,
    bones: BoneSequenceCount,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct BoneProbe {
    name: de::IgnoredAny,
    parent: de::IgnoredAny,
    pivot: de::IgnoredAny,
    rotation: de::IgnoredAny,
    mirror: de::IgnoredAny,
    inflate: de::IgnoredAny,
    never_render: de::IgnoredAny,
    reset: de::IgnoredAny,
    cubes: SequenceCount,
}

struct BoneSequenceCount;

impl<'de> Deserialize<'de> for BoneSequenceCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = BoneSequenceCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity geometry bone array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut bones = 0usize;
                let mut cubes = 0usize;
                while let Some(bone) = sequence.next_element::<BoneProbe>()? {
                    let BoneProbe {
                        name: _,
                        parent: _,
                        pivot: _,
                        rotation: _,
                        mirror: _,
                        inflate: _,
                        never_render: _,
                        reset: _,
                        cubes: bone_cubes,
                    } = bone;
                    bones = bones
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("entity bone count overflow"))?;
                    cubes = cubes
                        .checked_add(bone_cubes.0)
                        .ok_or_else(|| de::Error::custom("entity cube count overflow"))?;
                    if bones > MAX_ENTITY_GEOMETRY_BONES || cubes > MAX_ENTITY_GEOMETRY_CUBES {
                        return Err(de::Error::custom(
                            "entity geometry subarray count preflight exceeds bound",
                        ));
                    }
                }
                Ok(BoneSequenceCount)
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

struct GeometrySequenceCount(usize);

impl<'de> Deserialize<'de> for GeometrySequenceCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = GeometrySequenceCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity geometry array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut count = 0usize;
                while let Some(geometry) = sequence.next_element::<GeometryProbe>()? {
                    let GeometryProbe {
                        identifier: _,
                        inherits: _,
                        source_index: _,
                        texture_width: _,
                        texture_height: _,
                        bones: _,
                    } = geometry;
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("entity geometry count overflow"))?;
                }
                Ok(GeometrySequenceCount(count))
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

pub(crate) fn payload_counts(bytes: &[u8]) -> Result<[usize; 7], serde_json::Error> {
    let counts: EntityCatalogCountProbe = serde_json::from_slice(bytes)?;
    let bounded = [
        (counts.sources.0, MAX_ENTITY_ASSET_SOURCES),
        (counts.symbols.0, MAX_ENTITY_ASSET_SYMBOLS),
        (counts.geometries.0, MAX_ENTITY_GEOMETRIES),
        (counts.animation_clips.0, MAX_ENTITY_ANIMATION_CLIPS),
        (counts.animation_channels.0, MAX_ENTITY_ANIMATION_CHANNELS),
        (counts.animation_keyframes.0, MAX_ENTITY_ANIMATION_KEYFRAMES),
        (counts.molang_symbols.0, MAX_MOLANG_EXPRESSIONS),
        (counts.molang_expressions.0, MAX_MOLANG_EXPRESSIONS),
        (counts.molang_ops.0, MAX_MOLANG_OPS),
        (counts.molang_collections.0, MAX_MOLANG_COLLECTIONS),
        (
            counts.molang_collection_items.0,
            MAX_MOLANG_COLLECTION_ITEMS_TOTAL,
        ),
        (counts.controllers.0, MAX_ENTITY_CONTROLLERS),
        (counts.controller_states.0, MAX_ENTITY_CONTROLLER_STATES),
        (
            counts.controller_animations.0,
            MAX_ENTITY_CONTROLLER_ANIMATIONS,
        ),
        (
            counts.controller_transitions.0,
            MAX_ENTITY_CONTROLLER_TRANSITIONS,
        ),
        (counts.rig_bindings.0, MAX_ENTITY_RIG_BINDINGS),
        (counts.rig_geometries.0, MAX_ENTITY_RIG_GEOMETRIES),
        (counts.rig_animations.0, MAX_ENTITY_RIG_ANIMATIONS),
        (counts.rig_controllers.0, MAX_ENTITY_RIG_CONTROLLERS),
        (counts.rig_textures.0, MAX_ENTITY_RIG_TEXTURES),
        (counts.item_visuals.0, MAX_ITEM_VISUALS),
        (counts.item_visual_aliases.0, MAX_ITEM_VISUAL_ALIASES),
    ];
    if bounded.iter().any(|(count, maximum)| count > maximum) {
        return Err(<serde_json::Error as de::Error>::custom(
            "entity carrier count preflight exceeds a retained-section bound",
        ));
    }
    Ok([
        counts.sources.0,
        counts.symbols.0,
        counts.geometries.0,
        counts.animation_clips.0,
        counts.controllers.0,
        counts.rig_bindings.0,
        counts.item_visuals.0,
    ])
}
