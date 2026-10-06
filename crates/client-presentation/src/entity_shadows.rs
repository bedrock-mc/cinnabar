//! Publishes this frame's entity-shadow casters for the renderer.
use bevy::math::Vec3;
use chunk_pipeline::WorldStream;
use render::{ACTOR_LAYER_BODY, ActorCullView, ActorRenderFrame, EntityShadowScene};
use render_model::EntityShadow;

use crate::presentation::actors::within_actor_candidate_cube;

/// A drawn local body's caster follows its render-time feet; a spectator casts none.
#[derive(Debug, Clone, Copy)]
pub struct LocalShadowSource {
    pub runtime_id: u64,
    pub feet: Option<[f32; 3]>,
    pub spectator: bool,
}

/// Sorted runtime ids of the bodies `frame` drew, excluding capacity overflow and undrawable rigs.
pub fn drawn_bodies(frame: &ActorRenderFrame, out: &mut Vec<u64>) {
    out.clear();
    out.extend(
        frame
            .rig
            .manifest
            .iter()
            .filter(|entry| entry.identity.layer == ACTOR_LAYER_BODY)
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
                local.spectator || drawn_bodies.binary_search(&local.runtime_id).is_err()
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
                matches!(&actor.kind, protocol::ActorKind::Entity { identifier }
                    if identifier.as_ref() == "minecraft:item")
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

#[cfg(test)]
mod tests;
