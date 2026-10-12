#[cfg(not(feature = "acceptance"))]
use crate::acceptance::AcceptanceRun;
use crate::runtime::network::resource_packs::PackApplication;
#[cfg(feature = "acceptance")]
use crate::runtime::phase3_evidence::{
    Phase3EvidenceEmitter, Phase3EvidenceIdentitySource, emit_phase3_evidence,
};
#[cfg(feature = "acceptance")]
use crate::runtime::shutdown::finish_acceptance_run;
#[cfg(feature = "acceptance")]
use acceptance::model_witness::poll_model_witness_request;
use anyhow::{Context, Result, bail};
use assets::carriers::startup::{LoadedAssetKind, select_asset_path_from_environment};
#[cfg(not(target_arch = "wasm32"))]
use bevy::app::TerminalCtrlCHandlerPlugin;
use bevy::{
    anti_alias::AntiAliasPlugin,
    prelude::{
        App, ClearColor, Color, DefaultPlugins, First, IntoScheduleConfigs, Last, PluginGroup,
        Update, default,
    },
    render::diagnostic::RenderDiagnosticsPlugin,
    window::WindowPlugin,
};
use client_runtime::blob_cache::ClientBlobCacheOwner;
use client_runtime::network::NetworkHandle;
use client_state::ClientFrameSet;
use launcher_host::core_process::{CoreProcessGuard, spawn_core_for_address, wait_for_core};
use launcher_host::session_cleanup::reclaim_stale_session_directories;
use render_api::PublicationServiceConfig;
#[cfg(test)]
use std::fs;
use std::io::Write;
use std::{ffi::OsStr, path::Path, sync::Arc};
#[cfg(feature = "acceptance")]
use {
    crate::acceptance::{model_witness::drive_model_witness, world_ready::emit_world_ready},
    acceptance::transparent_witness::poll_transparent_witness_request,
};
use {
    render::{ActorRenderPlugin, AtmospherePlugin, ChunkRenderPlugin, UiRenderPlugin},
    render_scene::{
        ActorRenderScene, AtmosphereFrame, AtmosphereTextureAssets, ChunkRenderApplySet,
        ChunkTextureAssets, RuntimeStageProfiler, VisibilityDiagnosticsInput,
    },
};
mod logging;
mod startup;
use startup::bind_direct_session_directory;

use crate::runtime::telemetry::AcceptanceRuntimeConfig;
use client_presentation::{
    actor_publication::publish_actor_render_frame,
    camera::FlyCameraUpdateSet,
    local_player::{LocalPlayerFrameSet, publish_interaction_origin},
};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::{UiRuntime, oreui_fonts, presentation::UiPresentationRuntime};
use diagnostics::markers::SHUTDOWN_COMPLETED;
use diagnostics::metrics::MetricsCollector;
use launcher::install_layout::InstallLayout;
use {
    crate::{
        args,
        block_use::produce_block_use,
        camera::FlyCameraPlugin,
        environment::{
            self, EnvironmentContext, EnvironmentProfileRoute, WeatherState, WorldClock,
            update_atmosphere_frame, update_lightning, update_precipitation_scene,
            update_seasonal_foliage,
        },
        local_player::{publish_local_player_frame, resolve_camera_pose},
        melee::produce_melee,
        movement::{advance_local_physics, send_movement_prediction_sync},
        present_mode::{PresentModeRuntime, apply_present_mode},
        runtime::{
            network::{
                advance_actor_frame, prepare_actor_render_frame, publish_entity_shadows,
                receive_network_events,
                session::{NetworkConfig, spawn_network},
            },
            shutdown::{exit_on_fatal_runtime_error, exit_on_window_close_requested},
            telemetry::{
                publish_runtime_stage_profile, record_metrics, send_player_auth_inputs,
                update_visibility_diagnostics,
            },
            visibility::{
                AppMetrics, CaveVisibilityCache, DiagnosticQuads, apply_added_chunk_visibility,
                refresh_cave_visibility, remove_chunk_visibility,
            },
            world::{
                ClientWorld, WorldStreamFramePoll, drive_world_stream,
                reconcile_world_stream_before_physics, startup_biome_tints, update_camera_medium,
            },
        },
        semantic_controls::{
            collect_raw_input, finalize_semantic_input_after_ui_authority, route_semantic_input,
            synchronize_semantic_input_authority,
        },
        session::{
            SessionController, drive_session, follow_server_transfer, recover_session_failure,
        },
        shell_adapters::drive_menu_services,
        survival_mining::produce_survival_mining,
        ui_runtime::{
            drain_inventory_authority, drive_chat_keyboard_input, drive_chat_ui_actions,
            drive_inventory_ui_actions, drive_server_form_input, drive_sign_editor,
            drive_world_inventory_keys, flush_chat_network, flush_inventory_network,
            flush_server_form_network,
            gameplay_touch::drive_gameplay_touch_targets,
            presentation::{
                drive_menu_panorama, observe_mount_jump_input, prepare_ui_runtime,
                publish_ui_runtime,
            },
        },
    },
    client_interaction::movement::{
        LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
        PhysicsAuthorityGate,
    },
    client_runtime::{
        endpoint::{preflight_bridge_endpoint, resolve_socket_dir},
        pack_admission::ResourcePackAdmissionState,
        publication::PublicationController,
        shutdown::{
            SHUTDOWN_WATCHDOG_TIMEOUT, ShutdownWatchdog, TeardownWatchdog, app_exit_code,
            arm_shutdown_watchdog,
        },
    },
    client_shell::menu::MenuRuntime,
};

