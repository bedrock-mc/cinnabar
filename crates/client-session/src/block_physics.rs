use std::sync::Arc;

use pack_compiler::{BlockMolang, BlockStateValue};
use protocol::{
    CustomBlock, CustomBlockPhysics, CustomBlocks, CustomPhysicalComponents, CustomStateValue,
};

/// Bounds materialized state records across a session without allocating the reserve.
const MAX_RESOLVED_PHYSICS_STATES: usize = 1 << 18;

type Condition<'a> = (BlockMolang, &'a CustomPhysicalComponents);

fn conditions(block: &CustomBlock) -> (Vec<Condition<'_>>, usize) {
    let mut skipped = 0;
    let conditions = block
        .visual
        .permutations
        .iter()
        .filter_map(|permutation| {
            let physical = &permutation.physical;
            if physical.collision.is_none()
                && physical.selection.is_none()
                && physical.random_offset.is_none()
            {
                return None;
            }
            match BlockMolang::parse(&permutation.condition) {
                Some(expression) => Some((expression, physical)),
                None => {
                    skipped += 1;
                    None
                }
            }
        })
        .collect();
    (conditions, skipped)
}

fn evaluate(
    block: &CustomBlock,
    state: u32,
    conditions: &[Condition<'_>],
) -> Option<CustomBlockPhysics> {
    let values = block.state_values(state)?;
    let query = |name: &str| {
        let index = block
            .visual
            .state_axes
            .iter()
            .position(|axis| axis.name.as_ref() == name)?;
        Some(match &values[index] {
            CustomStateValue::String(value) => BlockStateValue::String(value),
            CustomStateValue::Int(value) => BlockStateValue::Number(*value as f32),
            CustomStateValue::Bool(value) => BlockStateValue::Number(f32::from(*value)),
        })
    };
    let mut physics = block.base_physics();
    for (condition, physical) in conditions {
        if !condition
            .evaluate(&query)
            .is_some_and(|value| value.is_finite() && value != 0.0)
        {
            continue;
        }
        if let Some(collision) = &physical.collision {
            physics.collides = collision.enabled;
            physics.collision_boxes.clone_from(&collision.boxes);
        }
        if let Some(offset) = physical.random_offset {
            physics.random_offset = Some(offset);
        }
        if let Some(selection) = physical.selection {
            physics.selection = selection;
        }
    }
    Some(physics)
}

/// Materializes state components once; registries never evaluate conditions while moving.
pub(crate) fn resolve(blocks: &mut CustomBlocks) {
    resolve_bounded(blocks, MAX_RESOLVED_PHYSICS_STATES);
}

fn resolve_bounded(blocks: &mut CustomBlocks, mut remaining: usize) {
    for block in Arc::make_mut(&mut blocks.blocks) {
        let (conditions, skipped) = conditions(block);
        blocks.skipped += skipped;
        if conditions.is_empty() {
            continue;
        }
        if block.visual.state_identity_incomplete || block.state_count as usize > remaining {
            blocks.skipped += 1;
            continue;
        }
        let resolved = (0..block.state_count)
            .map(|state| evaluate(block, state, &conditions))
            .collect::<Option<Vec<_>>>();
        if let Some(resolved) = resolved {
            remaining -= resolved.len();
            block.state_physics = resolved.into();
        } else {
            blocks.skipped += 1;
        }
    }
}

#[cfg(test)]
fn state_physics(
    block: &CustomBlock,
    state: u32,
) -> (
    bool,
    Option<Arc<[protocol::CustomBox]>>,
    protocol::CustomSelection,
) {
    let (conditions, _) = conditions(block);
    let physics = evaluate(block, state, &conditions).unwrap_or_else(|| block.base_physics());
    (physics.collides, physics.collision_boxes, physics.selection)
}

#[cfg(test)]
mod tests;
