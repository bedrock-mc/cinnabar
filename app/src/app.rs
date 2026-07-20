use std::{ffi::OsStr, fs, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use bevy::{
    anti_alias::{AntiAliasPlugin, fxaa::FxaaPlugin},
    app::TerminalCtrlCHandlerPlugin,
    prelude::{
        App, ClearColor, Color, DefaultPlugins, IntoScheduleConfigs, Last, PluginGroup, SystemSet,
        Update, Window, default,
    },
    render::{
        RenderPlugin,
        diagnostic::RenderDiagnosticsPlugin,
        settings::{Backends, RenderCreation, WgpuSettings},
    },
    window::WindowPlugin,
};
use client_world::PublicationServiceConfig;
use render::{
    ActorRenderPlugin, ActorRenderScene, AtmosphereFrame, AtmospherePlugin,
    AtmosphereTextureAssets, ChunkRenderApplySet, ChunkRenderPlugin, ChunkTextureAssets,
    UiRenderPlugin, VisibilityDiagnosticsInput,
};
use sha2::{Digest, Sha256};

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
    asset_startup::{
        LoadedAssetKind, load_runtime_assets, require_hud_assets,
        select_asset_path_from_environment,
    },
    camera::{FlyCameraPlugin, FlyCameraUpdateSet},
    environment::{
        self, EnvironmentContext, EnvironmentProfileRoute, WeatherState, WorldClock,
        update_atmosphere_frame,
    },
    local_player::{
        LocalPlayerFrameSet, publish_interaction_origin, publish_local_player_frame,
        resolve_camera_pose,
    },
    metrics::MetricsCollector,
    movement::{
        LocalPhysicsController, MovementTicker, PhysicsAuthorityGate, PhysicsCollisionRegistries,
        advance_local_physics,
    },
    present_mode::{PresentModeRuntime, apply_runtime_vsync_setting},
    runtime::{
        endpoint::{preflight_bridge_endpoint, resolve_socket_dir},
        network::{
            NetworkConfig, NetworkHandle, publish_actor_render_frame, receive_network_events,
            spawn_network,
        },
        phase3_evidence::{
            Phase3EvidenceEmitter, Phase3EvidenceIdentitySource, emit_phase3_evidence,
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
            ClientWorld, SHUTDOWN_WATCHDOG_TIMEOUT, ShutdownWatchdog, WorldStreamFramePoll,
            app_exit_code, arm_shutdown_watchdog, drive_world_stream,
            reconcile_world_stream_before_physics, startup_biome_tints, update_camera_medium,
        },
    },
    semantic_controls::{
        collect_raw_input, finalize_semantic_input_after_ui_authority, route_semantic_input,
        synchronize_semantic_input_authority,
    },
    ui_runtime::{
        UiRuntime, drive_chat_keyboard_input, drive_chat_ui_actions, flush_chat_network,
        gameplay_touch::drive_gameplay_touch_targets,
        presentation::{UiPresentationRuntime, publish_ui_runtime},
    },
};

use crate::acceptance::model_witness::drive_model_witness;

const PHYSICS_REGISTRY_PATH: &str = ".local/assets/block-physics-v1001.bin";
const PHYSICS_REGISTRY_SHA256: &str =
    include_str!("../../crates/assets/data/block-physics-v1001.sha256");
const PHYSICS_REGISTRY_GENERATION_GUIDANCE: &str =
    "run `make physics-assets` (normal `make client` does this automatically)";

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ClientFrameSet {
    RawInput,
    SemanticSample,
    UiAuthority,
    SemanticFinalize,
    Physics,
    Camera,
    Interaction,
    WorldPublication,
    ActorPublication,
    UiPublication,
    NetworkSend,
}

pub(crate) fn configure_client_frame_schedule(app: &mut App) {
    app.configure_sets(
        Update,
        (
            ClientFrameSet::RawInput,
            ClientFrameSet::SemanticSample,
            ClientFrameSet::UiAuthority,
            ClientFrameSet::SemanticFinalize,
            ClientFrameSet::Physics,
            ClientFrameSet::Camera,
            ClientFrameSet::Interaction,
            ClientFrameSet::WorldPublication,
            ClientFrameSet::ActorPublication,
            ClientFrameSet::UiPublication,
            ClientFrameSet::NetworkSend,
        )
            .chain(),
    );
}

