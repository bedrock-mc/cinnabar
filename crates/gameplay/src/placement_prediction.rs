//! Click-dependent block states and placement validation before local world mutation.

use std::ops::Range;

use assets::NetworkIdMode;
use sim::PaletteWorld;

use crate::{
    block_use::{BoxBounds, UseSurroundings, overlaps, placement_cell},
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredictedPlacement {
    pub position: [i32; 3],
    pub block: u32,
    pub additional: Vec<([i32; 3], u32)>,
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
    if crate::placement_support::is_candle(identifier)
        && context.input.face == 1
        && collisions.block_identifier(mode, clicked)? == identifier
    {
        let states = serde_json::from_str(collisions.block_canonical_state(mode, clicked)?).ok()?;
        if crate::placement_state::value(&states, "candles")?.as_u64()? >= 3 {
            return None;
        }
    }
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
        let stack_face = face.or_else(|| {
            (crate::placement_support::is_candle(identifier)
                || identifier == "minecraft:sea_pickle")
                .then_some(context.input.face)
        });
        stack_face
            .and_then(|face| stacked_placement_canonical(identifier, name, state, face))
            .map(|state| {
                (
                    position,
                    identifier.to_owned(),
                    state,
                    identifier == "minecraft:snow_layer",
                )
            })
    });
    let position = merged.as_ref().map_or(destination, |merged| merged.0);
    if (merged.is_none() && !free) || !context.build_height.contains(&position[1]) {
        return None;
    }
    // A retained click must still refer to readable world data when it is admitted.
    let existing = world.primary_runtime_id(position).ok()?;
    if crate::placement_support::is_candle(identifier)
        || identifier == "minecraft:sea_pickle"
        || identifier == "minecraft:vine"
        || crate::placement_multiface::is_multiface(identifier)
    {
        use sim::CollisionWorld;
        let sample = world.block_physics(position).ok()?;
        if sample.layers.iter().any(|layer| {
            layer.flags.contains(sim::BlockPhysicsFlags::WATER)
                || layer.flags.contains(sim::BlockPhysicsFlags::LAVA)
        }) {
            return None;
        }
    }
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
    let door_pair = crate::placement_doors::door_states(
        identifier,
        canonical,
        position,
        context.input.yaw,
        |cell| {
            let block = world.primary_runtime_id(cell).ok()?;
            Some(collisions.block_identifier(mode, block)?.to_owned())
        },
    );
    let (placed_identifier, mut states, full_cell) =
        if let Some((_, name, states, full_cell)) = merged {
            (name, states, full_cell)
        } else if let Some((name, states)) =
            crate::placement_signs::sign_states(identifier, canonical, context.input)
        {
            (name, states, false)
        } else {
            let original = serde_json::from_str(canonical).ok()?;
            let states = attachment_states(identifier, &original, context.input, support)
                .or_else(|| {
                    crate::placement_facing::facing_states(
                        identifier,
                        canonical,
                        position,
                        context.surroundings.player_box,
                        context.input.yaw,
                    )
                })
                .or_else(|| door_pair.as_ref().map(|pair| pair.0.clone()))
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
    let read = |position| {
        let block = world.primary_runtime_id(position).ok()?;
        Some((
            collisions.block_identifier(mode, block)?.to_owned(),
            serde_json::from_str(collisions.block_canonical_state(mode, block)?).ok()?,
        ))
    };
    crate::placement_stairs::corner_states(&placed_identifier, &mut states, position, read)?;
    let block = collisions.block_state_runtime_id(mode, &placed_identifier, &states)?;
    let mut neighbors = Vec::new();
    let door = crate::placement_doors::is_door(&placed_identifier);
    if door {
        if context.input.face != 1 {
            return None;
        }
        let upper_position = placement_cell(position, 1);
        if !context.build_height.contains(&upper_position[1])
            || collisions.block_identifier(mode, world.primary_runtime_id(upper_position).ok()?)?
                != "minecraft:air"
        {
            return None;
        }
        let (_, upper) = door_pair?;
        let upper_block = collisions.block_state_runtime_id(mode, &placed_identifier, &upper)?;
        neighbors.push((upper_position, upper_block));
    }
    if placed_identifier.ends_with("_stairs") {
        for face in 2..=5 {
            let neighbor_position = placement_cell(position, face);
            let (name, mut neighbor) = read(neighbor_position)?;
            if !name.ends_with("_stairs") {
                continue;
            }
            crate::placement_stairs::corner_states(
                &name,
                &mut neighbor,
                neighbor_position,
                |cell| {
                    if cell == position {
                        Some((placed_identifier.clone(), states.clone()))
                    } else {
                        read(cell)
                    }
                },
            )?;
            let neighbor_block = collisions.block_state_runtime_id(mode, &name, &neighbor)?;
            if world.primary_runtime_id(neighbor_position).ok()? != neighbor_block {
                neighbors.push((neighbor_position, neighbor_block));
            }
        }
    }
    let actor_overlap = |cell, local| {
        std::iter::once(&context.surroundings.player_box)
            .chain(&context.surroundings.actor_boxes)
            .any(|actor| overlaps(cell, local, *actor))
    };
    if full_cell && actor_overlap(position, ([0.0; 3], [1.0; 3])) {
        return None;
    }
    neighbors.push((position, block));
    for &(cell, runtime_id) in &neighbors {
        if cell != position && !door {
            continue;
        }
        let shapes = world
            .collision_shapes_with_updates(cell, runtime_id, &neighbors)
            .ok()?;
        if shapes.iter().any(|shape| {
            let local: BoxBounds = (
                [shape.min.x, shape.min.y, shape.min.z],
                [shape.max.x, shape.max.y, shape.max.z],
            );
            actor_overlap(cell, local)
        }) {
            return None;
        }
    }
    neighbors.pop();
    Some(PredictedPlacement {
        position,
        block,
        additional: neighbors,
    })
}
