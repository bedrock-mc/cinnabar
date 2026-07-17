use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    AssetError,
    item::{ItemVisualAlias, ItemVisualDefinition, validate_item_visuals},
};

use super::{
    CompiledEntityAssets, EntityAssetKind, EntityAssetSymbol, EntityGeometryScalar,
    RuntimeEntityAssets, encode_entity_blob, invalid, validate_compiled, validate_geometry_scalar,
    validate_identifier, validate_scalars,
};

pub const MAX_ENTITY_ANIMATION_CLIPS: usize = 4_096;
pub const MAX_ENTITY_ANIMATION_CHANNELS: usize = 65_536;
pub const MAX_ENTITY_ANIMATION_KEYFRAMES: usize = 524_288;
pub const MAX_ENTITY_CONTROLLERS: usize = 2_048;
pub const MAX_ENTITY_CONTROLLER_STATES: usize = 16_384;
pub const MAX_ENTITY_CONTROLLER_TRANSITIONS: usize = 32_768;
pub const MAX_MOLANG_EXPRESSIONS: usize = 65_536;
pub const MAX_MOLANG_OPS_PER_EXPRESSION: usize = 256;
pub const MAX_MOLANG_STACK_DEPTH: u8 = 32;
pub const MAX_MOLANG_COLLECTION_ITEMS: usize = 32;
pub const MAX_ENTITY_RIG_BINDINGS: usize = 8_192;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityCatalogCountProbe {
    sources: SequenceCount,
    symbols: SequenceCount,
    geometries: SequenceCount,
    animation_clips: SequenceCount,
    #[serde(rename = "animation_channels")]
    _animation_channels: de::IgnoredAny,
    #[serde(rename = "animation_keyframes")]
    _animation_keyframes: de::IgnoredAny,
    #[serde(rename = "molang_symbols")]
    _molang_symbols: de::IgnoredAny,
    #[serde(rename = "molang_expressions")]
    _molang_expressions: de::IgnoredAny,
    #[serde(rename = "molang_ops")]
    _molang_ops: de::IgnoredAny,
    controllers: SequenceCount,
    #[serde(rename = "controller_states")]
    _controller_states: de::IgnoredAny,
    #[serde(rename = "controller_animations")]
    _controller_animations: de::IgnoredAny,
    #[serde(rename = "controller_transitions")]
    _controller_transitions: de::IgnoredAny,
    rig_bindings: SequenceCount,
    #[serde(rename = "rig_animations")]
    _rig_animations: de::IgnoredAny,
    #[serde(rename = "rig_controllers")]
    _rig_controllers: de::IgnoredAny,
    item_visuals: SequenceCount,
    #[serde(rename = "item_visual_aliases")]
    _item_visual_aliases: de::IgnoredAny,
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

pub(super) fn payload_counts(bytes: &[u8]) -> Result<[usize; 7], serde_json::Error> {
    let counts: EntityCatalogCountProbe = serde_json::from_slice(bytes)?;
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "op", content = "operand")]
pub enum MolangOp {
    Push(EntityGeometryScalar),
    LoadQuery(u32),
    LoadVariable(u32),
    StoreVariable(u32),
    Add,
    Subtract,
    Multiply,
    Divide,
    Negate,
    Not,
    And,
    Or,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Select,
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
    pub geometry: u32,
    pub render_controller: u32,
    pub first_animation: u32,
    pub animation_count: u16,
    pub first_controller: u32,
    pub controller_count: u16,
    pub fallback: EntityRigFallback,
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
    pub controller_transitions: usize,
    pub molang_expressions: usize,
    pub rig_bindings: usize,
    pub item_visuals: usize,
    pub item_visual_aliases: usize,
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
    pub fn molang_symbols(&self) -> &[Box<str>] {
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
    pub fn rig_animations(&self) -> &[EntityRigAnimationBinding] {
        &self.rig_animations
    }

    #[must_use]
    pub fn rig_controllers(&self) -> &[EntityRigControllerBinding] {
        &self.rig_controllers
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
            controller_transitions: self.controller_transitions.len(),
            molang_expressions: self.molang_expressions.len(),
            rig_bindings: self.rig_bindings.len(),
            item_visuals: self.item_visuals.len(),
            item_visual_aliases: self.item_visual_aliases.len(),
        }
    }

    pub fn encode(&self) -> Result<Box<[u8]>, AssetError> {
        encode_entity_blob(&CompiledEntityAssets {
            source_manifest_sha256: self.source_manifest_sha256,
            sources: self.sources.iter().cloned().collect(),
            symbols: self.symbols.iter().cloned().collect(),
            geometries: self.geometries.iter().cloned().collect(),
            animation_clips: self.animation_clips.iter().copied().collect(),
            animation_channels: self.animation_channels.iter().copied().collect(),
            animation_keyframes: self.animation_keyframes.iter().copied().collect(),
            molang_symbols: self.molang_symbols.iter().cloned().collect(),
            molang_expressions: self.molang_expressions.iter().copied().collect(),
            molang_ops: self.molang_ops.iter().copied().collect(),
            controllers: self.controllers.iter().copied().collect(),
            controller_states: self.controller_states.iter().copied().collect(),
            controller_animations: self.controller_animations.iter().copied().collect(),
            controller_transitions: self.controller_transitions.iter().copied().collect(),
            rig_bindings: self.rig_bindings.iter().copied().collect(),
            rig_animations: self.rig_animations.iter().copied().collect(),
            rig_controllers: self.rig_controllers.iter().copied().collect(),
            item_visuals: self.item_visuals.iter().cloned().collect(),
            item_visual_aliases: self.item_visual_aliases.iter().cloned().collect(),
        })
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
    )?;
    for visual in &compiled.item_visuals {
        let path = &compiled.sources[visual.texture_source as usize].path;
        if !path.starts_with("textures/entity/")
            || !(path.ends_with(".png") || path.ends_with(".tga"))
        {
            return Err(invalid("item visual source is not an entity texture"));
        }
    }
    Ok(())
}

