//! Browser edge sampling and registration of existing production gameplay systems.
use crate::{
    runtime::{network, world},
    semantic_controls,
};
use bevy::prelude::*;
use client_state::ClientFrameSet;

use crate::browser_input::{CanvasCapture, sample_input, screen_focus};

/// Installs production owners in the same authority, simulation and publication phases.
pub(super) fn configure(app: &mut App) {
    crate::primitive_shapes::configure(app);
    use crate::ui_runtime::presentation::{
        observe_mount_jump_input, prepare_ui_runtime, publish_ui_runtime,
    };
    crate::game_frame::configure_client_frame_schedule(app);
    crate::game_frame::configure_world(app);
    app.init_resource::<CanvasCapture>();
    app.add_plugins((
        client_runtime::ClientRuntimePlugin::<network::resource_packs::PackApplication>::default(),
        client_presentation::ClientPresentationPlugin,
        client_interaction::ClientInteractionPlugin,
    ))
    .init_resource::<crate::acceptance::AcceptanceRun>()
    .init_resource::<client_interaction::movement::LocomotionState>()
    .init_resource::<world::WorldStreamFramePoll>()
    .init_resource::<client_state::EmoteInputConsumed>()
    .init_resource::<crate::ui_runtime::presentation::publish::PreparedUiPublication>()
    .init_resource::<crate::melee::MeleeTarget>()
    .init_resource::<client_presentation::server_camera::ServerCameraInstructions>()
    .init_resource::<client_presentation::local_player_camera_receipt::CameraPublicationAttempt>()
    .add_message::<client_audio::audio_ingress::SequencedAudioEvent>()
    .add_observer(crate::runtime::visibility::apply_added_chunk_visibility)
    .add_observer(crate::runtime::visibility::remove_chunk_visibility)
    .add_systems(
        Startup,
        (
            network::initialize_overlay_texture_limit,
            network::initialize_actor_geometry_budget,
        ),
    )
    .add_systems(Update, sample_input.in_set(ClientFrameSet::RawInput))
    .add_systems(
        Update,
        semantic_controls::route_semantic_input.in_set(ClientFrameSet::SemanticSample),
    )
    .add_systems(
        Update,
        (
            super::session::apply_commands,
            network::receive_network_events,
            super::session::observe_terminal,
            world::reconcile_world_stream_before_physics,
            world::drain_committed_ui_before_authority,
        )
            .chain()
            .before(ClientFrameSet::UiAuthority)
            .after(client_state::RuntimeFrameSet::PublicationBudget),
    )
    .add_systems(
        Update,
        (
            crate::ui_runtime::drain_inventory_authority,
            screen_focus,
            semantic_controls::synchronize_semantic_input_authority,
        )
            .chain()
            .in_set(ClientFrameSet::UiAuthority),
    )
    .add_systems(
        Update,
        world::advance_dimension_transfer
            .after(ClientFrameSet::UiAuthority)
            .before(ClientFrameSet::SemanticFinalize),
    )
    .add_systems(
        Update,
        semantic_controls::finalize_semantic_input_after_ui_authority
            .in_set(ClientFrameSet::SemanticFinalize),
    )
    .add_systems(
        Update,
        crate::hotbar::flush_hotbar_selection
            .in_set(client_state::ClientInteractionSet::HotbarSend)
            .after(ClientFrameSet::SemanticFinalize)
            .before(ClientFrameSet::ActorPreparation),
    )
    .add_systems(
        Update,
        crate::block_use::produce_block_use
            .after(ClientFrameSet::SemanticFinalize)
            .after(crate::hotbar::flush_hotbar_selection)
            .before(ClientFrameSet::Physics),
    )
    .add_systems(
        Update,
        crate::ui_runtime::presentation::dials::advance_dials
            .after(ClientFrameSet::Camera)
            .before(ClientFrameSet::ActorPreparation),
    )
    .add_systems(
        Update,
        (observe_mount_jump_input, prepare_ui_runtime)
            .chain()
            .in_set(ClientFrameSet::UiPreparation),
    )
    .add_systems(
        Update,
        publish_ui_runtime.in_set(ClientFrameSet::UiPublication),
    )
    .add_systems(
        Update,
        (
            world::camera_environment::update_camera_medium,
            crate::environment::update_atmosphere_frame,
            crate::environment::update_seasonal_foliage,
            crate::environment::update_precipitation_scene,
            crate::environment::update_lightning,
            crate::runtime::visibility::refresh_cave_visibility,
            crate::runtime::telemetry::update_visibility_diagnostics
                .after(render_scene::ChunkRenderApplySet),
        )
            .chain()
            .after(client_presentation::camera::FlyCameraUpdateSet),
    );
    crate::audio::configure(app);
    app.add_systems(
        Update,
        crate::ui_runtime::outbound::flush_chat_network
            .after(ClientFrameSet::UiAuthority)
            .before(ClientFrameSet::UiPreparation),
    );
    app.add_systems(
        Update,
        (
            crate::session_audio::drain_sequenced_audio_into_session,
            crate::named_audio::drain_live_named_audio,
        )
            .after(world::reconcile_world_stream_before_physics)
            .after(world::drive_world_stream),
    );
}
