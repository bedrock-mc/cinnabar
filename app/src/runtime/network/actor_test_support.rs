use crate::local_player::FrozenLocalAvatarVisibility;
use client_world::{ActorSnapshot, PlayerProfile};
use render::{ActorCullView, ActorRenderFrame, ActorRenderScene, ActorRenderSource};
use render_model::ActorSkinPixels;

/// Builds the simple actor render fixture used by publication tests.
pub(crate) fn actor_render_source(
    actor: &ActorSnapshot,
    profile: Option<&PlayerProfile>,
) -> ActorRenderSource {
    let skin = profile.and_then(|profile| match &profile.skin {
        protocol::PlayerSkin::Standard(skin) => Some(ActorSkinPixels {
            width: skin.width,
            height: skin.height,
            rgba8: skin.rgba8.clone(),
        }),
        protocol::PlayerSkin::Unavailable(_) => None,
    });
    ActorRenderSource {
        runtime_id: actor.runtime_id,
        unique_id: actor.unique_id,
        spawn_revision: actor.spawn_revision,
        movement_revision: actor.movement_revision,
        previous_position: actor.previous_pose.position,
        previous_pitch_degrees: actor.previous_pose.pitch,
        previous_yaw_degrees: actor.previous_pose.yaw,
        previous_head_yaw_degrees: actor.previous_pose.head_yaw,
        position: actor.position,
        pitch_degrees: actor.pitch,
        yaw_degrees: actor.yaw,
        head_yaw_degrees: actor.head_yaw,
        teleported: actor.teleported,
        skin,
    }
}

/// Combines local and remote actor fixtures for render-scene tests.
pub(crate) fn update_actor_render_scene<'a>(
    scene: &'a mut ActorRenderScene,
    partial_tick: f32,
    cull_view: Option<ActorCullView>,
    mut remote_sources: Vec<ActorRenderSource>,
    local: Option<&FrozenLocalAvatarVisibility>,
) -> &'a ActorRenderFrame {
    if let Some(local) = local {
        remote_sources.retain(|source| source.runtime_id != local.runtime_id());
    }
    let local = local.filter(|local| local.visible()).map(|local| {
        let (yaw, pitch, _) = local.rotation().to_euler(bevy::math::EulerRot::YXZ);
        let yaw_degrees = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
        let pitch_degrees = -pitch.to_degrees();
        let position = local.feet();
        ActorRenderSource {
            runtime_id: local.runtime_id(),
            unique_id: i64::try_from(local.runtime_id()).unwrap_or(i64::MAX),
            spawn_revision: local.session_generation(),
            movement_revision: local.pose_generation(),
            previous_position: position.to_array(),
            previous_pitch_degrees: pitch_degrees,
            previous_yaw_degrees: yaw_degrees,
            previous_head_yaw_degrees: yaw_degrees,
            position: position.to_array(),
            pitch_degrees,
            yaw_degrees,
            head_yaw_degrees: yaw_degrees,
            teleported: false,
            skin: None,
        }
    });
    scene.update_with_local(partial_tick, cull_view, remote_sources, local)
}
