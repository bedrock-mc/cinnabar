use serde::{Deserialize, Serialize};

use crate::{
    AssetError,
    item::{
        ItemVisualAlias, ItemVisualDefinition, ItemVisualDefinitionRoute, validate_item_visuals,
    },
};

use super::{
    CompiledEntityAssets, EntityAssetKind, EntityAssetSymbol, EntityGeometryScalar,
    RuntimeEntityAssets, invalid, validate_compiled, validate_geometry_scalar, validate_identifier,
    validate_scalars,
};

pub const MAX_ENTITY_ANIMATION_CLIPS: usize = 4_096;
pub const MAX_ENTITY_ANIMATION_CHANNELS: usize = 65_536;
pub const MAX_ENTITY_ANIMATION_KEYFRAMES: usize = 524_288;
pub const MAX_ENTITY_CONTROLLERS: usize = 2_048;
pub const MAX_ENTITY_CONTROLLER_STATES: usize = 16_384;
pub const MAX_ENTITY_CONTROLLER_TRANSITIONS: usize = 32_768;
pub const MAX_ENTITY_CONTROLLER_ANIMATIONS: usize = 524_288;
pub const MAX_MOLANG_EXPRESSIONS: usize = 65_536;
pub const MAX_MOLANG_OPS_PER_EXPRESSION: usize = 256;
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
pub const MAX_ENTITY_RIG_TEXTURES: usize = 2_048;
pub const MAX_ENTITY_RIG_TEXTURE_SIDE: usize = 2_048;
pub const MAX_ENTITY_RIG_TEXTURE_BYTES: usize = 256 * 1024 * 1024;

