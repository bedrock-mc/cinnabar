//! Adapts fresh world evidence to the gameplay-owned held placement intention.
use super::*;
use gameplay::{
    block_use::PlacementTarget,
    melee::{Crosshair, classify, pick_actor},
};
use sim::CollisionWorld;

/// Recasts the current ray and resolves the held support without reusing stale block evidence.
#[allow(clippy::too_many_arguments)]
pub(super) fn observe_use_target(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    context: &BlockUseContext,
    input_mode: PlayerInputMode,
    survival: bool,
    input_authority: (std::num::NonZeroU64, u64),
    position_authority_generation: u64,
    runtime: &BlockUseRuntime,
    velocity: [f32; 3],
    sneaking: bool,
) -> Option<FrozenBlockObservation> {
    let reach = if survival {
        survival_reach(input_mode)
    } else {
        creative_reach(input_mode)
    };
    let stream = context.client_world.stream.as_ref()?;
    let ray = context.origin.outbound_ray()?;
    let actor = pick_actor(
        stream.authority().remote_actors(),
        context.ui.gameplay_hud().mount_unique_id(),
        ray.origin().to_array(),
        ray.direction().to_array(),
        reach,
    );
    let selection = verified_use_selection(player_runtime, &context.ui)?;
    let orientation_hit = held_block_store_id(stream, selection.item.block_runtime_id())
        .filter(|block| {
            gameplay::block_use::orientation_sensitive(
                context
                    .collisions
                    .block_canonical_state(stream.network_id_mode(), *block),
            )
        })
        .and_then(|_| runtime.intention.first_world_hit());
    let observed = crate::interaction_authority::observe_block_ray_using(
        &context.origin,
        &context.ui,
        &context.client_world,
        &context.collisions,
        selection,
        (
            input_mode,
            reach,
            input_authority,
            position_authority_generation,
        ),
        |world, origin, direction, reach| {
            let hit = world.block_interaction_ray_current(origin, direction, reach)?;
            let actor = match classify(actor, hit.as_ref().map(|hit| hit.distance), reach) {
                Crosshair::Actor(actor) => Some(actor),
                _ => None,
            };
            let block = if actor.is_none() { hit.as_ref() } else { None };
            let endpoint = actor.map_or_else(
                || {
                    block.map_or(origin + direction * reach, |hit| {
                        origin + direction * hit.distance
                    })
                },
                |actor| {
                    sim::Vec3::new(
                        f64::from(actor.point[0]),
                        f64::from(actor.point[1]),
                        f64::from(actor.point[2]),
                    )
                },
            );
            let target = runtime.intention.target(
                block.map(|hit| PlacementTarget {
                    position: hit.block_pos,
                    face: hit.face,
                }),
                [origin.x as f32, origin.y as f32, origin.z as f32],
                [endpoint.x as f32, endpoint.y as f32, endpoint.z as f32],
                velocity,
                sneaking,
            );
            let Some(target) = target else {
                return Ok(None);
            };
            let primary = world
                .primary_is_air(target.position)?
                .ok_or(sim::WorldQueryError::InvalidRayOrigin)?;
            let runtime_id = world.primary_runtime_id(target.position)?;
            // Miss continuation uses the support corner; a real pick retains its world intercept.
            let click = orientation_hit
                .map(|point| point.map(f64::from))
                .unwrap_or_else(|| {
                    actor.map_or_else(
                        || {
                            block.map_or(target.position.map(|n| n as f64), |hit| {
                                [
                                    hit.block_pos[0] as f64 + hit.hit_local.x,
                                    hit.block_pos[1] as f64 + hit.hit_local.y,
                                    hit.block_pos[2] as f64 + hit.hit_local.z,
                                ]
                            })
                        },
                        |actor| actor.point.map(f64::from),
                    )
                });
            Ok(Some(sim::BlockHit {
                block_pos: target.position,
                face: target.face,
                hit_local: sim::Vec3::new(
                    click[0] - target.position[0] as f64,
                    click[1] - target.position[1] as f64,
                    click[2] - target.position[2] as f64,
                ),
                runtime_id,
                distance: actor.map_or_else(
                    || block.map_or(reach, |hit| hit.distance),
                    |actor| actor.distance,
                ),
                identity: primary.identity,
            }))
        },
    )
    .ok()
    .flatten()?;
    gameplay::interaction_authority::within_pick_range(&observed).then_some(observed)
}
