use std::sync::Arc;

use pack_entity_compiler::BlockMolang;
use protocol::{CustomBlock, CustomBlockPhysics, CustomBlocks, CustomPhysicalComponents};

/// Bounds materialized state records across a session without allocating the reserve.
const MAX_RESOLVED_PHYSICS_STATES: usize = 1 << 18;

type Condition<'a> = (BlockMolang, &'a CustomPhysicalComponents);

/// Parses only conditions that can change physical components.
fn conditions(block: &CustomBlock) -> (Vec<Condition<'_>>, usize) {
    let mut skipped = 0;
    let conditions = block
        .visual
        .permutations
        .iter()
        .filter_map(|permutation| {
            let physical = &permutation.physical;
            if !physical.affects_physics() {
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

/// Resolves one physical state and records conditions whose values are unknown.
fn evaluate(
    block: &CustomBlock,
    state: u32,
    conditions: &[Condition<'_>],
    complete: &mut bool,
) -> Option<CustomBlockPhysics> {
    let values = block.state_values(state)?;
    let mut physics = block.base_physics();
    for (condition, physical) in conditions {
        let Some(value) = crate::block_condition::evaluate(condition, block, &values) else {
            *complete = false;
            continue;
        };
        if value == 0.0 {
            continue;
        }
        if let Some(collision) = &physical.collision {
            physics.collides = collision.enabled;
            physics.collision_boxes.clone_from(&collision.boxes);
        }
        if let Some(offset) = physical.random_offset {
            physics.random_offset = Some(offset);
        }
        if let Some(transform) = physical.transformation {
            physics.transformation = Some(transform);
        }
        if let Some(selection) = physical.selection {
            physics.selection = selection;
        }
        if let Some(friction) = physical.friction {
            physics.friction = Some(friction);
        }
    }
    Some(physics)
}

/// Materializes state components once; registries never evaluate conditions while moving.
pub(crate) fn resolve(blocks: &mut CustomBlocks) {
    resolve_bounded(blocks, MAX_RESOLVED_PHYSICS_STATES);
}

/// Retains the physical reservation independently of other state consumers.
fn resolve_bounded(blocks: &mut CustomBlocks, mut remaining: usize) {
    for block in Arc::make_mut(&mut blocks.blocks) {
        block.state_physics_complete = false;
        if block.visual.materialization_disabled {
            continue;
        }
        let (conditions, skipped) = conditions(block);
        blocks.skipped += skipped;
        if conditions.is_empty() {
            block.state_physics_complete = !block.visual.components_incomplete && skipped == 0;
            continue;
        }
        if block.visual.state_identity_incomplete || block.state_count as usize > remaining {
            blocks.skipped += 1;
            continue;
        }
        let mut evaluated = true;
        let resolved = (0..block.state_count)
            .map(|state| evaluate(block, state, &conditions, &mut evaluated))
            .collect::<Option<Vec<_>>>();
        if !evaluated {
            blocks.skipped += 1;
        }
        if let Some(resolved) = resolved {
            remaining -= resolved.len();
            block.state_physics = resolved.into();
            block.state_physics_complete =
                !block.visual.components_incomplete && skipped == 0 && evaluated;
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
    let mut complete = true;
    let physics =
        evaluate(block, state, &conditions, &mut complete).unwrap_or_else(|| block.base_physics());
    (physics.collides, physics.collision_boxes, physics.selection)
}

#[cfg(test)]
mod tests;
