//! Click-dependent block states and placement validation before local world mutation.

use std::ops::Range;

use assets::NetworkIdMode;
use sim::PaletteWorld;

use crate::{
    block_use::{UseSurroundings, overlaps, placed_collision_boxes, placement_cell},
    movement::PhysicsCollisionRegistries,
    placement_stacking::stacked_placement_canonical,
    placement_state::{PlacementInput, merge_slab_state, resolve_placement_state},
    placement_support::{
        SupportRequirement, attachment_states, support_acceptance, support_requirement,
    },
};

/// Frozen click and collision facts used for one admitted placement.
pub struct PlacementContext<'a> {
    pub clicked: [i32; 3],
    pub input: PlacementInput,
    pub surroundings: &'a UseSurroundings,
    /// The session's inclusive minimum and exclusive maximum build heights.
    pub build_height: Range<i32>,
}

/// A validated cell and store identity to commit after transport admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PredictedPlacement {
    pub position: [i32; 3],
    pub block: u32,
}

/// Resolves a verified placement state; missing world or survival facts defer to the server.
pub fn predicted_placement(
    collisions: &PhysicsCollisionRegistries,
    mode: NetworkIdMode,
    held: u32,
    world: &PaletteWorld<'_>,
    context: &PlacementContext<'_>,
) -> Option<PredictedPlacement> {
    let identifier = collisions.block_identifier(mode, held)?;
    let canonical = collisions.block_canonical_state(mode, held)?;
    if context.input.face > 5 {
        return None;
    }
    let clicked = world.primary_runtime_id(context.clicked).ok()?;
    if collisions.block_identifier(mode, clicked)
        != context.surroundings.clicked_identifier.as_deref()
    {
        return None;
    }
    let (destination, free) = context
        .surroundings
        .destination(context.clicked, context.input.face);
    let merged = [
        (context.clicked, Some(context.input.face)),
        (destination, None),
    ]
    .into_iter()
    .filter(|(position, face)| face.is_some() || *position != context.clicked)
    .find_map(|(position, face)| {
        let existing = world.primary_runtime_id(position).ok()?;
        let name = collisions.block_identifier(mode, existing)?;
        let state = collisions.block_canonical_state(mode, existing)?;
        if let Some((name, state)) = merge_slab_state(identifier, canonical, name, state, face) {
            return Some((position, name, state, false));
        }
        face.and_then(|face| stacked_placement_canonical(identifier, name, state, face))
            .map(|state| (position, identifier.to_owned(), state, true))
    });
    let position = merged.as_ref().map_or(destination, |merged| merged.0);
    if (merged.is_none() && !free) || !context.build_height.contains(&position[1]) {
        return None;
    }
    // A retained click must still refer to readable world data when it is admitted.
    let existing = world.primary_runtime_id(position).ok()?;
    if merged.is_none() && collisions.block_identifier(mode, existing)? != "minecraft:air" {
        return None;
    }
    let support = std::array::from_fn(|face| {
        let block = world
            .primary_runtime_id(placement_cell(position, face as u8))
            .ok()?;
        let support_identifier = collisions.block_identifier(mode, block)?;
        support_acceptance(
            identifier,
            support_identifier,
            collisions.block_is_full_cube(mode, block),
        )
    });
    let (placed_identifier, mut states, full_cell) =
        if let Some((_, name, states, full_cell)) = merged {
            (name, states, full_cell)
        } else {
            let original = serde_json::from_str(canonical).ok()?;
            let states = attachment_states(identifier, &original, context.input, support)
                .or_else(|| resolve_placement_state(identifier, canonical, context.input))?;
            (identifier.to_owned(), states, false)
        };
    // Stacking existing snow skips the first-placement support check.
    let requirement = if full_cell {
        SupportRequirement::Independent
    } else {
        support_requirement(&placed_identifier, &states)
    };
    match requirement {
        SupportRequirement::Independent => {}
        SupportRequirement::Face(face) if support[usize::from(face)] == Some(true) => {}
        SupportRequirement::Face(_) | SupportRequirement::Unsupported => return None,
    }
    crate::placement_connections::states_for_neighbours(
        collisions,
        mode,
        world,
        position,
        &placed_identifier,
        &mut states,
    )?;
    let block = collisions.block_state_runtime_id(mode, &placed_identifier, &states)?;
    let shapes = placed_collision_boxes(collisions.registry(mode), block, position)?;
    let actor_overlap = |local| {
        std::iter::once(&context.surroundings.player_box)
            .chain(&context.surroundings.actor_boxes)
            .any(|actor| overlaps(position, local, *actor))
    };
    if (full_cell && actor_overlap(([0.0; 3], [1.0; 3]))) || shapes.into_iter().any(actor_overlap) {
        return None;
    }
    Some(PredictedPlacement { position, block })
}
