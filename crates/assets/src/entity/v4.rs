use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    AssetError,
    item::{
        ItemVisualAlias, ItemVisualDefinition, ItemVisualDefinitionRoute, validate_item_visuals,
    },
};

use super::{
    CompiledEntityAssets, EntityAssetKind, EntityAssetSymbol, EntityGeometryScalar,
    RuntimeEntityAssets, invalid, validate_compiled, validate_geometry_scalar, validate_scalars,
};

/// Clips are geometry-specific instances and share the retained rig animation instance budget.
pub const MAX_ENTITY_ANIMATION_CLIPS: usize = MAX_ENTITY_RIG_ANIMATIONS;
/// One channel per animated bone property of each clip; merged server packs reach six figures.
pub const MAX_ENTITY_ANIMATION_CHANNELS: usize = 262_144;
pub const MAX_ENTITY_ANIMATION_KEYFRAMES: usize = 524_288;
pub const MAX_ENTITY_CONTROLLERS: usize = 2_048;
pub const MAX_ENTITY_CONTROLLER_STATES: usize = 16_384;
pub const MAX_ENTITY_CONTROLLER_TRANSITIONS: usize = 32_768;
pub const MAX_ENTITY_CONTROLLER_ANIMATIONS: usize = 524_288;
pub const MAX_MOLANG_EXPRESSIONS: usize = 65_536;
pub const MAX_MOLANG_OPS_PER_EXPRESSION: usize = 2_048;
pub const MAX_MOLANG_OPS: usize = 1_048_576;
pub const MAX_MOLANG_STACK_DEPTH: u8 = 32;
pub const MAX_MOLANG_COLLECTION_ITEMS: usize = 32;
pub const MAX_MOLANG_COLLECTIONS: usize = 16_384;
pub const MAX_MOLANG_COLLECTION_ITEMS_TOTAL: usize =
    MAX_MOLANG_COLLECTIONS * MAX_MOLANG_COLLECTION_ITEMS;
pub const MAX_ENTITY_RIG_BINDINGS: usize = 8_192;
pub const MAX_ENTITY_RIG_GEOMETRIES: usize = 262_144;
pub const MAX_ENTITY_RIG_ANIMATIONS: usize = 262_144;
pub const MAX_ENTITY_RIG_CONTROLLERS: usize = 262_144;

#[path = "v4/preflight.rs"]
mod preflight;
pub(super) use preflight::payload_counts;
#[path = "v4/encode.rs"]
mod encode;
pub(super) use encode::{encode_compiled, encode_runtime};
#[path = "v4/render.rs"]
mod render;
use render::validate_render_payload;
pub use render::{
    ENTITY_ALPHA_TEST_THRESHOLD, EntityRenderCandidate, EntityRenderData, EntityRenderGeometry,
    EntityRenderLayer, EntityRenderMaterial, EntityRenderMaterialState, EntityRenderSlot,
    EntityRenderVisibility, MAX_ENTITY_RENDER_CANDIDATES, MAX_ENTITY_RENDER_LAYERS,
    MAX_ENTITY_RENDER_PATTERN_BYTES, MAX_ENTITY_RENDER_SLOTS, MAX_ENTITY_RENDER_VISIBILITY,
    entity_render_pattern_matches,
};
#[path = "v4/rig.rs"]
mod rig;
use rig::{validate_controller_nesting, validate_rig_payload};
#[path = "v4/molang.rs"]
mod molang;
#[path = "v4/molang_math.rs"]
mod molang_math;
pub use molang::{
    MAX_MOLANG_LOOP_DEPTH, MAX_MOLANG_LOOP_ITERATIONS, MAX_MOLANG_QUERY_ARGUMENTS,
    MAX_MOLANG_STRING_BYTES, MOLANG_QUERIES, MolangBranch, MolangCall, MolangEaseCurve,
    MolangEaseMode, MolangFunction, MolangOp, molang_call, molang_program_stack,
};
use molang::{molang_symbol_has_kind, validate_molang_payload};

