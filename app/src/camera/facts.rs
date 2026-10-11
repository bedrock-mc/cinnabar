//! Borrows gameplay and UI facts at the existing camera phase.
use bevy::prelude::{Local, Res, ResMut, Time};
use client_presentation::camera::facts::ItemUseClock;
use client_presentation::camera::{CameraFovInputs, ScreenEffectFacts};
use client_ui::ui_runtime::UiRuntime;
use {
    crate::{
        item_use::ItemUseRuntime,
        movement::{LocalPhysicsController, PhysicsCollisionRegistries},
        runtime::world::ClientWorld,
    },
    client_presentation::local_player::LocalViewPose,
};

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_screen_effect_facts(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    time: Res<bevy::prelude::Time>,
    item_use: Option<Res<ItemUseRuntime>>,
    ui: Option<Res<UiRuntime>>,
    client_world: Option<Res<ClientWorld>>,
    clock: ResMut<ItemUseClock>,
    facts: ResMut<ScreenEffectFacts>,
    fov: ResMut<CameraFovInputs>,
) {
    client_presentation::camera::facts::collect_screen_effect_facts(
        &player_runtime,
        time,
        item_use
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::ItemUseObservation),
        ui.as_deref(),
        client_world.as_deref().map(
            |world| client_presentation::observations::WorldObservation {
                stream: world.stream.as_ref(),
            },
        ),
        clock,
        facts,
        fov,
    );
}

pub(crate) fn collect_portal_contact(
    player_runtime: Res<crate::player_runtime::PlayerRuntime>,
    view: Res<LocalViewPose>,
    physics: Option<Res<LocalPhysicsController>>,
    client_world: Option<Res<ClientWorld>>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    mut facts: ResMut<ScreenEffectFacts>,
) {
    client_presentation::camera::facts::collect_portal_contact(
        &player_runtime,
        &view,
        physics
            .as_deref()
            .map(|physics| physics as &dyn client_presentation::observations::PhysicsObservation),
        client_world.as_deref().map(
            |world| client_presentation::observations::WorldObservation {
                stream: world.stream.as_ref(),
            },
        ),
        collisions.as_deref().map(|collisions| {
            collisions as &dyn client_presentation::observations::CollisionLookup
        }),
        &mut facts,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn diagnose_portal(
    time: Option<Res<Time<bevy::time::Real>>>,
    view: Res<LocalViewPose>,
    facts: Res<ScreenEffectFacts>,
    portal: Res<client_presentation::camera::PortalProgress>,
    overlays: Res<client_presentation::camera::ScreenOverlays>,
    settings: Res<client_presentation::camera::CameraSettingsAuthority>,
    physics: Option<Res<LocalPhysicsController>>,
    client_world: Option<Res<ClientWorld>>,
    mut diagnostics: Local<client_presentation::camera::portal_diagnostics::PortalDiagnostics>,
) {
    let Some(time) = time else {
        return;
    };
    let transfer_active = client_world
        .as_deref()
        .is_some_and(|world| world.dimension_transfer.active());
    client_presentation::camera::portal_diagnostics::diagnose_portal(
        time,
        view,
        facts,
        portal,
        overlays,
        settings,
        physics
            .as_deref()
            .map(|physics| physics as &dyn client_presentation::observations::PhysicsObservation),
        client_world.as_deref().map(
            |world| client_presentation::observations::WorldObservation {
                stream: world.stream.as_ref(),
            },
        ),
        transfer_active,
        &mut diagnostics,
    );
}
