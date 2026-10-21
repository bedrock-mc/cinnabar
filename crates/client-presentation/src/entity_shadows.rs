//! Publishes this frame's entity-shadow casters for the renderer.
use bevy::math::Vec3;
use chunk_pipeline::WorldStream;
use render_model::EntityShadow;
use {
    render_model::{ACTOR_LAYER_BODY, ActorCullView},
    render_scene::{ActorRenderFrame, EntityShadowScene},
};

use crate::presentation::actors::within_actor_candidate_cube;

/// A drawn local body's caster follows its render-time feet; a spectator casts none.
#[derive(Debug, Clone, Copy)]
pub struct LocalShadowSource {
    pub runtime_id: u64,
    pub visible: bool,
    pub feet: Option<[f32; 3]>,
    pub spectator: bool,
}

/// Sorted runtime ids of the network session bodies drawn in this frame.
pub fn drawn_bodies(frame: &ActorRenderFrame, session_id: Option<u64>, out: &mut Vec<u64>) {
    out.clear();
    out.extend(
        frame
            .rig
            .manifest
            .iter()
            .filter(|entry| {
                entry.identity.layer == ACTOR_LAYER_BODY
                    && Some(entry.identity.session_id) == session_id
                    && !frame
                        .mask_only_runtime_ids
                        .contains(&entry.identity.runtime_id)
            })
            .map(|entry| entry.identity.runtime_id),
    );
    out.sort_unstable();
}

/// Rebuilds the caster list into `staging` and publishes it; an unchanged list keeps the scene's
/// revision. Rigged actors cast only if `drawn_bodies` (sorted) holds their bodies; dropped
/// items, drawn outside the rig frame, are admitted as actors are and culled by volume.
pub fn publish_entity_shadows(
    stream: Option<&WorldStream>,
    partial_tick: f32,
    local: Option<LocalShadowSource>,
    view: Option<ActorCullView>,
    drawn_bodies: &[u64],
    staging: &mut Vec<EntityShadow>,
    scene: &mut EntityShadowScene,
) {
    staging.clear();
    if let Some(stream) = stream {
        for caster in stream.authority().actor_shadow_casters(partial_tick) {
            let local = local.filter(|local| local.runtime_id == caster.runtime_id);
            if local.is_some_and(|local| {
                !local.visible
                    || local.spectator
                    || drawn_bodies.binary_search(&local.runtime_id).is_err()
            }) {
                continue;
            }
            let feet = local.and_then(|local| local.feet).unwrap_or(caster.feet);
            let shadow = EntityShadow {
                feet,
                radius: caster.radius,
            };
            let actor = stream.authority().actor(caster.runtime_id);
            let item = actor.is_some_and(|actor| {
                actor.entity_fact.native_class == assets::facts::entity::NativeEntityClass::Item
            });
            let admitted = if local.is_some() || item {
                let player = local.is_some();
                view.is_none_or(|view| volume_may_be_visible(&shadow, player, view))
            } else {
                drawn_bodies.binary_search(&caster.runtime_id).is_ok()
            };
            if admitted {
                staging.push(shadow);
            }
        }
    }
    scene.0.publish(staging);
}

fn volume_may_be_visible(shadow: &EntityShadow, player: bool, view: ActorCullView) -> bool {
    let camera = view.camera_position.to_array();
    if !player && !within_actor_candidate_cube(shadow.feet, camera) {
        return false;
    }
    if Vec3::from_array(shadow.feet).distance_squared(view.camera_position)
        > view.max_distance * view.max_distance
    {
        return false;
    }
    let (low, high) = shadow.bounds();
    let corners: [bevy::math::Vec4; 8] = std::array::from_fn(|index| {
        let pick = |axis: usize| {
            if index >> axis & 1 == 0 {
                low[axis]
            } else {
                high[axis]
            }
        };
        view.clip_from_world * bevy::math::Vec4::new(pick(0), pick(1), pick(2), 1.0)
    });
    let outside = |test: fn(&bevy::math::Vec4) -> bool| corners.iter().all(test);
    !(outside(|clip| clip.x < -clip.w)
        || outside(|clip| clip.x > clip.w)
        || outside(|clip| clip.y < -clip.w)
        || outside(|clip| clip.y > clip.w)
        || outside(|clip| clip.w <= 0.0))
}

/// Samples the local visibility carrier and publishes shadows from the drawn actor frame.
#[allow(clippy::too_many_arguments)]
pub fn drive_entity_shadows(
    stream: Option<&WorldStream>,
    player: &player_state::PlayerState,
    partial_tick: bevy::prelude::Res<crate::actor_publication::ActorFramePartialTick>,
    local: bevy::prelude::Res<crate::local_player::LocalAvatarVisibilityCarrier>,
    camera: bevy::prelude::Query<
        (&bevy::prelude::Transform, &bevy::prelude::Projection),
        bevy::prelude::With<crate::camera::FlyCamera>,
    >,
    frame: bevy::prelude::Res<ActorRenderFrame>,
    mut drawn: bevy::prelude::Local<Vec<u64>>,
    mut staging: bevy::prelude::Local<Vec<EntityShadow>>,
    scene: Option<bevy::prelude::ResMut<EntityShadowScene>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    let local = stream.map(|stream| {
        let runtime_id = stream.local_player_runtime_id();
        crate::entity_shadows::LocalShadowSource {
            runtime_id,
            visible: local.snapshot().is_some_and(|visibility| {
                visibility.runtime_id() == runtime_id && visibility.visible()
            }),
            feet: local
                .snapshot()
                .filter(|visibility| visibility.runtime_id() == runtime_id)
                .map(|visibility| visibility.feet().to_array()),
            spectator: player
                .facts
                .game_mode_capabilities()
                .is_some_and(|caps| !caps.visible),
        }
    });
    let view = camera
        .single()
        .ok()
        .map(|(transform, projection)| render_model::ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
        });
    crate::entity_shadows::drawn_bodies(
        &frame,
        stream.map(|stream| stream.authority().actor_session_id()),
        &mut drawn,
    );
    crate::entity_shadows::publish_entity_shadows(
        stream,
        partial_tick.0,
        local,
        view,
        &drawn,
        &mut staging,
        &mut scene,
    );
}

#[cfg(test)]
mod tests;