/// Deepest controller-in-controller chain a rig may reference.
pub const MAX_ENTITY_CONTROLLER_NESTING: usize = 4;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum EntityAnimationLoop {
    Once = 0,
    Loop = 1,
    HoldOnLastFrame = 2,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum EntityAnimationProperty {
    Translation = 0,
    Rotation = 1,
    Scale = 2,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum EntityAnimationInterpolation {
    Linear = 0,
    Step = 1,
    CatmullRom = 2,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAnimationClip {
    pub symbol: u32,
    pub length_seconds: EntityGeometryScalar,
    pub loop_mode: EntityAnimationLoop,
    pub first_channel: u32,
    pub channel_count: u32,
    pub source: u32,
    /// Channels replace, rather than add to, what earlier animations produced.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub override_previous: bool,
    /// Geometry whose bones the channels index; clips of one symbol are ordered by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<u32>,
    /// Molang expression that supplies the clip's animation time instead of elapsed time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim_time_update: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAnimationChannel {
    pub bone: u32,
    /// Name-bound channels can drive a model selected after pack compilation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone_name: Option<Box<str>>,
    pub property: EntityAnimationProperty,
    pub first_keyframe: u32,
    pub keyframe_count: u32,
    /// This bone uses the entity's axes after its pivot follows the parent transform.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rotation_relative_to_entity: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAnimationKeyframe {
    pub time_seconds: EntityGeometryScalar,
    pub value: [EntityGeometryScalar; 3],
    pub interpolation: EntityAnimationInterpolation,
    /// Per-axis Molang expression evaluated per tick in place of `value`.
    #[serde(default, skip_serializing_if = "constant_axes")]
    pub expressions: [Option<u32>; 3],
}

fn constant_axes(expressions: &[Option<u32>; 3]) -> bool {
    expressions.iter().all(Option::is_none)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledMolangExpression {
    pub first_op: u32,
    pub op_count: u16,
    pub max_stack: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum MolangSymbolKind {
    Name = 0,
    Query = 1,
    Variable = 2,
    Temporary = 3,
    /// A string literal, which may be empty.
    String = 4,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MolangSymbol {
    pub kind: MolangSymbolKind,
    pub identifier: Box<str>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MolangCollection {
    pub first_item: u32,
    pub item_count: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MolangCollectionItem {
    pub value: EntityGeometryScalar,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAnimationController {
    pub symbol: u32,
    pub first_state: u32,
    pub state_count: u16,
    pub initial_state: u16,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityControllerState {
    /// Seconds to blend out this state when transitioning to another state.
    #[serde(default, skip_serializing_if = "is_default_blend")]
    pub blend_transition: EntityGeometryScalar,
    /// Chooses the shorter rotation arc when blending controller states.
    #[serde(default, skip_serializing_if = "is_default_blend")]
    pub blend_via_shortest_path: bool,
    pub name: u32,
    pub first_animation: u32,
    pub animation_count: u16,
    pub first_transition: u32,
    pub transition_count: u16,
    pub on_entry: Option<u32>,
    pub on_exit: Option<u32>,
}

/// Omits legacy-compatible controller blending defaults from encoded payloads.
fn is_default_blend<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityControllerAnimation {
    pub target: EntityControllerAnimationTarget,
    pub weight: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "index")]
pub enum EntityControllerAnimationTarget {
    Clip(u32),
    Controller(u32),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityControllerTransition {
    pub target_state: u16,
    pub condition: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum EntityRigFallback {
    Skip = 0,
    GeometryOnly = 1,
    Diagnostic = 2,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRigBinding {
    pub entity_symbol: u32,
    pub render_controller: u32,
    pub first_geometry: u32,
    pub geometry_count: u16,
    pub fallback: EntityRigFallback,
    pub initialize: Option<u32>,
    pub pre_animation: Option<u32>,
    pub scale: EntityGeometryScalar,
    /// `scale`, `scaleX`, `scaleY` and `scaleZ` expressions when any is authored as Molang or
    /// per axis; `scale` then holds only the constant fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_expressions: Option<[u32; 4]>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRigGeometryBinding {
    pub geometry: u32,
    pub condition: Option<u32>,
    pub first_animation: u32,
    pub animation_count: u16,
    pub first_controller: u32,
    pub controller_count: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRigAnimationBinding {
    pub name: u32,
    pub clip: u32,
    pub weight: Option<u32>,
    /// Position in the entity's authored `animate` list, shared with controller bindings.
    #[serde(default, skip_serializing_if = "is_zero_order")]
    pub order: u16,
}

fn is_zero_order(order: &u16) -> bool {
    *order == 0
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRigControllerBinding {
    pub name: u32,
    pub controller: u32,
    pub weight: Option<u32>,
    #[serde(default, skip_serializing_if = "is_zero_order")]
    pub order: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityAssetSummary {
    pub sources: usize,
    pub symbols: usize,
    pub geometries: usize,
    pub animation_clips: usize,
    pub animation_channels: usize,
    pub animation_keyframes: usize,
    pub controllers: usize,
    pub controller_states: usize,
    pub controller_animations: usize,
    pub controller_transitions: usize,
    pub molang_symbols: usize,
    pub molang_expressions: usize,
    pub molang_ops: usize,
    pub molang_collections: usize,
    pub molang_collection_items: usize,
    pub rig_bindings: usize,
    pub rig_geometries: usize,
    pub rig_animations: usize,
    pub rig_controllers: usize,
    pub item_visuals: usize,
    pub item_visual_aliases: usize,
    pub block_visuals: usize,
}

impl CompiledEntityAssets {
    pub fn validate(&self) -> Result<(), AssetError> {
        validate_compiled(self).map(|_| ())
    }
}

impl RuntimeEntityAssets {
    #[must_use]
    pub fn animation_clips(&self) -> &[EntityAnimationClip] {
        &self.animation_clips
    }

    /// The clip of animation `symbol` compiled against `geometry`'s bones.
    #[must_use]
    pub fn clip_for_geometry(&self, symbol: u32, geometry: u32) -> Option<u32> {
        let first = self
            .animation_clips
            .partition_point(|clip| clip.symbol < symbol);
        self.animation_clips[first..]
            .iter()
            .take_while(|clip| clip.symbol == symbol)
            .position(|clip| clip.geometry == Some(geometry))
            .or_else(|| {
                self.animation_clips[first..]
                    .iter()
                    .take_while(|clip| clip.symbol == symbol)
                    .position(|clip| clip.geometry.is_none())
            })
            .map(|offset| (first + offset) as u32)
    }

    #[must_use]
    pub fn animation_channels(&self) -> &[EntityAnimationChannel] {
        &self.animation_channels
    }

    #[must_use]
    pub fn animation_keyframes(&self) -> &[EntityAnimationKeyframe] {
        &self.animation_keyframes
    }

    #[must_use]
    pub fn molang_symbols(&self) -> &[MolangSymbol] {
        &self.molang_symbols
    }

    #[must_use]
    pub fn molang_expressions(&self) -> &[CompiledMolangExpression] {
        &self.molang_expressions
    }

    #[must_use]
    pub fn molang_ops(&self) -> &[MolangOp] {
        &self.molang_ops
    }

    #[must_use]
    pub fn molang_collections(&self) -> &[MolangCollection] {
        &self.molang_collections
    }

    #[must_use]
    pub fn molang_collection_items(&self) -> &[MolangCollectionItem] {
        &self.molang_collection_items
    }

    #[must_use]
    pub fn controllers(&self) -> &[EntityAnimationController] {
        &self.controllers
    }

    #[must_use]
    pub fn controller_states(&self) -> &[EntityControllerState] {
        &self.controller_states
    }

    #[must_use]
    pub fn controller_animations(&self) -> &[EntityControllerAnimation] {
        &self.controller_animations
    }

    #[must_use]
    pub fn controller_transitions(&self) -> &[EntityControllerTransition] {
        &self.controller_transitions
    }

    #[must_use]
    pub fn rig_bindings(&self) -> &[EntityRigBinding] {
        &self.rig_bindings
    }

    /// The unambiguous animated attachable rig with this authored identifier.
    #[must_use]
    pub fn attachable_rig_binding(&self, identifier: &str) -> Option<usize> {
        let mut matches = self.rig_bindings.iter().enumerate().filter(|(_, rig)| {
            self.symbols
                .get(rig.entity_symbol as usize)
                .is_some_and(|symbol| {
                    symbol.kind == EntityAssetKind::Attachable
                        && symbol.identifier.as_ref() == identifier
                })
        });
        let (index, _) = matches.next()?;
        matches.next().is_none().then_some(index)
    }

    #[must_use]
    pub fn rig_geometries(&self) -> &[EntityRigGeometryBinding] {
        &self.rig_geometries
    }

    #[must_use]
    pub fn rig_animations(&self) -> &[EntityRigAnimationBinding] {
        &self.rig_animations
    }

    #[must_use]
    pub fn rig_controllers(&self) -> &[EntityRigControllerBinding] {
        &self.rig_controllers
    }

    #[must_use]
    pub fn render_data(&self) -> &EntityRenderData {
        &self.render
    }

    /// Render layers of one rig binding, in authored controller order.
    #[must_use]
    pub fn render_layers(&self, rig_binding: usize) -> &[EntityRenderLayer] {
        let layers = &self.render.layers;
        let Ok(rig) = u32::try_from(rig_binding) else {
            return &[];
        };
        let start = layers.partition_point(|layer| layer.rig < rig);
        let length = layers[start..].partition_point(|layer| layer.rig == rig);
        &layers[start..start + length]
    }

    #[must_use]
    pub fn item_visuals(&self) -> &[ItemVisualDefinition] {
        &self.item_visuals
    }

    #[must_use]
    pub fn item_visual_aliases(&self) -> &[ItemVisualAlias] {
        &self.item_visual_aliases
    }

    #[must_use]
    pub fn summary(&self) -> EntityAssetSummary {
        EntityAssetSummary {
            sources: self.sources.len(),
            symbols: self.symbols.len(),
            geometries: self.geometries.len(),
            animation_clips: self.animation_clips.len(),
            animation_channels: self.animation_channels.len(),
            animation_keyframes: self.animation_keyframes.len(),
            controllers: self.controllers.len(),
            controller_states: self.controller_states.len(),
            controller_animations: self.controller_animations.len(),
            controller_transitions: self.controller_transitions.len(),
            molang_symbols: self.molang_symbols.len(),
            molang_expressions: self.molang_expressions.len(),
            molang_ops: self.molang_ops.len(),
            molang_collections: self.molang_collections.len(),
            molang_collection_items: self.molang_collection_items.len(),
            rig_bindings: self.rig_bindings.len(),
            rig_geometries: self.rig_geometries.len(),
            rig_animations: self.rig_animations.len(),
            rig_controllers: self.rig_controllers.len(),
            item_visuals: self.item_visuals.len(),
            item_visual_aliases: self.item_visual_aliases.len(),
            block_visuals: self.block_visual_count as usize,
        }
    }

    pub fn encode(&self) -> Result<Box<[u8]>, AssetError> {
        encode_runtime(self)
    }
}

pub(super) fn validate_extended_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    validate_animation_payload(compiled)?;
    validate_molang_payload(compiled)?;
    validate_controller_payload(compiled)?;
    validate_rig_payload(compiled)?;
    validate_render_payload(compiled)?;
    validate_item_visuals(
        &compiled.item_visuals,
        &compiled.item_visual_aliases,
        compiled.sources.len(),
        compiled.block_visual_count as usize,
    )?;
    let reviewed_bindings: DefaultSpriteBindings = serde_json::from_slice(DEFAULT_SPRITE_BINDINGS)
        .map_err(|_| invalid("embedded default sprite bindings are invalid"))?;
    let binding_hash: [u8; 32] = Sha256::digest(DEFAULT_SPRITE_BINDINGS).into();
    let legacy_hash: [u8; 32] = Sha256::digest(LEGACY_ICON_ROUTES).into();
    for source in &compiled.sources {
        if source.path.as_ref() == DEFAULT_SPRITE_BINDINGS_PATH
            && (source.source_bytes as usize != DEFAULT_SPRITE_BINDINGS.len()
                || source.source_sha256 != binding_hash)
        {
            return Err(invalid("default sprite defining source identity mismatch"));
        }
        if source.path.as_ref() == LEGACY_ICON_ROUTES_PATH
            && (source.source_bytes as usize != LEGACY_ICON_ROUTES.len()
                || source.source_sha256 != legacy_hash)
        {
            return Err(invalid("legacy icon defining source identity mismatch"));
        }
    }
    for visual in &compiled.item_visuals {
        let defining_path = &compiled.sources[visual.source as usize].path;
        if defining_path.as_ref() == DEFAULT_SPRITE_BINDINGS_PATH {
            if visual.key.metadata != 0
                || !reviewed_bindings
                    .routes
                    .iter()
                    .any(|binding| binding.identifier == visual.key.identifier)
                || !matches!(
                    visual.route,
                    ItemVisualDefinitionRoute::Missing
                        | ItemVisualDefinitionRoute::Sprite {
                            texture: crate::item::ItemTextureReference { variant: 0, .. }
                        }
                )
            {
                return Err(invalid(
                    "item visual is outside reviewed default sprite bindings",
                ));
            }
        } else if defining_path.as_ref() == LEGACY_ICON_ROUTES_PATH {
            if !legacy_icon_route_listed(&visual.key.identifier, visual.key.metadata)
                || !matches!(
                    visual.route,
                    ItemVisualDefinitionRoute::Missing | ItemVisualDefinitionRoute::Sprite { .. }
                )
            {
                return Err(invalid(
                    "item visual is outside reviewed legacy icon routes",
                ));
            }
        } else if !valid_item_definition_source(defining_path) {
            return Err(invalid("item visual defining source is not reviewed"));
        }
        if let ItemVisualDefinitionRoute::Sprite { texture } = visual.route {
            let texture_path = &compiled.sources[texture.source as usize].path;
            if !valid_item_raster_source(texture_path) {
                return Err(invalid("item sprite source is not a reviewed raster"));
            }
        }
    }
    Ok(())
}

const LEGACY_ICON_ROUTES_PATH: &str = "registry/legacy-icon-routes-26.30.tsv";
const LEGACY_ICON_ROUTES: &[u8] = include_bytes!("../../data/legacy-icon-routes-26.30.tsv");

/// Whether the embedded legacy table lists `(identifier, metadata)`.
fn legacy_icon_route_listed(identifier: &str, metadata: u32) -> bool {
    std::str::from_utf8(LEGACY_ICON_ROUTES)
        .into_iter()
        .flat_map(str::lines)
        .filter_map(|line| {
            let mut columns = line.split('\t');
            Some((columns.next()?, columns.next()?.parse::<u32>().ok()?))
        })
        .any(|row| row == (identifier, metadata))
}

const DEFAULT_SPRITE_BINDINGS_PATH: &str = "registry/default-sprite-bindings-1.26.50.json";
const DEFAULT_SPRITE_BINDINGS: &[u8] =
    include_bytes!("../../data/default-sprite-bindings-1.26.50.json");

#[derive(Deserialize)]
struct DefaultSpriteBindings {
    routes: Vec<DefaultSpriteBinding>,
}

#[derive(Deserialize)]
struct DefaultSpriteBinding {
    identifier: Box<str>,
}

fn valid_item_definition_source(path: &str) -> bool {
    (path.starts_with("entity/") && path.ends_with(".json"))
        || path == "textures/item_texture.json"
        || path == "registry/block-item-routes-v2193.json"
}

fn valid_item_raster_source(path: &str) -> bool {
    (path.starts_with("textures/items/") || path.starts_with("textures/entity/"))
        && (path.ends_with(".png") || path.ends_with(".tga"))
}

fn validate_animation_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.animation_clips.len() > MAX_ENTITY_ANIMATION_CLIPS
        || compiled.animation_channels.len() > MAX_ENTITY_ANIMATION_CHANNELS
        || compiled.animation_keyframes.len() > MAX_ENTITY_ANIMATION_KEYFRAMES
    {
        return Err(invalid("entity animation payload count exceeds bound"));
    }
    for clip in &compiled.animation_clips {
        validate_geometry_scalar(clip.length_seconds)?;
        if clip.length_seconds.get() < 0.0
            || !index_has_kind(&compiled.symbols, clip.symbol, EntityAssetKind::Animation)
            || clip.source as usize >= compiled.sources.len()
            || compiled.symbols[clip.symbol as usize].source_index != clip.source
            || clip
                .geometry
                .is_some_and(|geometry| geometry as usize >= compiled.geometries.len())
            || clip
                .anim_time_update
                .is_some_and(|expression| expression as usize >= compiled.molang_expressions.len())
            || !range_in_bounds(
                clip.first_channel,
                clip.channel_count,
                compiled.animation_channels.len(),
            )
        {
            return Err(invalid("invalid entity animation clip index or scalar"));
        }
    }
    for channel in &compiled.animation_channels {
        if let Some(name) = &channel.bone_name {
            super::validate_geometry_name(name)?;
        }
        if !range_in_bounds(
            channel.first_keyframe,
            channel.keyframe_count,
            compiled.animation_keyframes.len(),
        ) {
            return Err(invalid("entity animation channel index is out of range"));
        }
    }
    for keyframe in &compiled.animation_keyframes {
        validate_geometry_scalar(keyframe.time_seconds)?;
        validate_scalars(&keyframe.value)?;
        if keyframe.time_seconds.get() < 0.0
            || keyframe
                .expressions
                .iter()
                .flatten()
                .any(|index| *index as usize >= compiled.molang_expressions.len())
        {
            return Err(invalid("invalid entity animation keyframe"));
        }
    }
    validate_flattened_ranges(
        compiled
            .animation_clips
            .iter()
            .map(|clip| (clip.first_channel, clip.channel_count)),
        compiled.animation_channels.len(),
        "animation channel",
    )?;
    validate_flattened_ranges(
        compiled
            .animation_channels
            .iter()
            .map(|channel| (channel.first_keyframe, channel.keyframe_count)),
        compiled.animation_keyframes.len(),
        "animation keyframe",
    )?;
    Ok(())
}

fn validate_controller_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.controllers.len() > MAX_ENTITY_CONTROLLERS
        || compiled.controller_states.len() > MAX_ENTITY_CONTROLLER_STATES
        || compiled.controller_animations.len() > MAX_ENTITY_CONTROLLER_ANIMATIONS
        || compiled.controller_transitions.len() > MAX_ENTITY_CONTROLLER_TRANSITIONS
    {
        return Err(invalid("entity controller payload count exceeds bound"));
    }
    for controller in &compiled.controllers {
        if controller.state_count == 0
            || controller.initial_state >= controller.state_count
            || !index_has_kind(
                &compiled.symbols,
                controller.symbol,
                EntityAssetKind::AnimationController,
            )
            || !range_in_bounds(
                controller.first_state,
                u32::from(controller.state_count),
                compiled.controller_states.len(),
            )
        {
            return Err(invalid("invalid entity animation controller"));
        }
        let states = &compiled.controller_states[controller.first_state as usize
            ..controller.first_state as usize + controller.state_count as usize];
        for state in states {
            validate_controller_state(compiled, state, controller.state_count)?;
        }
    }
    validate_flattened_ranges(
        compiled
            .controllers
            .iter()
            .map(|controller| (controller.first_state, u32::from(controller.state_count))),
        compiled.controller_states.len(),
        "controller state",
    )?;
    validate_flattened_ranges(
        compiled
            .controller_states
            .iter()
            .map(|state| (state.first_animation, u32::from(state.animation_count))),
        compiled.controller_animations.len(),
        "controller animation",
    )?;
    validate_flattened_ranges(
        compiled
            .controller_states
            .iter()
            .map(|state| (state.first_transition, u32::from(state.transition_count))),
        compiled.controller_transitions.len(),
        "controller transition",
    )?;
    validate_controller_nesting(compiled)
}

fn validate_controller_state(
    compiled: &CompiledEntityAssets,
    state: &EntityControllerState,
    controller_state_count: u16,
) -> Result<(), AssetError> {
    validate_geometry_scalar(state.blend_transition)?;
    if state.blend_transition.get() < 0.0 {
        return Err(invalid("entity controller blend duration is negative"));
    }
    if !molang_symbol_has_kind(compiled, state.name, &[MolangSymbolKind::Name])
        || !range_in_bounds(
            state.first_animation,
            u32::from(state.animation_count),
            compiled.controller_animations.len(),
        )
        || !range_in_bounds(
            state.first_transition,
            u32::from(state.transition_count),
            compiled.controller_transitions.len(),
        )
        || state
            .on_entry
            .is_some_and(|index| index as usize >= compiled.molang_expressions.len())
        || state
            .on_exit
            .is_some_and(|index| index as usize >= compiled.molang_expressions.len())
    {
        return Err(invalid("entity controller state index is out of range"));
    }
    let animations = &compiled.controller_animations[state.first_animation as usize
        ..state.first_animation as usize + state.animation_count as usize];
    for animation in animations {
        let target_valid = match animation.target {
            EntityControllerAnimationTarget::Clip(clip) => {
                (clip as usize) < compiled.animation_clips.len()
            }
            EntityControllerAnimationTarget::Controller(controller) => {
                (controller as usize) < compiled.controllers.len()
            }
        };
        if !target_valid
            || animation
                .weight
                .is_some_and(|index| index as usize >= compiled.molang_expressions.len())
        {
            return Err(invalid("entity controller animation index is out of range"));
        }
    }
    let transitions = &compiled.controller_transitions[state.first_transition as usize
        ..state.first_transition as usize + state.transition_count as usize];
    for transition in transitions {
        if transition.target_state >= controller_state_count
            || transition.condition as usize >= compiled.molang_expressions.len()
        {
            return Err(invalid(
                "entity controller transition index is out of range",
            ));
        }
    }
    Ok(())
}

fn validate_flattened_ranges(
    ranges: impl IntoIterator<Item = (u32, u32)>,
    total: usize,
    section: &str,
) -> Result<(), AssetError> {
    let mut next = 0usize;
    for (first, count) in ranges {
        let first = first as usize;
        let count = count as usize;
        if first != next {
            return Err(invalid(format!(
                "noncanonical {section} ranges overlap or leave an orphan gap"
            )));
        }
        next = next
            .checked_add(count)
            .ok_or_else(|| invalid(format!("{section} range overflows")))?;
        if next > total {
            return Err(invalid(format!("{section} range is out of bounds")));
        }
    }
    if next != total {
        return Err(invalid(format!("orphan {section} tail")));
    }
    Ok(())
}

fn index_has_kind(symbols: &[EntityAssetSymbol], index: u32, kind: EntityAssetKind) -> bool {
    symbols
        .get(index as usize)
        .is_some_and(|symbol| symbol.kind == kind)
}

fn range_in_bounds(first: u32, count: u32, length: usize) -> bool {
    usize::try_from(first)
        .ok()
        .and_then(|first| {
            usize::try_from(count)
                .ok()
                .and_then(|count| first.checked_add(count))
        })
        .is_some_and(|end| end <= length)
}