mod launch;
mod render_setup;
pub use launch::{CoreLink, run, run_with};
use render_setup::render_plugin;

const PHYSICS_REGISTRY_SHA256: &str =
    include_str!("../../crates/assets/data/block-physics-v2193.sha256");
const PHYSICS_REGISTRY_GENERATION_GUIDANCE: &str =
    "run `make assets` (normal `make client` does this automatically)";

mod authority;
mod executor;
pub(crate) use authority::{configure_client_authority_systems, configure_client_frame_schedule};

/// Registers the production actor observation and publication boundaries.
pub(crate) fn configure_actor_render_systems(app: &mut App) {
    app.init_resource::<client_presentation::actor_publication::ActorFrameState>()
        .add_systems(
            Update,
            (
                advance_actor_frame,
                crate::runtime::network::publish_local_actor_damage,
            )
                .chain()
                .in_set(ClientFrameSet::ActorPreparation),
        )
        // Picks in Interaction and NetworkSend read this frame's remote actor positions.
        .add_systems(
            Update,
            crate::runtime::network::advance_actor_motion
                .after(ClientFrameSet::Camera)
                .before(ClientFrameSet::Interaction),
        )
        .add_systems(
            Update,
            prepare_actor_render_frame.in_set(ClientFrameSet::ActorFinalization),
        )
        .add_systems(
            Update,
            (publish_actor_render_frame, publish_entity_shadows)
                .chain()
                .in_set(ClientFrameSet::ActorPublication),
        );
}

pub(crate) fn configure_client_production_frame_systems(app: &mut App) {
    #[cfg(not(feature = "acceptance"))]
    app.init_resource::<AcceptanceRun>();
    #[cfg(feature = "acceptance")]
    app.init_resource::<Phase3EvidenceEmitter>();
    app.init_resource::<crate::runtime::network::PackReload>();
    configure_client_authority_systems(app);
    configure_actor_render_systems(app);
    crate::audio::configure(app);
    app.add_plugins(client_interaction::ClientInteractionPlugin)
        .init_resource::<client_presentation::server_camera::ServerCameraInstructions>()
        .init_resource::<client_presentation::local_player_camera_receipt::CameraPublicationAttempt>()
        .add_systems(
            Update,
            receive_network_events
                .before(drive_server_form_input)
                .before(drain_inventory_authority)
                .before(ClientFrameSet::Physics),
        )
        // The session-audio reader consumes exactly what the world-stream
        // writer above produced. It also owns teardown of retained outcomes,
        // so it must observe every system that can remove or replace the
        // active stream in this frame.
        .add_systems(
            Update,
            crate::session_audio::drain_sequenced_audio_into_session
                .after(reconcile_world_stream_before_physics)
                .after(drive_session)
                .after(follow_server_transfer)
                .after(recover_session_failure),
        )
        .add_systems(
            Update,
            advance_local_physics
                .in_set(LocalPlayerFrameSet::Physics)
                .in_set(ClientFrameSet::Physics),
        )
        .add_systems(
            Update,
            send_movement_prediction_sync
                .after(advance_local_physics)
                .in_set(ClientFrameSet::NetworkSend),
        )
        .add_systems(
            Update,
            (
                client_presentation::local_player_camera_receipt::begin_camera_publication_attempt,
                resolve_camera_pose,
            )
                .chain()
                .in_set(LocalPlayerFrameSet::Camera)
                .in_set(ClientFrameSet::Camera),
        )
        .add_systems(
            Update,
            (publish_local_player_frame, publish_interaction_origin, crate::camera::aim_assist::publish_assisted_interaction, crate::camera::aim_highlight::publish)
                .chain()
                .before(client_state::ClientInteractionSet::RetainPick)
                .in_set(LocalPlayerFrameSet::Interaction)
                .in_set(ClientFrameSet::Interaction),
        )
        .add_systems(
            Update,
            drive_world_stream
                .after(receive_network_events)
                .before(ChunkRenderApplySet)
                .in_set(ClientFrameSet::WorldPublication),
        )
        .add_systems(
            Update,
            crate::named_audio::drain_live_named_audio
                .after(publish_local_player_frame)
                .after(drive_world_stream)
                .after(reconcile_world_stream_before_physics)
                .after(drive_session)
                .after(follow_server_transfer)
                .after(recover_session_failure),
        )
        .add_systems(
            Update,
            crate::hotbar::flush_hotbar_selection
                .in_set(client_state::ClientInteractionSet::HotbarSend)
                .after(ClientFrameSet::SemanticFinalize)
                .before(ClientFrameSet::ActorPreparation),
        )
        // Build actions resolve before the tick they precede, so its movement sees the block.
        .add_systems(
            Update,
            produce_block_use
                .after(ClientFrameSet::SemanticFinalize)
                .after(crate::hotbar::flush_hotbar_selection)
                .after(reconcile_world_stream_before_physics)
                .before(ClientFrameSet::Physics),
        )
        .add_systems(
            Update,
            (observe_mount_jump_input, prepare_ui_runtime)
                .chain()
                .in_set(ClientFrameSet::UiPreparation),
        )
        .add_systems(
            Update,
            (publish_ui_runtime, drive_menu_panorama)
                .chain()
                .in_set(ClientFrameSet::UiPublication),
        )
        .add_systems(
            Update,
            (
                flush_inventory_network,
                #[cfg(feature = "acceptance")]
                emit_phase3_evidence,
                #[cfg(not(feature = "acceptance"))]
                crate::runtime::telemetry::discard_completed_movement_evidence,
                produce_melee,
                produce_survival_mining,
                crate::item_use::produce_item_use,
                #[cfg(feature = "tracy")]
                crate::tracy::plot_physics_to_send,
                send_player_auth_inputs,
                crate::pick_block::produce_pick_block,
            )
                .chain()
                .in_set(ClientFrameSet::NetworkSend),
        );
    #[cfg(feature = "tracy")]
    app.init_resource::<crate::tracy::PhysicsEnd>().add_systems(
        Update,
        crate::tracy::mark_physics_end
            .after(advance_local_physics)
            .in_set(ClientFrameSet::Physics),
    );
}

