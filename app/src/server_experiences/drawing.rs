//! Publishes client-part output through the personal mod renderer and its native frame fence.

use super::driver::ExperienceService;
use crate::environment::ClientPartEnvironment;
use bevy::prelude::*;
use render_scene::ModRenderScene;

/// Reads the native authority and render publication used by personal mods.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Frame<'w, 's> {
    pub world: Res<'w, crate::runtime::world::ClientWorld>,
    actors: Option<Res<'w, render_scene::ActorRenderFrame>>,
    cameras: Query<'w, 's, &'static GlobalTransform, With<client_presentation::camera::FlyCamera>>,
}

impl Frame<'_, '_> {
    /// Returns only actor incarnations admitted in the current native dimension type.
    pub fn current(&self, key: mod_render::retained::EntityKey) -> bool {
        self.world.stream.as_ref().is_some_and(|stream| {
            let authority = stream.authority();
            key.session == authority.actor_session_id()
                && key.dimension == authority.current_dimension()
                && authority
                    .actor(key.runtime_id)
                    .is_some_and(|actor| actor.render_spawn_revision.get() == key.spawn_revision)
        })
    }

    /// Captures authority and identities together, pausing callbacks during dimension transfers.
    pub fn context(&self, epoch: u64) -> Option<mod_host::presentation::Context> {
        let authority = self
            .world
            .stream
            .as_ref()
            .filter(|_| !self.world.dimension_transfer.active())?
            .authority();
        let candidates = self
            .cameras
            .single()
            .map(|camera| {
                crate::modding::anchors::candidates(authority, camera.translation().to_array())
            })
            .unwrap_or_default();
        let targets = self
            .actors
            .as_deref()
            .map(render_scene::mod_scene::mask_targets)
            .unwrap_or_default()
            .into_iter()
            .filter(|(key, _)| {
                self.current(mod_render::retained::EntityKey {
                    session: key.connection,
                    dimension: key.dimension,
                    runtime_id: key.runtime_id,
                    spawn_revision: key.spawn_revision,
                })
            })
            .take(mod_api::drawing::MAX_MASK_ENTITIES)
            .collect();
        Some(mod_host::presentation::Context {
            connection: authority.actor_session_id(),
            epoch,
            dimension: authority.current_dimension(),
            live: candidates.clone(),
            candidates,
            targets,
        })
    }
}

/// Publishes session drawing after personal drawing through the same render budgets.
#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
pub(super) fn present(
    mut service: ResMut<ExperienceService>,
    mut scene: ResMut<ModRenderScene>,
    mut environment: ResMut<ClientPartEnvironment>,
    world: Res<crate::runtime::world::ClientWorld>,
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
    let Some(_) = world
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
