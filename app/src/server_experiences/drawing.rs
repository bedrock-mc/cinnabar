//! Publishes client-part output through the personal mod renderer and its native frame fence.

use super::driver::ExperienceService;
use crate::environment::ClientPartEnvironment;
use bevy::prelude::*;
use render_scene::ModRenderScene;

/// Publishes session drawing after personal drawing through the same render budgets.
#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
pub(super) fn present(
    mut service: ResMut<ExperienceService>,
    mut scene: ResMut<ModRenderScene>,
    mut environment: ResMut<ClientPartEnvironment>,
    world: Res<crate::runtime::world::ClientWorld>,
    actors: Option<Res<render_scene::ActorRenderFrame>>,
    cameras: Query<&GlobalTransform, With<client_presentation::camera::FlyCamera>>,
) {
    let Some(live) = &mut service.live else {
        if scene
            .bypass_change_detection()
            .apply_server(&Default::default(), 0)
        {
            scene.set_changed();
        }
        *environment = ClientPartEnvironment::default();
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
        *environment = ClientPartEnvironment::default();
        return;
    };
    let authority = stream.authority();
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
    if scene
        .bypass_change_detection()
        .apply_server(output, revision)
    {
        scene.set_changed();
    }
}
