//! Per-state component resolution: permutation conditions and bone visibility evaluate as
//! block Molang against one state's values.

use std::sync::Arc;

use pack_compiler::{BlockMolang, BlockStateValue};
use protocol::{CustomBlock, CustomStateValue, CustomVisualComponents};

use super::OverlayGaps;

/// What one block state draws.
pub(super) struct StateVisual {
    pub(super) components: CustomVisualComponents,
    /// Geometry bones whose cubes the state's bone visibility hides.
    pub(super) hidden_bones: Box<[Arc<str>]>,
}

/// A block's permutation conditions and bone visibility, parsed once for all of its states.
pub(super) struct BlockExpressions {
    conditions: Box<[Option<BlockMolang>]>,
    /// Bone visibility of each component set: the base, then each permutation.
    bones: Box<[Box<[BoneVisibility]>]>,
}

/// A bone and the condition that shows it; `None` always shows it.
type BoneVisibility = (Arc<str>, Option<BlockMolang>);

impl BlockExpressions {
    pub(super) fn new(block: &CustomBlock) -> Self {
        let visuals = &block.visual;
        let bones = |components: &CustomVisualComponents| {
            components
                .bone_visibility
                .iter()
                .map(|(bone, expression)| (Arc::clone(bone), BlockMolang::parse(expression)))
                .collect()
        };
        Self {
            conditions: visuals
                .permutations
                .iter()
                .map(|permutation| BlockMolang::parse(&permutation.condition))
                .collect(),
            bones: std::iter::once(&visuals.base)
                .chain(
                    visuals
                        .permutations
                        .iter()
                        .map(|permutation| &permutation.components),
                )
                .map(bones)
                .collect(),
        }
    }
}

/// Resolves the state whose values follow the block's `state_axes`. Without values (a state
/// the axes cannot describe) conditions reading block states stay unevaluated and are counted.
pub(super) fn state_visual(
    block: &CustomBlock,
    expressions: &BlockExpressions,
    values: Option<&[CustomStateValue]>,
    gaps: &mut OverlayGaps,
) -> StateVisual {
    let visuals = &block.visual;
    let block_state = |name: &str| {
        let position = visuals
            .state_axes
            .iter()
            .position(|axis| axis.name.as_ref() == name)?;
        Some(match values?.get(position)? {
            CustomStateValue::String(value) => BlockStateValue::String(value),
            CustomStateValue::Int(value) => BlockStateValue::Number(*value as f32),
            CustomStateValue::Bool(value) => BlockStateValue::Number(f32::from(u8::from(*value))),
        })
    };
    let mut components = visuals.base.clone();
    let mut bone_set = 0;
    for (index, (permutation, condition)) in visuals
        .permutations
        .iter()
        .zip(expressions.conditions.iter())
        .enumerate()
    {
        match condition
            .as_ref()
            .and_then(|condition| condition.evaluate(&block_state))
        {
            Some(value) if value != 0.0 => {
                if permutation.components.geometry.is_some() {
                    bone_set = index + 1;
                }
                apply(&mut components, &permutation.components);
            }
            Some(_) => {}
            None => gaps.unevaluated_permutations += 1,
        }
    }
    // A bone hides when its value rounds to zero; one that cannot evaluate stays visible.
    let hidden_bones = expressions.bones[bone_set]
        .iter()
        .filter(|(_, expression)| {
            expression
                .as_ref()
                .and_then(|expression| expression.evaluate(&block_state))
                .is_some_and(|value| value.round() == 0.0)
        })
        .map(|(bone, _)| Arc::clone(bone))
        .collect();
    StateVisual {
        components,
        hidden_bones,
    }
}

fn apply(resolved: &mut CustomVisualComponents, components: &CustomVisualComponents) {
    if components.geometry.is_some() {
        // Bone visibility belongs to the geometry component it arrived with.
        resolved.geometry.clone_from(&components.geometry);
        resolved.geometry_use_block_type_light_absorption =
            components.geometry_use_block_type_light_absorption;
        resolved
            .bone_visibility
            .clone_from(&components.bone_visibility);
    }
    if components.materials.is_some() {
        resolved.materials.clone_from(&components.materials);
    }
    if components.transformation.is_some() {
        resolved.transformation = components.transformation;
    }
    if components.light_dampening.is_some() {
        resolved.light_dampening = components.light_dampening;
    }
    if components.light_emission.is_some() {
        resolved.light_emission = components.light_emission;
    }
}
