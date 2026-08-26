use std::{ffi::OsStr, fs, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use bevy::{
    anti_alias::{AntiAliasPlugin, fxaa::FxaaPlugin},
    app::TerminalCtrlCHandlerPlugin,
    prelude::{
        App, ClearColor, Color, DefaultPlugins, IntoScheduleConfigs, Last, PluginGroup, Resource,
        SystemSet, Update, Window, default,
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
    RuntimeStageProfiler, UiRenderPlugin, VisibilityDiagnosticsInput,
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
        LoadedAssetKind, load_runtime_assets, require_hud_assets, require_icon_assets,
        select_asset_path_from_environment,
    },
    camera::{FlyCameraPlugin, FlyCameraUpdateSet},
    environment::{
        self, EnvironmentContext, EnvironmentProfileRoute, WeatherState, WorldClock,
        update_atmosphere_frame,
    },
    install_layout::InstallLayout,
    local_player::{
        LocalPlayerFrameSet, publish_interaction_origin, publish_local_player_frame,
        resolve_camera_pose,
    },
    menu::{
        CoreProcessGuard, MenuRuntime, drive_menu_connection, drive_menu_input,
        follow_server_transfer, recover_menu_session_failure, spawn_core_for_address,
        wait_for_core,
    },
    metrics::MetricsCollector,
    movement::{
        LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
        PhysicsAuthorityGate, PhysicsCollisionRegistries, advance_local_physics,
    },
    present_mode::{PresentModeRuntime, apply_runtime_vsync_setting},
    runtime::{
        endpoint::{preflight_bridge_endpoint, resolve_socket_dir},
        network::{
            NetworkConfig, NetworkHandle, ResourcePackAdmissionState, publish_actor_render_frame,
            receive_network_events, spawn_network,
        },
        phase3_evidence::{
            Phase3EvidenceEmitter, Phase3EvidenceIdentitySource, emit_phase3_evidence,
        },
        publication::{PublicationController, begin_publication_frame},
        shutdown::{
            exit_on_fatal_runtime_error, exit_on_window_close_requested, finish_acceptance_run,
        },
        telemetry::{
            AcceptanceRuntimeConfig, frame_limited_winit_settings, publish_runtime_stage_profile,
            record_metrics_and_title, send_player_auth_inputs, update_visibility_diagnostics,
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
    session_cleanup::{ScopedSessionDirectory, reclaim_stale_session_directories},
    ui_runtime::{
        UiRuntime, drain_inventory_authority, drive_chat_keyboard_input, drive_chat_ui_actions,
        drive_inventory_ui_actions, flush_chat_network, flush_inventory_network,
        gameplay_touch::drive_gameplay_touch_targets,
        presentation::{UiPresentationRuntime, observe_mount_jump_input, publish_ui_runtime},
    },
};

use crate::acceptance::model_witness::drive_model_witness;

const PHYSICS_REGISTRY_SHA256: &str =
    include_str!("../../crates/assets/data/block-physics-v1001.sha256");
const PHYSICS_REGISTRY_GENERATION_GUIDANCE: &str =
    "run `make physics-assets` (normal `make client` does this automatically)";

#[derive(Debug, Clone, Default, Resource)]
pub(crate) struct ClientBlobCacheOwner(protocol::ClientBlobCache);

impl ClientBlobCacheOwner {
    /// Returns a shared handle to the process-lifetime verified blob cache.
    pub(crate) fn cache(&self) -> protocol::ClientBlobCache {
        self.0.clone()
    }

    /// Whether cores spawned for sessions backed by this owner may advertise
    /// upstream client-cache capability (`-upstream-client-cache`). True
    /// because every session receives [`Self::cache`] and answers
    /// LoginSuccess with cache-enabled status downstream, and the two
    /// advertisements must stay coupled; a future cache-less flow must report
    /// `false` here so the core keeps its default-disabled wire bytes.
    pub(crate) fn enables_upstream_client_cache(&self) -> bool {
        true
    }
}

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
    app.add_message::<crate::runtime::audio::SequencedAudioEvent>()
        .init_resource::<WorldStreamFramePoll>()
        .init_resource::<Phase3EvidenceEmitter>()
        .init_resource::<crate::server_camera::ServerCameraInstructions>()
        .init_resource::<crate::session_audio::SessionAudio>()
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
                drain_inventory_authority,
                drive_chat_keyboard_input,
                drive_menu_input,
                drive_inventory_ui_actions,
                drive_menu_connection,
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
            receive_network_events
                .before(drain_inventory_authority)
                .before(ClientFrameSet::Physics),
        )
        .add_systems(
            Update,
            reconcile_world_stream_before_physics
                .after(receive_network_events)
                .before(ClientFrameSet::Physics),
        )
        // The session-audio reader consumes exactly what the world-stream
        // writer above produced, so it must order after that writer.
        .add_systems(
            Update,
            crate::session_audio::drain_sequenced_audio_into_session
                .after(reconcile_world_stream_before_physics),
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
            (observe_mount_jump_input, publish_ui_runtime)
                .chain()
                .in_set(ClientFrameSet::UiPublication),
        )
        .add_systems(
            Update,
            (
                flush_inventory_network,
                emit_phase3_evidence,
                send_player_auth_inputs,
            )
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
    )
    // The launcher gets first refusal on a fatal session error, so a failed
    // join returns to the menu instead of ending the process. This has to sit
    // after the failure is recorded (network drain) and before both systems
    // that act on it. The transfer follower runs first so a server-directed
    // move is classified as a replacement handoff, not a failure.
    .add_systems(
        Update,
        (follow_server_transfer, recover_menu_session_failure)
            .chain()
            .after(receive_network_events)
            .before(exit_on_fatal_runtime_error)
            .before(finish_acceptance_run),
    );
}

pub(crate) fn configure_client_runtime_frame_systems(app: &mut App) {
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
                .before(ChunkRenderApplySet),
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
                publish_runtime_stage_profile,
            )
                .chain()
                .after(FlyCameraUpdateSet),
        )
        .add_systems(Last, arm_shutdown_watchdog);
}