fn validate_animation_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.animation_clips.len() > MAX_ENTITY_ANIMATION_CLIPS
        || compiled.animation_channels.len() > MAX_ENTITY_ANIMATION_CHANNELS
        || compiled.animation_keyframes.len() > MAX_ENTITY_ANIMATION_KEYFRAMES
    {
        return Err(invalid("entity animation payload count exceeds bound"));
    }
    let bone_count = compiled
        .geometries
        .iter()
        .try_fold(0usize, |total, geometry| {
            total
                .checked_add(geometry.bones.len())
                .ok_or_else(|| invalid("entity animation bone count overflow"))
        })?;
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
        if channel.bone as usize >= bone_count
            || !range_in_bounds(
                channel.first_keyframe,
                channel.keyframe_count,
                compiled.animation_keyframes.len(),
            )
        {
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
    Ok(())
}

fn validate_molang_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.molang_symbols.len() > MAX_MOLANG_EXPRESSIONS
        || compiled.molang_expressions.len() > MAX_MOLANG_EXPRESSIONS
    {
        return Err(invalid("Molang symbol or expression count exceeds bound"));
    }
    let mut previous: Option<&str> = None;
    for symbol in &compiled.molang_symbols {
        validate_identifier(symbol)?;
        if previous.is_some_and(|value| value >= symbol.as_ref()) {
            return Err(invalid("Molang symbols are not strictly ordered"));
        }
        previous = Some(symbol);
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
    for op in &compiled.molang_ops {
        match *op {
            MolangOp::Push(value) => validate_geometry_scalar(value)?,
            MolangOp::LoadQuery(symbol)
            | MolangOp::LoadVariable(symbol)
            | MolangOp::StoreVariable(symbol) => {
                if symbol as usize >= compiled.molang_symbols.len() {
                    return Err(invalid("Molang symbol index is out of range"));
                }
            }
            MolangOp::Add
            | MolangOp::Subtract
            | MolangOp::Multiply
            | MolangOp::Divide
            | MolangOp::Negate
            | MolangOp::Not
            | MolangOp::And
            | MolangOp::Or
            | MolangOp::Equal
            | MolangOp::NotEqual
            | MolangOp::Less
            | MolangOp::LessEqual
            | MolangOp::Greater
            | MolangOp::GreaterEqual
            | MolangOp::Select => {}
        }
    }
    Ok(())
}

fn validate_molang_stack(ops: &[MolangOp], declared_max: u8) -> Result<(), AssetError> {
    let mut depth = 0usize;
    let mut observed_max = 0usize;
    for op in ops {
        match op {
            MolangOp::Push(_) | MolangOp::LoadQuery(_) | MolangOp::LoadVariable(_) => depth += 1,
            MolangOp::StoreVariable(_) => {
                if depth < 1 {
                    return Err(invalid("Molang expression stack underflows"));
                }
                depth -= 1;
            }
            MolangOp::Add
            | MolangOp::Subtract
            | MolangOp::Multiply
            | MolangOp::Divide
            | MolangOp::And
            | MolangOp::Or
            | MolangOp::Equal
            | MolangOp::NotEqual
            | MolangOp::Less
            | MolangOp::LessEqual
            | MolangOp::Greater
            | MolangOp::GreaterEqual => {
                if depth < 2 {
                    return Err(invalid("Molang expression stack underflows"));
                }
                depth -= 1;
            }
            MolangOp::Negate | MolangOp::Not => {
                if depth < 1 {
                    return Err(invalid("Molang expression stack underflows"));
                }
            }
            MolangOp::Select => {
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
    Ok(())
}

fn validate_controller_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.controllers.len() > MAX_ENTITY_CONTROLLERS
        || compiled.controller_states.len() > MAX_ENTITY_CONTROLLER_STATES
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
    Ok(())
}

fn validate_controller_state(
    compiled: &CompiledEntityAssets,
    state: &EntityControllerState,
    controller_state_count: u16,
) -> Result<(), AssetError> {
    if state.name as usize >= compiled.molang_symbols.len()
        || state.animation_count as usize > MAX_MOLANG_COLLECTION_ITEMS
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

fn validate_rig_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.rig_bindings.len() > MAX_ENTITY_RIG_BINDINGS {
        return Err(invalid("entity rig binding count exceeds bound"));
    }
    for binding in &compiled.rig_bindings {
        if !index_has_kind(
            &compiled.symbols,
            binding.entity_symbol,
            EntityAssetKind::Entity,
        ) || binding.geometry as usize >= compiled.geometries.len()
            || !index_has_kind(
                &compiled.symbols,
                binding.render_controller,
                EntityAssetKind::RenderController,
            )
            || !range_in_bounds(
                binding.first_animation,
                u32::from(binding.animation_count),
                compiled.rig_animations.len(),
            )
            || !range_in_bounds(
                binding.first_controller,
                u32::from(binding.controller_count),
                compiled.rig_controllers.len(),
            )
        {
            return Err(invalid("entity rig binding index is out of range"));
        }
    }
    for binding in &compiled.rig_animations {
        if binding.name as usize >= compiled.molang_symbols.len()
            || binding.clip as usize >= compiled.animation_clips.len()
        {
            return Err(invalid("entity rig animation index is out of range"));
        }
    }
    for binding in &compiled.rig_controllers {
        if binding.name as usize >= compiled.molang_symbols.len()
            || binding.controller as usize >= compiled.controllers.len()
        {
            return Err(invalid("entity rig controller index is out of range"));
        }
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
