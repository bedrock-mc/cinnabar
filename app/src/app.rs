use std::sync::Arc;

use anyhow::{Context, Result, bail};
use bevy::{
    anti_alias::{AntiAliasPlugin, fxaa::FxaaPlugin},
    app::TerminalCtrlCHandlerPlugin,
    prelude::{
        App, ClearColor, Color, DefaultPlugins, IntoScheduleConfigs, Last, PluginGroup, Update,
        Window, default,
    },
    render::diagnostic::RenderDiagnosticsPlugin,
    window::WindowPlugin,
};
use render::{
    ActorRenderPlugin, ActorRenderScene, AtmosphereFrame, AtmospherePlugin,
    AtmosphereTextureAssets, ChunkRenderApplySet, ChunkRenderPlugin, ChunkTextureAssets,
    VisibilityDiagnosticsInput,
};

use crate::acceptance::{
    markers::{SHUTDOWN_COMPLETED, requested_present_mode},
    world_ready::emit_world_ready,
};
use crate::{
    acceptance::{
        AcceptanceRun,
        model_witness::{ModelWitnessFileSource, poll_model_witness_request},
        transparent_witness::{TransparentWitnessFileSource, poll_transparent_witness_request},
    },
    args,
    asset_startup::{LoadedAssetKind, load_runtime_assets, select_asset_path_from_environment},
    camera::{FlyCameraPlugin, FlyCameraUpdateSet},
    environment::{self, WeatherState, WorldClock, update_atmosphere_frame},
    metrics::MetricsCollector,
    movement::MovementTicker,
    runtime::{
        endpoint::{preflight_bridge_endpoint, resolve_socket_dir},
        network::{
            NetworkConfig, NetworkHandle, publish_actor_render_frame, receive_network_events,
            spawn_network,
        },
        publication::{PublicationController, begin_publication_frame},
        shutdown::{
            exit_on_fatal_runtime_error, exit_on_window_close_requested, finish_acceptance_run,
        },
        telemetry::{
            AcceptanceRuntimeConfig, frame_limited_winit_settings, record_metrics_and_title,
            send_player_auth_inputs, update_visibility_diagnostics,
        },
        visibility::{
            AppMetrics, CaveVisibilityCache, DiagnosticQuads, apply_added_chunk_visibility,
            refresh_cave_visibility, remove_chunk_visibility,
        },
        world::{
            ClientWorld, SHUTDOWN_WATCHDOG_TIMEOUT, ShutdownWatchdog, app_exit_code,
            arm_shutdown_watchdog, drive_world_stream, startup_biome_tints, update_camera_medium,
        },
    },
};

use crate::acceptance::model_witness::drive_model_witness;