pub(crate) fn configure_client_production_frame_systems(app: &mut App) {
    app.init_resource::<WorldStreamFramePoll>()
        .init_resource::<Phase3EvidenceEmitter>()
        .add_systems(
            Update,
            (drive_gameplay_touch_targets, collect_raw_input)
                .chain()
                .in_set(ClientFrameSet::RawInput),
        )
        .add_systems(
            Update,
            route_semantic_input.in_set(ClientFrameSet::SemanticSample),
        )
        .add_systems(
            Update,
            (
                drive_chat_ui_actions,
                drive_chat_keyboard_input,
                synchronize_semantic_input_authority,
            )
                .chain()
                .in_set(ClientFrameSet::UiAuthority),
        )
        .add_systems(
            Update,
            finalize_semantic_input_after_ui_authority.in_set(ClientFrameSet::SemanticFinalize),
        )
        .add_systems(
            Update,
            (
                receive_network_events,
                reconcile_world_stream_before_physics,
            )
                .chain()
                .before(ClientFrameSet::Physics),
        )
        .add_systems(
            Update,
            advance_local_physics
                .in_set(LocalPlayerFrameSet::Physics)
                .in_set(ClientFrameSet::Physics),
        )
        .add_systems(
            Update,
            resolve_camera_pose
                .in_set(LocalPlayerFrameSet::Camera)
                .in_set(ClientFrameSet::Camera),
        )
        .add_systems(
            Update,
            (publish_local_player_frame, publish_interaction_origin)
                .chain()
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
            publish_actor_render_frame.in_set(ClientFrameSet::ActorPublication),
        )
        .add_systems(
            Update,
            crate::hotbar::select_hotbar_slot
                .after(ClientFrameSet::SemanticFinalize)
                .before(ClientFrameSet::UiPublication),
        )
        .add_systems(
            Update,
            publish_ui_runtime.in_set(ClientFrameSet::UiPublication),
        )
        .add_systems(
            Update,
            (emit_phase3_evidence, send_player_auth_inputs)
                .chain()
                .in_set(ClientFrameSet::NetworkSend),
        );
}

pub(crate) fn configure_acceptance_finish_system(app: &mut App) {
    app.add_systems(
        Update,
        finish_acceptance_run
            .after(ClientFrameSet::NetworkSend)
            .after(record_metrics_and_title),
    );
}

fn read_verified_physics_registry(path: &Path, expected_sha256: &str) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| {
        format!(
            "read required protocol-1001 physics registry {}; {}",
            path.display(),
            PHYSICS_REGISTRY_GENERATION_GUIDANCE
        )
    })?;
    let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let expected_sha256 = expected_sha256.trim();
    if actual_sha256 != expected_sha256 {
        bail!(
            "protocol-1001 physics registry {} is stale or corrupt: expected sha256 {}, got {}; {}",
            path.display(),
            expected_sha256,
            actual_sha256,
            PHYSICS_REGISTRY_GENERATION_GUIDANCE
        );
    }
    Ok(bytes)
}

