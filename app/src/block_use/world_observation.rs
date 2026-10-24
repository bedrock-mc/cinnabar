//! World-dependent use facts share the preparation query observation.

use super::*;

/// Resolves placement surroundings through the caller's evidence-recording palette view.
pub(super) fn use_surroundings(
    context: &BlockUseContext,
    world: &PaletteWorld<'_>,
    observed: &FrozenBlockObservation,
    network_position: [f32; 3],
    sneaking: bool,
) -> UseSurroundings {
    let stream = context.client_world.stream.as_ref();
    let fact = |position| {
        let stream = stream?;
        let mode = stream.network_id_mode();
        let runtime_id = if world.is_air(position).ok()? {
            stream.air_block_id()
        } else {
            world.primary_runtime_id(position).ok()?
        };
        Some(context.collisions.block_facts(mode, runtime_id))
    };
    let feet = [
        f64::from(network_position[0]),
        f64::from(network_position[1] - protocol::PLAYER_NETWORK_OFFSET),
        f64::from(network_position[2]),
    ];
    let half_width = sim::PLAYER_WIDTH * 0.5;
    let profile = context
        .physics
        .as_deref()
        .map_or(sim::SneakProfile::Current, |physics| {
            physics.sneak_profile()
        });
    let height = sim::MovementMode::Walking.hitbox_height(sneaking, profile);
    let mut surroundings = UseSurroundings {
        clicked: context.collisions.block_facts(
            stream.map_or(assets::NetworkIdMode::Sequential, |stream| {
                stream.network_id_mode()
            }),
            observed.target.runtime_id,
        ),
        clicked_canonical_state: stream.and_then(|stream| {
            context
                .collisions
                .block_canonical_state(stream.network_id_mode(), observed.target.runtime_id)
                .map(str::to_owned)
        }),
        held: stream
            .and_then(|stream| {
                let block =
                    held_block_store_id(stream, observed.selection.item.block_runtime_id())?;
                Some(
                    context
                        .collisions
                        .block_facts(stream.network_id_mode(), block),
                )
            })
            .unwrap_or_default(),
        neighbor: fact(placement_cell(
            observed.target.position,
            observed.target.face,
        ))
        .unwrap_or_default(),
        player_box: (
            [feet[0] - half_width, feet[1], feet[2] - half_width],
            [feet[0] + half_width, feet[1] + height, feet[2] + half_width],
        ),
        actor_boxes: stream
            .into_iter()
            .flat_map(|stream| stream.authority().remote_actors())
            .filter(|actor| obstructs_placement(actor))
            .filter_map(|actor| actor.bounding_box())
            .map(|(min, max)| (min.map(f64::from), max.map(f64::from)))
            .collect(),
        sneaking,
        placed_boxes: None,
    };
    if let Some((stream, held)) = stream.and_then(|stream| {
        held_block_store_id(stream, observed.selection.item.block_runtime_id())
            .map(|held| (stream, held))
    }) {
        surroundings.set_placed_collision_shapes(
            context.collisions.registry(stream.network_id_mode()),
            held,
            observed.target.position,
            observed.target.face,
        );
    }
    surroundings
}
