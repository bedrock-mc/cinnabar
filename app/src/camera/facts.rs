//! Borrows gameplay and UI facts at the existing camera phase.
use super::{CameraFovInputs, ScreenEffectFacts};
use crate::{
    item_use::ItemUseRuntime, local_player::LocalViewPose, movement::PhysicsCollisionRegistries,
    runtime::world::ClientWorld,
};
use bevy::prelude::{Res, ResMut};
pub use client_presentation::camera::facts::ItemUseClock;
use client_ui::ui_runtime::UiRuntime;

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_screen_effect_facts(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    time: Res<bevy::prelude::Time>,
    view: Res<LocalViewPose>,
    item_use: Option<Res<ItemUseRuntime>>,
    ui: Option<Res<UiRuntime>>,
    client_world: Option<Res<ClientWorld>>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    clock: ResMut<ItemUseClock>,
    facts: ResMut<ScreenEffectFacts>,
    fov: ResMut<CameraFovInputs>,
) {
    client_presentation::camera::facts::collect_screen_effect_facts(
        &player_runtime,
        time,
        view,
        item_use
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::ItemUseObservation),
        ui.as_deref(),
        client_world.as_deref().map(
            |world| client_presentation::observations::WorldObservation {
                stream: world.stream.as_ref(),
            },
        ),
        collisions
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
        clock,
        facts,
        fov,
    );
}