fn read_verified_physics_registry(
    path: &Path,
    expected_sha256: &str,
    expected_protocol: u32,
) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| {
        format!(
            "read required protocol-{expected_protocol} physics registry {}; {}",
            path.display(),
            PHYSICS_REGISTRY_GENERATION_GUIDANCE
        )
    })?;
    let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let expected_sha256 = expected_sha256.trim();
    if actual_sha256 != expected_sha256 {
        bail!(
            "protocol-{expected_protocol} physics registry {} is stale or corrupt: expected sha256 {}, got {}; {}",
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

/// Binds the identity-checked session-directory owner for direct starts.
///
/// Only app-derived directories carry the `direct-<pid>` naming grammar the
/// guard enforces. A flag-provided `--socket-dir` belongs to the operator
/// (documented custom layouts predate the ownership guard) and its leaf may
/// violate that grammar, so binding it would abort startup with
/// `InvalidName`; such sessions own no runtime directory and leave the
/// provided directory exactly as supplied, after preserving the historical
/// side effect that it exists. Teardown order is unchanged: the core child
/// is stopped by the explicit `drop(app)` below before any app-owned state
/// is released, and an unowned directory is never removed.
fn bind_direct_session_directory(
    args: &args::ClientArgs,
    socket_dir: std::path::PathBuf,
) -> Result<ScopedSessionDirectory> {
    if args.address.is_some() && !args.socket_dir_explicit {
        return ScopedSessionDirectory::bind(socket_dir.clone()).with_context(|| {
            format!(
                "prepare direct-connect session directory {}",
                socket_dir.display()
            )
        });
    }
    if args.address.is_some() {
        fs::create_dir_all(&socket_dir)
            .with_context(|| format!("prepare socket directory {}", socket_dir.display()))?;
    }
    Ok(ScopedSessionDirectory::none())
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
    let layout = InstallLayout::discover().context("resolve install and user runtime layout")?;
    // Reclaim leftovers of crashed earlier sessions before this process
    // binds anything new; failures are logged and never fatal.
    reclaim_stale_session_directories(&layout);
    let connection_requested = args.connection_requested();
    let socket_dir = if args.address.is_some() && !args.socket_dir_explicit {
        layout.direct_socket_dir(std::process::id())
    } else if !args.socket_dir_explicit {
        layout.runtime_root.clone()
    } else {
        resolve_socket_dir(&args.socket_dir)
    };
    // Owned before any core spawn so the direct-connect path below derives
    // the upstream client-cache advertisement from the same cache it later
    // hands to the network session.
    let client_blob_cache = ClientBlobCacheOwner::default();
    // Bound before the core guard is declared so Rust's reverse local-drop
    // order always stops the core before releasing this directory, including
    // every startup `?` before the app takes ownership of the guard.
    let _direct_session_directory = bind_direct_session_directory(&args, socket_dir.clone())?;
    let mut core_process = CoreProcessGuard::default();
    if let Some(address) = args.address.as_deref() {
        let child = spawn_core_for_address(
            &layout,
            &socket_dir,
            address,
            None,
            client_blob_cache.enables_upstream_client_cache(),
        )
        .with_context(|| format!("spawn Go core for direct connection to {address}"))?;
        core_process.replace(child);
        if let Err(error) = wait_for_core(&socket_dir) {
            crate::menu::core_process::stop_core_then(&mut core_process, |_| ());
            return Err(error).with_context(|| format!("wait for Go core endpoint for {address}"));
        }
    } else if connection_requested {
        preflight_bridge_endpoint(&socket_dir)?;
    }

    let selected_assets =
        select_asset_path_from_environment(args.assets.as_deref(), &layout.world_assets());
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
    let icon_assets = require_icon_assets(
        &loaded_assets.selected_path,
        crate::asset_startup::vanilla_source_manifest_json(),
    )
    .context("load pinned official Mojang sample item-icon carrier")?;
    eprintln!("{}", icon_assets.startup_summary());
    let lang_assets = crate::asset_startup::require_lang_assets(
        &loaded_assets.selected_path,
        crate::asset_startup::vanilla_source_manifest_json(),
    )
    .context("load pinned official Mojang sample localization carrier")?;
    eprintln!("{}", lang_assets.startup_summary());
    // The sound-definition catalog binds optionally (VPA-017): absence falls
    // back to a bounded empty catalog with this one-time notice, while a
    // present-but-invalid carrier fails startup closed above through the
    // typed error naming the exact path and rebuild command.
    let audio_catalog = match crate::asset_startup::load_audio_assets(&loaded_assets.selected_path)
    {
        Ok(Some(loaded)) => {
            eprintln!("{}", loaded.startup_summary());
            Some(loaded.into_runtime())
        }
        Ok(None) => {
            eprintln!(
                "{}",
                crate::asset_startup::audio_assets_missing_notice(
                    &crate::asset_startup::audio_asset_path(&loaded_assets.selected_path)
                )
            );
            None
        }
        Err(error) => {
            return Err(anyhow::Error::new(error))
                .context("load optional pinned sound-definition carrier");
        }
    };
    let font_runtime = loaded_assets.fonts.into_runtime();
    let mut ui_presentation = UiPresentationRuntime::with_hud_and_icons(
        font_runtime,
        hud_assets.into_runtime(),
        icon_assets.into_runtime(),
    )
    .context("prepare bounded font, HUD, and item-icon texture arrays for UI rendering")?;
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
    // One shared authority drives both startup registry gates: the
    // world-carrier provenance pins and this physics binding both derive
    // their protocol expectation from it, so a partially flipped carrier set
    // fails closed here instead of aliasing live block identities.
    let expected_protocol = crate::asset_startup::active_content_registry_protocol();
    let collision_breg = crate::asset_startup::pinned_block_registry_bytes();
    let collision_preg = read_verified_physics_registry(
        &layout.physics_registry,
        PHYSICS_REGISTRY_SHA256,
        expected_protocol,
    )?;
    let collision_registries = PhysicsCollisionRegistries::bind_coherent_assets(
        collision_breg,
        &collision_preg,
        &layout.physics_registry,
        &loaded_assets.selected_path,
        expected_protocol,
    )
    .context("decode and bind the active-content-protocol collision registries")?;
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

    let network = if connection_requested {
        match spawn_network(NetworkConfig {
            session_generation: 1,
            socket_dir,
            display_name: args.display_name.clone(),
            client_blob_cache: client_blob_cache.cache(),
        })
        .context("spawn Bedrock network worker")
        {
            Ok(network) => network,
            Err(error) => {
                crate::menu::core_process::stop_core_then(&mut core_process, |_| ());
                return Err(error);
            }
        }
    } else {
        NetworkHandle::disconnected()
    };
    let movement_ticker = network.movement_ticker();
    let present_mode = requested_present_mode(args.no_vsync);
    let diagnostics_enabled = args.acceptance_seconds.is_some() || args.metrics_out.is_some();
    let stage_profile_enabled = std::env::var_os(crate::acceptance::markers::STAGE_PROFILE)
        .as_deref()
        == Some(OsStr::new("1"));
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
                    title: if connection_requested {
                        "Rust MCBE | connecting".to_owned()
                    } else {
                        "Rust MCBE | Cinnabar".to_owned()
                    },
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
    let clear_color = if connection_requested {
        Color::srgb(0.46, 0.70, 0.92)
    } else {
        Color::srgb(0.035, 0.043, 0.059)
    };
    app.insert_resource(frame_limited_winit_settings(args.frame_cap))
        .insert_resource(ClearColor(clear_color))
        .insert_resource(shutdown_watchdog.clone())
        .insert_resource(present_mode_runtime)
        .insert_resource(core_process)
        .insert_resource(client_blob_cache)
        .insert_resource(network)
        .insert_resource(ResourcePackAdmissionState::default())
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
        .insert_resource(movement_ticker)
        .insert_resource(if args.freecam || args.auto_fly {
            PhysicsAuthorityGate::ProductionDisabled
        } else if args.phase3_candidate_physics {
            PhysicsAuthorityGate::CandidateEvidence
        } else {
            PhysicsAuthorityGate::ProductionEnabled
        })
        .insert_resource(MenuRuntime::new_with_layout(
            !connection_requested,
            args.gui_scale.unwrap_or(args::DEFAULT_GUI_SCALE),
            args.display_name.clone(),
            layout,
        ))
        .insert_resource(crate::session_audio::SessionAudioCatalog(audio_catalog))
        .insert_resource(LocalPhysicsController::default())
        .insert_resource(LocalMovementEffectTimeline::default())
        .insert_resource(LocalMovementSpeedAuthority::default())
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
        ));
    if stage_profile_enabled {
        app.insert_resource(RuntimeStageProfiler::new(true));
    }
    app.add_plugins((
        ActorRenderPlugin,
        AtmospherePlugin,
        ChunkRenderPlugin::with_budget(
            PublicationController::new(PublicationServiceConfig::PHASE2_GATE).budget(),
        ),
        FlyCameraPlugin::with_startup_capture(
            args.auto_fly,
            args.auto_fly || args.freecam || args.phase3_candidate_physics,
        ),
        UiRenderPlugin,
    ));
    if let Some(identity) = phase3_identity_source {
        app.insert_resource(identity);
    }
    configure_client_production_frame_systems(&mut app);
    configure_client_runtime_frame_systems(&mut app);
    configure_acceptance_finish_system(&mut app);

    let exit = app.run();
    if let Some(mut network) = app.world_mut().remove_resource::<NetworkHandle>() {
        network.shutdown();
    }
    drop(app);
    shutdown_watchdog.complete();
    eprintln!("{SHUTDOWN_COMPLETED} exit_code={}", app_exit_code(&exit));
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

        let result = read_verified_physics_registry(
            &path,
            &format!("{expected}\n"),
            crate::asset_startup::active_content_registry_protocol(),
        );
        fs::remove_file(path).expect("remove fixture");

        assert_eq!(result.expect("valid digest"), b"PREG test carrier");
    }

    #[test]
    fn verified_physics_registry_rejects_stale_carrier_with_guidance() {
        let path = temporary_path("preg-stale");
        fs::write(&path, b"stale PREG test carrier").expect("write fixture");

        let error = read_verified_physics_registry(
            &path,
            &"0".repeat(64),
            crate::asset_startup::active_content_registry_protocol(),
        )
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
        let error = read_verified_physics_registry(
            &path,
            &"0".repeat(64),
            crate::asset_startup::active_content_registry_protocol(),
        )
        .expect_err("missing carrier must fail");
        let message = format!("{error:#}");

        assert!(message.contains("read required protocol-1001 physics registry"));
        assert!(message.contains("make physics-assets"));
        assert!(message.contains("make client"));
    }
}

