//! Authored custom-block placement without a compiled vanilla identity.

use assets::{NetworkIdMode, facts::BlockProperty as P, registry::BlockTraits as T};
use protocol::{CustomBlock, CustomStateValue};
use sim::PaletteWorld;

use crate::{
    block_use::{PlacementContext, PredictedPlacement, overlaps, placed_collision_boxes},
    movement::PhysicsCollisionRegistries,
    placement_facing::body_facing,
    placement_state::yaw_quadrant,
};

/// Resolves enabled traits over the held state, retaining authored property values.
fn placement_index(
    definition: &CustomBlock,
    held_state: u32,
    position: [i32; 3],
    context: &PlacementContext<'_>,
) -> Option<u32> {
    let placement = definition.visual.placement;
    let axes = &definition.visual.state_axes;
    if placement.trait_state_count > axes.len()
        || definition.visual.state_identity_incomplete
        || definition.visual.materialization_disabled
        || held_state >= definition.state_count
    {
        return None;
    }
    let mut held = held_state;
    let mut index = 0_u32;
    let mut stride = 1_u32;
    for (axis_index, axis) in axes.iter().enumerate() {
        let count = u32::try_from(axis.values.len()).ok()?;
        if count == 0 {
            return None;
        }
        let held_value = held % count;
        held /= count;
        let value_index = if axis_index < placement.trait_state_count {
            let input = context.input;
            let value = match P::from_name(&axis.name)? {
                P::MinecraftBlockFace => usize::from(input.face),
                P::MinecraftVerticalHalf => usize::from(match input.face {
                    0 => true,
                    1 => false,
                    _ if input.click_position[1].is_finite() => input.click_position[1] > 0.5,
                    _ => return None,
                }),
                P::MinecraftCardinalDirection => {
                    yaw_quadrant(input.yaw - placement.y_rotation_offset)?
                }
                P::MinecraftFacingDirection => usize::from(
                    body_facing(
                        position,
                        context.surroundings.player_box,
                        input.yaw - placement.y_rotation_offset,
                        0.0,
                    )? ^ 1,
                ),
                _ => return None,
            };
            if !matches!(axis.values.get(value), Some(CustomStateValue::String(_))) {
                return None;
            }
            value
        } else {
            held_value as usize
        };
        index = index.checked_add(stride.checked_mul(value_index as u32)?)?;
        stride = stride.checked_mul(count)?;
    }
    (stride == definition.state_count).then_some(index)
}

/// Predicts only admitted states with readable, replaceable destinations and clear actor bounds.
pub(crate) fn predict(
    collisions: &PhysicsCollisionRegistries,
    mode: NetworkIdMode,
    held: u32,
    definition: &CustomBlock,
    held_state: u32,
    world: &PaletteWorld<'_>,
    context: &PlacementContext<'_>,
) -> Option<PredictedPlacement> {
    let placement = definition.visual.placement;
    if context.input.face > 5
        || placement.server_placing
        || placement.has_placement_filter
        || definition.visual.components_incomplete
    {
        return None;
    }
    let clicked = world.primary_runtime_id(context.clicked).ok()?;
    if collisions.block_facts(mode, clicked) != context.surroundings.clicked {
        return None;
    }
    let (position, free) = context
        .surroundings
        .destination(context.clicked, context.input.face);
    if !free || !context.build_height.contains(&position[1]) {
        return None;
    }
    let existing = world.primary_runtime_id(position).ok()?;
    if !collisions
        .block_facts(mode, existing)
        .traits
        .contains(T::REPLACEABLE)
    {
        return None;
    }
    let index = placement_index(definition, held_state, position, context)?;
    // Unresolved permutation physics must not silently use a different state's bounds.
    if !definition.visual.permutations.is_empty()
        && (!definition.state_physics_complete
            || definition.state_physics.len() != definition.state_count as usize)
        && definition
            .visual
            .permutations
            .iter()
            .any(|permutation| permutation.physical.affects_physics())
    {
        return None;
    }
    let block = collisions.custom_placement_runtime_id(mode, held, index)?;
    let boxes = placed_collision_boxes(collisions.registry(mode), block, position)?;
    if std::iter::once(&context.surroundings.player_box)
        .chain(&context.surroundings.actor_boxes)
        .any(|actor| boxes.iter().any(|local| overlaps(position, *local, *actor)))
    {
        return None;
    }
    Some(PredictedPlacement {
        position,
        block,
        additional: Vec::new(),
    })
}