pub fn run(args: args::ClientArgs) -> Result<()> {
    let socket_dir = resolve_socket_dir(&args.socket_dir);
    preflight_bridge_endpoint(&socket_dir)?;

    let selected_assets = select_asset_path_from_environment(args.assets.as_deref());
    let loaded_assets =
        load_runtime_assets(selected_assets).context("load startup block assets")?;
    if let Some(notice) = &loaded_assets.notice {
        eprintln!("{notice}");
    } else if loaded_assets.kind == LoadedAssetKind::CompiledBlob {
        eprintln!(
            "loaded compiled block assets from {} (sha256 {})",
            loaded_assets.selected_path.display(),
            loaded_assets.metrics.blob_sha256
        );
    }
    eprintln!(
        "loaded required atmosphere assets from {}",
        loaded_assets.atmosphere.selected_path().display()
    );
    eprintln!("{}", loaded_assets.atmosphere.startup_summary());
    let (atmosphere_runtime, atmosphere_identity) = loaded_assets.atmosphere.into_parts();
    let runtime_assets = loaded_assets.runtime;
    let asset_metrics = loaded_assets.metrics;

    let network = spawn_network(NetworkConfig {
        socket_dir,
        display_name: args.display_name.clone(),
    })
    .context("spawn Bedrock network worker")?;
    let present_mode = requested_present_mode(args.no_vsync);
    let runtime_config = AcceptanceRuntimeConfig {
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    };
    let shutdown_watchdog = ShutdownWatchdog::process(SHUTDOWN_WATCHDOG_TIMEOUT);

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Rust MCBE | connecting".to_owned(),
                    present_mode,
                    ..default()
                }),
                ..default()
            })
            // Cinnabar uses FXAA without Bevy's TAA/SMAA/CAS bundle. The TAA
            // graph requires post-process nodes that are intentionally absent
            // from this compact custom renderer.
            .disable::<AntiAliasPlugin>()
            // The launcher owns the production process lifecycle. Keeping the
            // OS default SIGINT action also preserves a real developer escape
            // hatch if graceful Bevy teardown is wedged.
            .disable::<TerminalCtrlCHandlerPlugin>(),
    );
    app.add_plugins(FxaaPlugin);
    let diagnostics_enabled = args.acceptance_seconds.is_some() || args.metrics_out.is_some();
    if diagnostics_enabled {
        app.add_plugins(RenderDiagnosticsPlugin);
    }
    app.insert_resource(frame_limited_winit_settings(args.frame_cap))
        .insert_resource(ClearColor(Color::srgb(0.46, 0.70, 0.92)))
        .insert_resource(shutdown_watchdog.clone())
        .insert_resource(network)
        .insert_resource(ClientWorld::new(Arc::clone(&runtime_assets)))
        .insert_resource(WorldClock::default())
        .insert_resource(WeatherState::default())
        .insert_resource(environment::CameraMediumState::default())
        .insert_resource(MovementTicker::default())
        .insert_resource(ActorRenderScene::default())
        .insert_resource(AtmosphereFrame::default())
        .insert_resource(AtmosphereTextureAssets::new(
            atmosphere_runtime,
            atmosphere_identity,
        ))
        .insert_resource(startup_biome_tints(&runtime_assets))
        .insert_resource(ChunkTextureAssets::new(runtime_assets))
        .insert_resource(CaveVisibilityCache::default())
        .insert_resource(VisibilityDiagnosticsInput::new(diagnostics_enabled))
        .insert_resource(runtime_config)
        .insert_resource(AppMetrics(MetricsCollector::with_asset_metrics(
            asset_metrics,
        )))
        .insert_resource(DiagnosticQuads::default())
        .insert_resource(PublicationController::default())
        .insert_resource(TransparentWitnessFileSource::new(
            args.transparent_witness_request,
        ))
        .insert_resource(ModelWitnessFileSource::new(args.model_witness_request))
        .insert_resource(AcceptanceRun::new(
            args.acceptance_seconds,
            args.metrics_out,
            args.full_view_teleport_gate,
            args.require_transparent_presentation,
        ))
        .add_plugins((
            ActorRenderPlugin,
            AtmospherePlugin,
            ChunkRenderPlugin::with_budget(PublicationController::default().budget()),
            FlyCameraPlugin::new(args.auto_fly),
        ))
        .add_observer(apply_added_chunk_visibility)
        .add_observer(remove_chunk_visibility)
        .add_systems(
            Update,
            begin_publication_frame
                .before(ChunkRenderApplySet)
                .after(FlyCameraUpdateSet),
        )
        .add_systems(
            Update,
            (
                exit_on_window_close_requested,
                receive_network_events,
                exit_on_fatal_runtime_error,
                poll_transparent_witness_request,
                poll_model_witness_request,
                drive_world_stream.before(ChunkRenderApplySet),
                publish_actor_render_frame,
                update_camera_medium,
                update_atmosphere_frame,
                refresh_cave_visibility,
                update_visibility_diagnostics.after(ChunkRenderApplySet),
                emit_world_ready,
                drive_model_witness,
                record_metrics_and_title,
                finish_acceptance_run,
            )
                .chain()
                .after(FlyCameraUpdateSet),
        )
        .add_systems(
            Last,
            (send_player_auth_inputs, arm_shutdown_watchdog).chain(),
        );

    let exit = app.run();
    shutdown_watchdog.complete();
    eprintln!("{SHUTDOWN_COMPLETED} exit_code={}", app_exit_code(&exit));
    if let Some(mut network) = app.world_mut().remove_resource::<NetworkHandle>() {
        network.shutdown();
    }
    if exit.is_error() {
        bail!("Bevy app exited after a fatal runtime error");
    }
    Ok(())
}
