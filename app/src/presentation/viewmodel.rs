//! Samples the viewmodel's world, screen and movement observations in the current frame.
use crate::{player_runtime::PlayerRuntime, runtime::world::ClientWorld};
use bevy::{ecs::system::SystemParam, prelude::*};
pub(crate) use client_presentation::presentation::viewmodel::HandAdapter;
#[cfg(test)]
pub(crate) use client_presentation::presentation::viewmodel::HandFallback;
use client_presentation::presentation::viewmodel::{ViewmodelAuthority, ViewmodelWorld};
use client_ui::ui_runtime::UiRuntime;

#[derive(SystemParam)]
pub(crate) struct ViewmodelPublish<'w, 's> {
    inner: client_presentation::presentation::viewmodel::ViewmodelPublish<'w, 's>,
    menu: Option<Res<'w, crate::menu::MenuRuntime>>,
    movement: Option<Res<'w, crate::movement::MovementTicker>>,
}

impl<'w, 's> std::ops::Deref for ViewmodelPublish<'w, 's> {
    type Target = client_presentation::presentation::viewmodel::ViewmodelPublish<'w, 's>;
    /// Exposes presentation operations that require no app observations.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl std::ops::DerefMut for ViewmodelPublish<'_, '_> {
    /// Exposes presentation operations that require no app observations.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Borrows the current authority and artwork without retaining another mutable owner.
fn observation<'a>(
    world: &'a ClientWorld,
    movement: Option<&crate::movement::MovementTicker>,
    renders_game: bool,
) -> ViewmodelWorld<'a> {
    ViewmodelWorld {
        stream: world.stream.as_ref(),
        entity_assets: world.entity_assets.as_ref(),
        runtime_assets: &world.runtime_assets,
        block_registry_hash: crate::asset_startup::pinned_world_provenance().block_registry_sha256,
        renders_game,
        movement: movement.map(|movement| {
            let (session, epoch) = movement.interaction_authority_identity();
            ViewmodelAuthority {
                session,
                epoch,
                authorized: movement.physics_is_authorized(),
            }
        }),
    }
}

impl ViewmodelPublish<'_, '_> {
    /// Publishes from the same borrowed owners as the UI's pre-send capture.
    pub(crate) fn observe(
        &mut self,
        player: &PlayerRuntime,
        ui: &UiRuntime,
        world: &ClientWorld,
        first_person: bool,
        hidden: bool,
        viewport: [u32; 2],
    ) -> bool {
        let renders_game =
            crate::screen_policy::renders_game(player, Some(ui), self.menu.as_deref(), None);
        self.inner.observe(
            player,
            ui,
            &observation(world, self.movement.as_deref(), renders_game),
            first_person,
            hidden,
            viewport,
        )
    }

    /// Samples the evidence fields from exactly the same borrowed presentation inputs.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)] // Mirrors the existing presentation snapshot inputs.
    pub(crate) fn diagnostic_snapshot(
        &self,
        player: &PlayerRuntime,
        ui: &UiRuntime,
        world: &ClientWorld,
        first_person: bool,
        hidden: bool,
        viewport: [u32; 2],
        completed: bool,
    ) -> (u8, [i128; 32]) {
        let renders_game =
            crate::screen_policy::renders_game(player, Some(ui), self.menu.as_deref(), None);
        self.inner.diagnostic_snapshot(
            player,
            ui,
            &observation(world, self.movement.as_deref(), renders_game),
            first_person,
            hidden,
            viewport,
            completed,
        )
    }
}
