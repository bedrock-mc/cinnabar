//! Session and world adapters for the interaction movement resources.

mod colliders;
mod prediction_sync;
mod runtime_system;
pub(crate) use colliders::MovementColliderCache;
pub(crate) use prediction_sync::send_movement_prediction_sync;
pub(crate) use runtime_system::advance_local_physics;

/// Borrows terrain and retains solid actor bounds and the floor under the current dimension
/// for prediction and correction replay.
pub(crate) fn collision_world<'a>(
    stream: &'a chunk_pipeline::WorldStream,
    registry: &'a sim::CollisionRegistry,
    cache: &MovementColliderCache,
) -> sim::PaletteWorld<'a> {
    let colliders = cache.snapshot(stream.authority().movement_actor_colliders());
    let dimension = stream.current_dimension();
    let world = sim::PaletteWorld::for_movement(stream.collision_store(), registry, dimension)
        .with_actor_colliders(colliders);
    match stream.dimension_range(dimension) {
        Some(range) => world.with_dimension_floor(range.base_sub_chunk_y.saturating_mul(16)),
        None => world,
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod teleport_ack_wiring_tests;
