//! Observes existing local pose and picking authority for the chat coordinate header.

use bevy::{ecs::system::SystemParam, prelude::Res};

use crate::{
    local_player::{InteractionOriginSnapshot, LocalPlayerFrameCarrier},
    movement::PhysicsCollisionRegistries,
    runtime::world::ClientWorld,
    semantic_controls::SemanticInputSnapshot,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

#[derive(SystemParam)]
pub(crate) struct ChatCoordinateContext<'w> {
    frame: Res<'w, LocalPlayerFrameCarrier>,
    origin: Res<'w, InteractionOriginSnapshot>,
    input: Res<'w, SemanticInputSnapshot>,
    world: Res<'w, ClientWorld>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
}

impl ChatCoordinateContext<'_> {
    /// Refreshes both sources without generating inventory or network actions.
    pub(super) fn publish(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        runtime: &UiRuntime,
        presentation: &mut UiPresentationRuntime,
    ) {
        let position = self
            .frame
            .snapshot()
            .map(|frame| frame.pose().translation.to_array());
        presentation.set_chat_coordinates(position, self.target(player_runtime, runtime));
    }

    /// Reuses block picking's validated ray and input-mode reach.
    fn target(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        runtime: &UiRuntime,
    ) -> Option<[i32; 3]> {
        let snapshot = self.input.snapshot()?;
        let selection = crate::mining::hand_interaction_selection(player_runtime)?;
        let mode = crate::mining::protocol_input_mode(snapshot.input_mode);
        let reach = if player_runtime.facts.player_game_mode()
            == Some(protocol::PlayerGameMode::Creative)
        {
            crate::mining::creative_reach(mode)
        } else {
            crate::mining::survival_reach(mode)
        };
        crate::interaction_authority::observe_block(
            &self.origin,
            runtime,
            &self.world,
            self.collisions.as_deref()?,
            selection,
            (
                mode,
                reach,
                (snapshot.authority_generation, snapshot.frame_sequence),
                0,
            ),
        )
        .map(|observed| observed.target.position)
    }
}
