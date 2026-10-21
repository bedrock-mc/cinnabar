//! Current actor authorization and root poses copied from the actor renderer's publication.
use super::{ModRuntime, gameplay::GameplayContext};
use bevy::prelude::*;
use client_extensions::modding::multi::ModHosts;
use mod_render::retained::EntityKey;
use render_scene::mod_scene::retained::{DrawingFrame, RootPose};
use std::collections::BinaryHeap;

/// Supplies bounded nearby candidates; confirmed removal is distinct from unloaded terrain.
pub(super) fn reconcile(hosts: &mut ModHosts, context: &GameplayContext<'_>) {
    if !(0..hosts.host_count())
        .any(|index| hosts.host(index).is_active() && hosts.host(index).grants().entity_observe)
    {
        return;
    }
    let authority = context
        .world
        .as_ref()
        .and_then(|world| world.stream.as_ref())
        .map(|stream| stream.authority());
    let Some(authority) = authority else { return };
    let dimension = authority.current_dimension();
    let session = authority.actor_session_id();
    let mut nearest = BinaryHeap::new();
    if let Some(view) = context.view.as_ref() {
        let origin = view.feet_translation().to_array();
        for actor in authority.remote_actors() {
            let distance = actor
                .position
                .iter()
                .zip(origin)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>();
            if !distance.is_finite()
                || distance > mod_api::MAX_MOB_RANGE_BLOCKS.powi(2)
                || actor.spawn_revision == 0
            {
                continue;
            }
            nearest.push((distance.to_bits(), actor.runtime_id, actor.spawn_revision));
            if nearest.len() > mod_api::MAX_DRAW_ENTITIES {
                nearest.pop();
            }
        }
    }
    let candidates: Vec<_> = nearest
        .into_sorted_vec()
        .into_iter()
        .map(|(_, runtime_id, spawn_revision)| EntityKey {
            session,
            dimension,
            runtime_id,
            spawn_revision,
        })
        .collect();
    for index in 0..hosts.host_count() {
        let host = hosts.host_mut(index);
        let removed: Vec<_> = host
            .drawing_entity_keys()
            .filter(|key| {
                key.session != session
                    || key.dimension != dimension
                    || authority
                        .actor(key.runtime_id)
                        .is_none_or(|actor| actor.spawn_revision != key.spawn_revision)
            })
            .collect();
        let candidates = if host.grants().entity_observe {
            candidates.clone()
        } else {
            Vec::new()
        };
        if let Err(error) = host.update_drawing_entities(candidates, &removed) {
            warn!("Drawing entity admission failed: {}", error.detail);
        }
    }
}

/// Publishes only exact, still-live body roots after the actor renderer's same-frame commit.
pub(super) fn publish(
    runtime: Option<ResMut<ModRuntime>>,
    packet_drawing: Option<ResMut<super::packet_render::Drawing>>,
    context: GameplayContext,
    mut scene: ResMut<render_scene::ModRenderScene>,
    actors: Option<Res<render_scene::ActorRenderFrame>>,
    mut drawing: ResMut<DrawingFrame>,
    cameras: Query<&Transform, With<client_presentation::camera::FlyCamera>>,
) {
    let mut packet_drawing = packet_drawing;
    if let (Some(drawing), Some(actors), Ok(camera)) = (
        packet_drawing.as_deref_mut(),
        actors.as_deref(),
        cameras.single(),
    ) {
        drawing.follow_anchors(
            actors,
            &GlobalTransform::from(*camera),
            |session, dimension, runtime_id| {
                let authority = context.world.as_ref()?.stream.as_ref()?.authority();
                if authority.actor_session_id() != session
                    || authority.current_dimension() != dimension
                {
                    return None;
                }
                authority.actor(runtime_id)?.interpolated_position(
                    context
                        .actor_partial
                        .as_deref()
                        .map_or(0., |partial| partial.0),
                )
            },
        );
    }
    let Some(mut runtime) = runtime else {
        *drawing = Default::default();
        if let Some(packet_drawing) = packet_drawing.as_deref() {
            super::packet_render::publish_frame(packet_drawing, &context, &mut drawing);
            super::packet_render::publish_alone(&mut scene, packet_drawing);
        }
        return;
    };
    let changed = super::drawing_policy::bind(&mut runtime.hosts, &context);
    if changed {
        scene.clear();
        runtime.render_sources.clear();
    }
    reconcile(&mut runtime.hosts, &context);
    super::drawing_policy::publish_frame(&runtime.hosts, &context, &mut drawing);
    if let Some(packet_drawing) = packet_drawing.as_deref() {
        super::packet_render::publish_frame(packet_drawing, &context, &mut drawing);
    }
    super::render::publish(Some(scene), &mut runtime, packet_drawing.as_deref());
    let Some(actors) = actors else { return };
    let Some(world) = context
        .world
        .as_ref()
        .filter(|world| !world.dimension_transfer.active())
    else {
        return;
    };
    let Some(stream) = world.stream.as_ref() else {
        return;
    };
    let authority = stream.authority();
    if drawing.dimension != authority.current_dimension() {
        return;
    }
    let Ok(camera) = cameras.single() else { return };
    let origin = camera.translation;
    for (output, _) in runtime.hosts.render_outputs() {
        for resources in &output.retained {
            for (handle, key) in &resources.entities {
                let Some(actor) = authority.actor(key.runtime_id).filter(|actor| {
                    actor.spawn_revision == key.spawn_revision
                        && key.session == authority.actor_session_id()
                        && key.dimension == authority.current_dimension()
                }) else {
                    continue;
                };
                let chunk = world::ChunkKey::new(
                    key.dimension,
                    (actor.position[0].floor() as i32).div_euclid(world::SUB_CHUNK_SIDE as i32),
                    (actor.position[2].floor() as i32).div_euclid(world::SUB_CHUNK_SIDE as i32),
                );
                if !authority.terrain().is_chunk_loaded(chunk) {
                    continue;
                }
                let root = RootPose::from_actor_frame(&actors, *key);
                if let Some(root) = root {
                    let position = Vec3::new(
                        root.world_from_actor[0][3],
                        root.world_from_actor[1][3],
                        root.world_from_actor[2][3],
                    );
                    if position.distance_squared(origin) <= mod_api::MAX_MOB_RANGE_BLOCKS.powi(2) {
                        drawing.entity_poses.insert(*handle, root);
                    }
                }
            }
        }
    }
}