pub(crate) fn preferred_render_backends(explicit: Option<&OsStr>) -> Option<Backends> {
    if explicit.is_some() {
        return None;
    }
    #[cfg(target_os = "windows")]
    {
        Some(Backends::DX12)
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

fn render_plugin() -> RenderPlugin {
    let mut settings = WgpuSettings::default();
    if let Some(backends) = preferred_render_backends(std::env::var_os("WGPU_BACKEND").as_deref()) {
        settings.backends = Some(backends);
    }
    RenderPlugin {
        render_creation: RenderCreation::Automatic(settings),
        ..default()
    }
}

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
    eprintln!(
        "loaded required entity assets from {}",
        loaded_assets.entities.selected_path().display()
    );
    eprintln!("{}", loaded_assets.entities.startup_summary());
    eprintln!("{}", loaded_assets.fonts.startup_summary());
    let entity_runtime = Arc::clone(loaded_assets.entities.runtime());
    let hud_assets = require_hud_assets(&loaded_assets.selected_path)
        .context("load pinned official Mojang sample HUD carrier")?;
    eprintln!("{}", hud_assets.startup_summary());
    let lang_assets = crate::asset_startup::require_lang_assets(
        &loaded_assets.selected_path,
        crate::asset_startup::vanilla_source_manifest_json(),
    )
    .context("load pinned official Mojang sample localization carrier")?;
    eprintln!("{}", lang_assets.startup_summary());
    let font_runtime = loaded_assets.fonts.into_runtime();
    let mut ui_presentation =
        UiPresentationRuntime::with_hud(font_runtime, hud_assets.into_runtime())
            .context("prepare bounded font and HUD texture array for UI rendering")?;
    // Hybrid HUD: Bedrock has no static scoreboard background alpha (it is a runtime engine
    // binding), so bind Java Edition's sidebar opacities. The sidebar still shows only when the
    // server publishes a sidebar objective.
    ui_presentation.enable_scoreboard_background();
    ui_presentation.set_gui_scale_preference(args.gui_scale);
    ui_presentation.set_safe_area(crate::ui_runtime::presentation::platform_safe_area_insets());
    let (atmosphere_runtime, atmosphere_identity) = loaded_assets.atmosphere.into_parts();
    let runtime_assets = loaded_assets.runtime;
    let asset_metrics = loaded_assets.metrics;
    let actor_render_scene = ActorRenderScene::with_runtime_entity_assets(&entity_runtime)
        .map_err(|error| {
            anyhow::anyhow!(
                "prepare validated runtime entity geometry for actor rendering: {error:?}"
            )
        })?;
    let collision_breg = include_bytes!("../../crates/assets/data/block-registry-v1001.bin");
    let collision_records = assets::read_registry(collision_breg)
        .context("decode checked-in protocol-1001 collision registry")?;
    let collision_preg =
        read_verified_physics_registry(Path::new(PHYSICS_REGISTRY_PATH), PHYSICS_REGISTRY_SHA256)?;
    let collision_registries = PhysicsCollisionRegistries::from_assets(
        collision_breg,
        &collision_records,
        &collision_preg,
    )
    .context("decode and bind protocol-1001 PREG collision registries")?;
    eprintln!(
        "loaded {} authoritative collision records for local physics",
        collision_registries.available_record_count()
    );
    let phase3_identity_source = args
        .phase3_evidence_target
        .map(|target| {
            Phase3EvidenceIdentitySource::from_build(
                target,
                args.phase3_candidate_physics,
                &collision_registries,
            )
        })
        .transpose()
        .context("bind Phase 3 evidence to this exact build and collision registry")?;

    let network = spawn_network(NetworkConfig {
        session_generation: 1,
        socket_dir,
        display_name: args.display_name.clone(),
        client_blob_cache: protocol::ClientBlobCache::default(),
    })
    .context("spawn Bedrock network worker")?;
    let present_mode = requested_present_mode(args.no_vsync);
    let diagnostics_enabled = args.acceptance_seconds.is_some() || args.metrics_out.is_some();
    let present_mode_runtime =
        PresentModeRuntime::from_startup(args.force_vsync, args.no_vsync, diagnostics_enabled);
    let present_mode_policy = present_mode_runtime.policy();
    let runtime_config = AcceptanceRuntimeConfig {
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    };
    let shutdown_watchdog = ShutdownWatchdog::process(SHUTDOWN_WATCHDOG_TIMEOUT);

    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
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
            .set(render_plugin())
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
    app.add_plugins(render::Dx12PresentModePolicyPlugin::new(
        present_mode_policy,
    ));
    if diagnostics_enabled {
        app.add_plugins(RenderDiagnosticsPlugin);
    }
    app.insert_resource(frame_limited_winit_settings(args.frame_cap))
        .insert_resource(ClearColor(Color::srgb(0.46, 0.70, 0.92)))
        .insert_resource(shutdown_watchdog.clone())
        .insert_resource(present_mode_runtime)
        .insert_resource(network)
        .insert_resource(ClientWorld::new_with_entity_assets(
            Arc::clone(&runtime_assets),
            entity_runtime,
        ))
        .insert_resource({
            let mut ui_runtime = UiRuntime::new(0);
            ui_runtime.set_lang_catalog(lang_assets.into_runtime());
            ui_runtime
        })
        .insert_resource(ui_presentation)
        .insert_resource(WorldClock::default())
        .insert_resource(WeatherState::default())
        .insert_resource(environment::CameraMediumState::default())
        .insert_resource(EnvironmentContext::default())
        .insert_resource(EnvironmentProfileRoute::default())
        .insert_resource(MovementTicker::default())
        .insert_resource(if args.phase3_candidate_physics {
            PhysicsAuthorityGate::CandidateEvidence
        } else {
            PhysicsAuthorityGate::ProductionDisabled
        })
        .insert_resource(LocalPhysicsController::default())
        .insert_resource(collision_registries)
        .insert_resource(actor_render_scene)
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
        .insert_resource(AppMetrics(
            if let Some(sample_seconds) = args.metrics_sample_seconds {
                MetricsCollector::with_asset_metrics_window(
                    asset_metrics,
                    std::time::Duration::from_secs(args.metrics_warmup_seconds),
                    std::time::Duration::from_secs(sample_seconds),
                )
            } else {
                MetricsCollector::with_asset_metrics_and_warmup(
                    asset_metrics,
                    std::time::Duration::from_secs(args.metrics_warmup_seconds),
                )
            },
        ))
        .insert_resource(DiagnosticQuads::default())
        .insert_resource(PublicationController::new(
            PublicationServiceConfig::PHASE2_GATE,
        ))
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
            ChunkRenderPlugin::with_budget(
                PublicationController::new(PublicationServiceConfig::PHASE2_GATE).budget(),
            ),
            FlyCameraPlugin::new(args.auto_fly),
            UiRenderPlugin,
        ));
    if let Some(identity) = phase3_identity_source {
        app.insert_resource(identity);
    }
    configure_client_production_frame_systems(&mut app);
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
        .add_systems(
            Update,
            begin_publication_frame
                .before(receive_network_events)
                .before(drive_world_stream)
                .before(ChunkRenderApplySet)
                .after(FlyCameraUpdateSet),
        )
        .add_systems(
            Update,
            (
                exit_on_window_close_requested,
                flush_chat_network,
                exit_on_fatal_runtime_error,
                poll_transparent_witness_request,
                poll_model_witness_request,
                update_camera_medium,
                update_atmosphere_frame,
                refresh_cave_visibility,
                update_visibility_diagnostics.after(ChunkRenderApplySet),
                emit_world_ready,
                drive_model_witness,
                apply_runtime_vsync_setting,
                record_metrics_and_title,
            )
                .chain()
                .after(FlyCameraUpdateSet),
        )
        .add_systems(Last, arm_shutdown_watchdog);
    configure_acceptance_finish_system(&mut app);

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

