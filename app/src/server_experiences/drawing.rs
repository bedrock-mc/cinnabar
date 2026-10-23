//! Publishes client-part output through the personal mod renderer and its native frame fence.

use super::driver::ExperienceService;
use bevy::prelude::*;
use render_scene::{ModRenderScene, mod_scene::retained::DrawingFrame};

/// Session-scoped local time and weather, read by the common atmosphere owner.
#[derive(Resource, Default)]
pub(crate) struct Environment {
    pub time: Option<u32>,
    pub weather: Option<[f32; 2]>,
}

/// Publishes after personal drawing so both producers share one frame and render budgets.
#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
pub(super) fn present(
    mut service: ResMut<ExperienceService>,
    mut scene: ResMut<ModRenderScene>,
    mut frame: ResMut<DrawingFrame>,
    mut environment: ResMut<Environment>,
    world: Res<crate::runtime::world::ClientWorld>,
    actors: Option<Res<render_scene::ActorRenderFrame>>,
    cameras: Query<&GlobalTransform, With<client_presentation::camera::FlyCamera>>,
    mut owned: Local<Vec<(u64, u64)>>,
    mut entity_tokens: Local<Vec<mod_api::drawing::DrawingHandle>>,
) {
    for owner in owned.drain(..) {
        frame.live_owners.remove(&owner);
    }
    for token in entity_tokens.drain(..) {
        frame.entity_poses.remove(&token);
    }
    let Some(live) = &mut service.live else {
        if scene
            .bypass_change_detection()
            .apply_server(&Default::default(), 0)
        {
            scene.set_changed();
        }
        *environment = Environment::default();
        return;
    };
    let Some(stream) = world
        .stream
        .as_ref()
        .filter(|_| !world.dimension_transfer.active())
    else {
        if scene
            .bypass_change_detection()
            .apply_server(&Default::default(), 0)
        {
            scene.set_changed();
        }
        *environment = Environment::default();
        return;
    };
    let authority = stream.authority();
    frame.connection = authority.actor_session_id();
    frame.dimension = authority.current_dimension();
    let candidates = cameras
        .single()
        .map(|camera| {
            crate::modding::anchors::candidates(authority, camera.translation().to_array())
        })
        .unwrap_or_default();
    let targets = actors
        .as_deref()
        .map(render_scene::mod_scene::mask_targets)
        .unwrap_or_default()
        .into_iter()
        .take(mod_api::drawing::MAX_MASK_ENTITIES)
        .collect();
    live.set_drawing_context(candidates, targets, |key| {
        key.session == authority.actor_session_id()
            && key.dimension == authority.current_dimension()
            && authority
                .actor(key.runtime_id)
                .is_some_and(|actor| actor.spawn_revision == key.spawn_revision)
    });
    let (time, weather) = live.environment();
    environment.time = time;
    environment.weather = weather;
    let (output, revision) = live.drawing();
    for resources in &output.retained {
        if let Some(actors) = actors.as_deref() {
            for (&id, &key) in &resources.entities {
                if let Some(pose) =
                    render_scene::mod_scene::retained::RootPose::from_actor_frame(actors, key)
                {
                    frame.entity_poses.insert(id, pose);
                    entity_tokens.push(id);
                }
            }
        }
        crate::modding::drawing_policy::publish_retained(
            resources,
            authority.terrain(),
            &mut frame,
        );
    }
    owned.extend(output.retained.iter().flat_map(|resources| {
        resources
            .geometry
            .keys()
            .chain(resources.materials.keys())
            .map(|id| (id.owner, id.generation))
    }));
    if scene
        .bypass_change_detection()
        .apply_server(output, revision)
    {
        scene.set_changed();
    }
}