pub(crate) fn configure_acceptance_finish_system(app: &mut App) {
    #[cfg(feature = "acceptance")]
    app.add_systems(
        Update,
        finish_acceptance_run
            .after(ClientFrameSet::NetworkSend)
            .after(ClientFrameSet::UiPublication)
            .after(record_metrics)
            .after(recover_session_failure),
    );
    app
        // The launcher gets first refusal on a fatal session error, so a failed
        // join returns to the menu instead of ending the process. This has to sit
        // after the failure is recorded (network drain) and before both systems
        // that act on it. The transfer follower runs first so a server-directed
        // move is classified as a replacement handoff, not a failure.
        .add_systems(
            Update,
            (follow_server_transfer, recover_session_failure)
                .chain()
                .after(receive_network_events)
                .after(ClientFrameSet::NetworkSend)
                .after(ClientFrameSet::UiPublication)
                .before(exit_on_fatal_runtime_error),
        );
}

pub(crate) fn configure_client_runtime_frame_systems(app: &mut App) {
    app.add_plugins(client_runtime::ClientRuntimePlugin::<PackApplication>::default());
    crate::runtime::world::configure_world_service(app);
    app.add_observer(apply_added_chunk_visibility)
        .add_observer(remove_chunk_visibility)
        .configure_sets(
            Update,
            (
                LocalPlayerFrameSet::Physics,
                LocalPlayerFrameSet::Camera,
                LocalPlayerFrameSet::Interaction,
            )
                .chain()
                .after(FlyCameraUpdateSet),
        )
        .configure_sets(
            Update,
            client_state::RuntimeFrameSet::PublicationBudget
                .before(receive_network_events)
                .before(drive_world_stream),
        )
        .add_systems(
            Update,
            (
                exit_on_window_close_requested,
                flush_chat_network.before(ClientFrameSet::UiPreparation),
                flush_server_form_network.in_set(ClientFrameSet::NetworkSend),
                exit_on_fatal_runtime_error,
                #[cfg(feature = "acceptance")]
                poll_transparent_witness_request,
                #[cfg(feature = "acceptance")]
                poll_model_witness_request,
                update_camera_medium,
                update_atmosphere_frame,
                update_seasonal_foliage,
                crate::environment::log_world_lighting,
                update_precipitation_scene,
                update_lightning,
                refresh_cave_visibility,
                update_visibility_diagnostics.after(ChunkRenderApplySet),
                #[cfg(feature = "acceptance")]
                emit_world_ready,
                #[cfg(feature = "acceptance")]
                drive_model_witness,
                apply_present_mode,
                record_metrics,
                publish_runtime_stage_profile,
            )
                .chain()
                .after(FlyCameraUpdateSet),
        );
}

#[cfg(test)]
mod direct_session_directory_tests;

#[cfg(test)]
mod preg_startup_tests;
#[cfg(test)]
mod schedule_tests;