#[cfg(test)]
mod preg_startup_tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temporary_path(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rust-mcbe-{label}-{}-{nonce}.bin",
            std::process::id()
        ))
    }

    #[test]
    fn verified_physics_registry_accepts_exact_digest() {
        let path = temporary_path("preg-valid");
        fs::write(&path, b"PREG test carrier").expect("write fixture");
        let expected = format!("{:x}", Sha256::digest(b"PREG test carrier"));

        let result = read_verified_physics_registry(&path, &format!("{expected}\n"));
        fs::remove_file(path).expect("remove fixture");

        assert_eq!(result.expect("valid digest"), b"PREG test carrier");
    }

    #[test]
    fn verified_physics_registry_rejects_stale_carrier_with_guidance() {
        let path = temporary_path("preg-stale");
        fs::write(&path, b"stale PREG test carrier").expect("write fixture");

        let error = read_verified_physics_registry(&path, &"0".repeat(64))
            .expect_err("stale digest must fail");
        fs::remove_file(path).expect("remove fixture");
        let message = format!("{error:#}");

        assert!(message.contains("stale or corrupt"));
        assert!(message.contains("make physics-assets"));
        assert!(message.contains("make client"));
    }

    #[test]
    fn missing_physics_registry_reports_acquisition_guidance() {
        let path = temporary_path("preg-missing");
        let error = read_verified_physics_registry(&path, &"0".repeat(64))
            .expect_err("missing carrier must fail");
        let message = format!("{error:#}");

        assert!(message.contains("read required protocol-1001 physics registry"));
        assert!(message.contains("make physics-assets"));
        assert!(message.contains("make client"));
    }
}