#[cfg(test)]
mod direct_session_directory_tests {
    use super::*;
    use crate::args::ParseOutcome;

    fn run_args(arguments: &[&str]) -> args::ClientArgs {
        match args::ClientArgs::parse_from(arguments.to_vec()) {
            Ok(ParseOutcome::Run(parsed)) => *parsed,
            outcome => panic!("expected run arguments, got {outcome:?}"),
        }
    }

    fn temporary_root(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("rust-mcbe-{label}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn explicit_non_grammar_socket_dir_starts_without_a_guard() {
        // Base behavior: documented custom socket directories are accepted
        // even though their leaf violates the session-directory grammar.
        let root = temporary_root("explicit-sock-dir");
        let custom = root.join("custom.sock");
        let parsed = run_args(&[
            "client",
            "--address",
            "127.0.0.1:19132",
            "--socket-dir",
            custom.to_str().expect("temp path is UTF-8"),
        ]);
        assert!(parsed.socket_dir_explicit);

        let holder = bind_direct_session_directory(&parsed, resolve_socket_dir(&parsed.socket_dir))
            .expect("an explicit non-grammar socket directory must start cleanly");
        assert!(
            custom.is_dir(),
            "the historical side effect of ensuring the directory exists stays"
        );
        let owned_entries = fs::read_dir(&custom)
            .expect("read prepared socket directory")
            .count();
        assert_eq!(
            owned_entries, 0,
            "unguarded operator directories never receive an ownership marker"
        );
        drop(holder);
        assert!(
            custom.is_dir(),
            "teardown leaves an unowned operator directory untouched"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn derived_default_directory_still_binds_the_guard() {
        let root = temporary_root("derived-sock-dir");
        fs::create_dir_all(&root).expect("create temp root");
        let parsed = run_args(&["client", "--address", "127.0.0.1:19132"]);
        assert!(!parsed.socket_dir_explicit);
        let socket_dir = root.join("direct-123");

        let holder = bind_direct_session_directory(&parsed, socket_dir.clone())
            .expect("app-derived directories keep exclusive ownership");
        assert!(socket_dir.is_dir(), "binding prepares the owned directory");
        drop(holder);
        assert!(
            !socket_dir.exists(),
            "default-path ownership and teardown are unchanged"
        );
        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod schedule_tests {
    use bevy::ecs::schedule::Schedules;

    use super::*;

    #[test]
    fn network_config_call_sites_share_the_process_blob_cache() {
        let replacing_initializer = [
            "client_blob_cache: protocol::ClientBlobCache",
            "::default()",
        ]
        .concat();
        for source in [include_str!("app.rs"), include_str!("menu/input.rs")] {
            assert!(
                !source.contains(&replacing_initializer),
                "network configuration must clone the app-owned cache"
            );
        }

        let owner = ClientBlobCacheOwner::default();
        let first = NetworkConfig {
            session_generation: 7,
            socket_dir: std::path::PathBuf::from("first-core.sock"),
            display_name: "cache-owner".to_owned(),
            client_blob_cache: owner.cache(),
        };
        let hash = first
            .client_blob_cache
            .insert(b"verified-across-session")
            .expect("seed verified blob before replacement");
        let replacement = NetworkConfig {
            session_generation: 8,
            socket_dir: std::path::PathBuf::from("replacement-core.sock"),
            display_name: "cache-owner".to_owned(),
            client_blob_cache: owner.cache(),
        };

        assert!(replacement.client_blob_cache.contains(hash));
    }

    #[test]
    fn production_update_schedule_initializes_without_dependency_cycles() {
        let mut app = App::new();
        configure_client_frame_schedule(&mut app);
        app.add_plugins(FlyCameraPlugin::default());
        configure_client_production_frame_systems(&mut app);
        configure_client_runtime_frame_systems(&mut app);
        configure_acceptance_finish_system(&mut app);

        let mut schedules = app
            .world_mut()
            .remove_resource::<Schedules>()
            .expect("Schedules resource");
        let result = schedules
            .get_mut(Update)
            .expect("production Update schedule")
            .initialize(app.world_mut());
        app.world_mut().insert_resource(schedules);

        assert!(result.is_ok(), "production Update schedule: {result:?}");
    }
}