#[path = "v4/preflight.rs"]
mod preflight;
pub(super) use preflight::payload_counts;
#[path = "v4/encode.rs"]
mod encode;
pub(super) use encode::{encode_compiled, encode_runtime};
#[path = "v4/rig.rs"]
mod rig;
use rig::validate_rig_payload;

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
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAnimationChannel {
    pub bone: u32,
    pub property: EntityAnimationProperty,
    pub first_keyframe: u32,
    pub keyframe_count: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAnimationKeyframe {
    pub time_seconds: EntityGeometryScalar,
    pub value: [EntityGeometryScalar; 3],
    pub interpolation: EntityAnimationInterpolation,
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
#[serde(rename_all = "snake_case", tag = "op", content = "operand")]
pub enum MolangOp {
    Push(EntityGeometryScalar),
    LoadQuery(u32),
    LoadVariable(u32),
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Negate,
    Not,
    Abs,
    Ceil,
    Floor,
    Round,
    Sqrt,
    Sin,
    Cos,
    And,
    Or,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Min,
    Max,
    Select,
    Clamp,
    Lerp,
    SelectCollection(u32),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAnimationController {
    pub symbol: u32,
    pub first_state: u32,
    pub state_count: u16,
    pub initial_state: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityControllerState {
    pub name: u32,
    pub first_animation: u32,
    pub animation_count: u16,
    pub first_transition: u32,
    pub transition_count: u16,
    pub on_entry: Option<u32>,
    pub on_exit: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityControllerAnimation {
    pub clip: u32,
    pub weight: Option<u32>,
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
    pub default_texture: Option<u32>,
    pub fallback: EntityRigFallback,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRigTexture {
    pub symbol: u32,
    pub source: u32,
    pub width: u16,
    pub height: u16,
    pub pixels_sha256: [u8; 32],
    pub rgba8: Box<[u8]>,
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
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRigControllerBinding {
    pub name: u32,
    pub controller: u32,
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
    pub rig_textures: usize,
    pub rig_texture_bytes: usize,
    pub item_visuals: usize,
    pub item_visual_aliases: usize,
    pub block_visuals: usize,
}

impl CompiledEntityAssets {
    pub fn validate(&self) -> Result<(), AssetError> {
        validate_compiled(self)
    }
}

impl RuntimeEntityAssets {
    #[must_use]
    pub fn animation_clips(&self) -> &[EntityAnimationClip] {
        &self.animation_clips
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
    pub fn rig_textures(&self) -> &[EntityRigTexture] {
        &self.rig_textures
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
            rig_textures: self.rig_textures.len(),
            rig_texture_bytes: self
                .rig_textures
                .iter()
                .map(|texture| texture.rgba8.len())
                .sum(),
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
    validate_item_visuals(
        &compiled.item_visuals,
        &compiled.item_visual_aliases,
        compiled.sources.len(),
        compiled.block_visual_count as usize,
    )?;
    for visual in &compiled.item_visuals {
        let defining_path = &compiled.sources[visual.source as usize].path;
        if !valid_item_definition_source(defining_path) {
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

fn valid_item_definition_source(path: &str) -> bool {
    (path.starts_with("entity/") && path.ends_with(".json"))
        || path == "textures/item_texture.json"
        || path == "registry/block-item-routes-v1001.json"
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
        if keyframe.time_seconds.get() < 0.0 {
            return Err(invalid("entity animation keyframe time is negative"));
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

fn validate_molang_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.molang_symbols.len() > MAX_MOLANG_EXPRESSIONS
        || compiled.molang_expressions.len() > MAX_MOLANG_EXPRESSIONS
        || compiled.molang_ops.len() > MAX_MOLANG_OPS
        || compiled.molang_collections.len() > MAX_MOLANG_COLLECTIONS
        || compiled.molang_collection_items.len() > MAX_MOLANG_COLLECTION_ITEMS_TOTAL
    {
        return Err(invalid("Molang payload count exceeds bound"));
    }
    let mut previous: Option<(MolangSymbolKind, &str)> = None;
    for symbol in &compiled.molang_symbols {
        validate_molang_symbol(symbol)?;
        let key = (symbol.kind, symbol.identifier.as_ref());
        if previous.is_some_and(|value| value >= key) {
            return Err(invalid("Molang symbols are not strictly ordered"));
        }
        previous = Some(key);
    }
    for expression in &compiled.molang_expressions {
        if expression.op_count as usize > MAX_MOLANG_OPS_PER_EXPRESSION
            || expression.max_stack > MAX_MOLANG_STACK_DEPTH
            || !range_in_bounds(
                expression.first_op,
                u32::from(expression.op_count),
                compiled.molang_ops.len(),
            )
        {
            return Err(invalid("invalid Molang expression range or stack bound"));
        }
        let start = expression.first_op as usize;
        let end = start + expression.op_count as usize;
        validate_molang_stack(&compiled.molang_ops[start..end], expression.max_stack)?;
    }
    validate_flattened_ranges(
        compiled
            .molang_expressions
            .iter()
            .map(|expression| (expression.first_op, u32::from(expression.op_count))),
        compiled.molang_ops.len(),
        "Molang operation",
    )?;
    for op in &compiled.molang_ops {
        match *op {
            MolangOp::Push(value) => validate_geometry_scalar(value)?,
            MolangOp::LoadQuery(symbol) => {
                if !molang_symbol_has_kind(compiled, symbol, &[MolangSymbolKind::Query]) {
                    return Err(invalid("Molang query symbol kind is invalid"));
                }
            }
            MolangOp::LoadVariable(symbol) => {
                if !molang_symbol_has_kind(
                    compiled,
                    symbol,
                    &[MolangSymbolKind::Variable, MolangSymbolKind::Temporary],
                ) {
                    return Err(invalid("Molang variable symbol kind is invalid"));
                }
            }
            MolangOp::SelectCollection(collection) => {
                if collection as usize >= compiled.molang_collections.len() {
                    return Err(invalid("Molang collection index is out of range"));
                }
            }
            MolangOp::Add
            | MolangOp::Subtract
            | MolangOp::Multiply
            | MolangOp::Divide
            | MolangOp::Modulo
            | MolangOp::Negate
            | MolangOp::Not
            | MolangOp::Abs
            | MolangOp::Ceil
            | MolangOp::Floor
            | MolangOp::Round
            | MolangOp::Sqrt
            | MolangOp::Sin
            | MolangOp::Cos
            | MolangOp::And
            | MolangOp::Or
            | MolangOp::Equal
            | MolangOp::NotEqual
            | MolangOp::Less
            | MolangOp::LessEqual
            | MolangOp::Greater
            | MolangOp::GreaterEqual
            | MolangOp::Min
            | MolangOp::Max
            | MolangOp::Select
            | MolangOp::Clamp
            | MolangOp::Lerp => {}
        }
    }
    for collection in &compiled.molang_collections {
        if collection.item_count == 0
            || collection.item_count as usize > MAX_MOLANG_COLLECTION_ITEMS
            || !range_in_bounds(
                collection.first_item,
                u32::from(collection.item_count),
                compiled.molang_collection_items.len(),
            )
        {
            return Err(invalid("invalid Molang collection range"));
        }
    }
    validate_flattened_ranges(
        compiled
            .molang_collections
            .iter()
            .map(|collection| (collection.first_item, u32::from(collection.item_count))),
        compiled.molang_collection_items.len(),
        "Molang collection item",
    )?;
    for item in &compiled.molang_collection_items {
        validate_geometry_scalar(item.value)?;
    }
    Ok(())
}

fn validate_molang_symbol(symbol: &MolangSymbol) -> Result<(), AssetError> {
    validate_identifier(&symbol.identifier)?;
    let valid = match symbol.kind {
        MolangSymbolKind::Name => !["query.", "variable.", "temp."]
            .iter()
            .any(|prefix| symbol.identifier.starts_with(prefix)),
        MolangSymbolKind::Query => matches!(
            symbol.identifier.as_ref(),
            "query.anim_time"
                | "query.life_time"
                | "query.modified_move_speed"
                | "query.ground_speed"
                | "query.is_on_ground"
                | "query.is_moving"
                | "query.is_sprinting"
                | "query.is_sneaking"
                | "query.is_sleeping"
                | "query.body_y_rotation"
                | "query.head_y_rotation"
                | "query.target_x_rotation"
        ),
        MolangSymbolKind::Variable => valid_molang_slot(&symbol.identifier, "variable."),
        MolangSymbolKind::Temporary => valid_molang_slot(&symbol.identifier, "temp."),
    };
    if !valid {
        return Err(invalid("Molang symbol is outside the reviewed namespace"));
    }
    Ok(())
}

fn valid_molang_slot(identifier: &str, prefix: &str) -> bool {
    identifier.strip_prefix(prefix).is_some_and(|slot| {
        !slot.is_empty()
            && slot
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    })
}

fn molang_symbol_has_kind(
    compiled: &CompiledEntityAssets,
    index: u32,
    permitted: &[MolangSymbolKind],
) -> bool {
    compiled
        .molang_symbols
        .get(index as usize)
        .is_some_and(|symbol| permitted.contains(&symbol.kind))
}

fn validate_molang_stack(ops: &[MolangOp], declared_max: u8) -> Result<(), AssetError> {
    let mut depth = 0usize;
    let mut observed_max = 0usize;
    for op in ops {
        match op {
            MolangOp::Push(_) | MolangOp::LoadQuery(_) | MolangOp::LoadVariable(_) => depth += 1,
            MolangOp::Add
            | MolangOp::Subtract
            | MolangOp::Multiply
            | MolangOp::Divide
            | MolangOp::Modulo
            | MolangOp::And
            | MolangOp::Or
            | MolangOp::Equal
            | MolangOp::NotEqual
            | MolangOp::Less
            | MolangOp::LessEqual
            | MolangOp::Greater
            | MolangOp::GreaterEqual
            | MolangOp::Min
            | MolangOp::Max => {
                if depth < 2 {
                    return Err(invalid("Molang expression stack underflows"));
                }
                depth -= 1;
            }
            MolangOp::Negate
            | MolangOp::Not
            | MolangOp::Abs
            | MolangOp::Ceil
            | MolangOp::Floor
            | MolangOp::Round
            | MolangOp::Sqrt
            | MolangOp::Sin
            | MolangOp::Cos
            | MolangOp::SelectCollection(_) => {
                if depth < 1 {
                    return Err(invalid("Molang expression stack underflows"));
                }
            }
            MolangOp::Select | MolangOp::Clamp | MolangOp::Lerp => {
                if depth < 3 {
                    return Err(invalid("Molang expression stack underflows"));
                }
                depth -= 2;
            }
        }
        observed_max = observed_max.max(depth);
        if observed_max > declared_max as usize {
            return Err(invalid("Molang expression exceeds its declared stack"));
        }
    }
    if depth != 1 {
        return Err(invalid(
            "Molang expression must leave exactly one stack value",
        ));
    }
    if observed_max != declared_max as usize {
        return Err(invalid("Molang expression declared stack is not exact"));
    }
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
    Ok(())
}

fn validate_controller_state(
    compiled: &CompiledEntityAssets,
    state: &EntityControllerState,
    controller_state_count: u16,
) -> Result<(), AssetError> {
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
        if animation.clip as usize >= compiled.animation_clips.len()
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
