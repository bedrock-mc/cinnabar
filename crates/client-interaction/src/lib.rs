//! Player interaction resources, camera input and inventory selection.
use bevy::prelude::{App, IntoScheduleConfigs, Plugin, Update};
use client_state::{ClientFrameSet, ClientInteractionSet};

mod audio_observations;
pub mod block_use;
pub mod camera;
pub mod hotbar;
pub mod item_use;
pub mod melee;
pub mod mining;
pub mod movement;
mod presentation_observations;
pub mod survival_mining;

/// Installs interaction authority and the input systems that use the shared frame contract.
#[derive(Default)]
pub struct ClientInteractionPlugin;

impl Plugin for ClientInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<block_use::BlockUseRuntime>()
            .init_resource::<item_use::ItemUseRuntime>()
            .init_resource::<survival_mining::SurvivalMiningRuntime>()
            .init_resource::<melee::MeleeRuntime>()
            .init_resource::<melee::SwingTracker>()
            .add_systems(
                Update,
                hotbar::queue_hotbar_selection
                    .after(ClientFrameSet::SemanticFinalize)
                    .before(ClientInteractionSet::HotbarSend)
                    .before(ClientFrameSet::ActorPreparation),
            )
            .add_systems(
                Update,
                block_use::retain_block_use_pick
                    .in_set(ClientInteractionSet::RetainPick)
                    .in_set(client_presentation::local_player::LocalPlayerFrameSet::Interaction)
                    .in_set(ClientFrameSet::Interaction),
            );
    }
}
