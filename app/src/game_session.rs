//! Shared session-state retirement before a host starts another login.
use crate::runtime::{shutdown::FatalError, world::ClientWorld};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use client_interaction::movement::{LocalPhysicsController, MovementTicker};
use client_presentation::local_player::{
    InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameReset,
};
use client_runtime::{
    pack_admission::ResourcePackAdmissionState,
    session::{begin_session, release_off_frame},
};
use client_shell::ui_resources::UiRuntimeResource;
use client_state::PlayerRuntime;

/// Stops local movement authority and drops the ended session's frame and interaction samples.
pub(crate) fn quiesce_local_player(
    movement: &mut MovementTicker,
    local_physics: &mut LocalPhysicsController,
    frame: &mut LocalPlayerFrameCarrier,
    interaction: &mut InteractionOriginSnapshot,
) {
    movement.deactivate();
    local_physics.deactivate();
    frame.reset(LocalPlayerFrameReset::Session);
    interaction.invalidate();
}

/// Game-owned resources retired together independently of host transport and process services.
#[derive(SystemParam)]
pub(crate) struct SessionWorld<'w> {
    pub(crate) resource_packs: ResMut<'w, ResourcePackAdmissionState>,
    pub(crate) runtime: ResMut<'w, UiRuntimeResource>,
    pub(crate) player_runtime: ResMut<'w, PlayerRuntime>,
    pub(crate) client_world: ResMut<'w, ClientWorld>,
    pub(crate) fatal_error: ResMut<'w, FatalError>,
    pub(crate) movement: ResMut<'w, MovementTicker>,
    pub(crate) local_physics: ResMut<'w, LocalPhysicsController>,
    pub(crate) local_frame: ResMut<'w, LocalPlayerFrameCarrier>,
    pub(crate) interaction: ResMut<'w, InteractionOriginSnapshot>,
}

impl SessionWorld<'_> {
    /// Fences old gameplay and presentation before any replacement session can publish.
    pub(crate) fn retire(&mut self, generation: u64) {
        self.resource_packs.begin_generation(generation);
        begin_session(&mut self.runtime, &mut self.player_runtime, generation);
        release_off_frame((
            self.client_world.stream.take(),
            self.client_world.pack_entities.take(),
            self.client_world.prepared_actor_artwork.take(),
            self.client_world.session_items.take(),
            self.client_world.pending_digger_registry.take(),
            std::mem::take(&mut self.client_world.digger_filters),
        ));
        self.client_world.pending_surface_spawn = None;
        self.fatal_error.0 = None;
        self.client_world.transfer_notice = None;
        quiesce_local_player(
            &mut self.movement,
            &mut self.local_physics,
            &mut self.local_frame,
            &mut self.interaction,
        );
    }
}
