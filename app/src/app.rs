#[cfg(feature = "acceptance")]
use crate::acceptance::{
    model_witness::poll_model_witness_request,
    transparent_witness::poll_transparent_witness_request,
};
#[cfg(feature = "acceptance")]
use crate::runtime::phase3_evidence::{
    Phase3EvidenceEmitter, Phase3EvidenceIdentitySource, emit_phase3_evidence,
};
#[cfg(feature = "acceptance")]
use crate::runtime::shutdown::finish_acceptance_run;
use std::{ffi::OsStr, fs, sync::Arc};

use anyhow::{Context, Result, bail};
use bevy::{
    anti_alias::AntiAliasPlugin,
    app::TerminalCtrlCHandlerPlugin,
    prelude::{
        App, ClearColor, Color, DefaultPlugins, First, IntoScheduleConfigs, Last, PluginGroup,
        Resource, SystemSet, Update, Window, default,
    },
    render::{diagnostic::RenderDiagnosticsPlugin, settings::Backends},
    window::WindowPlugin,
};
use chunk_pipeline::PublicationServiceConfig;
use render::{
    ActorRenderPlugin, ActorRenderScene, AtmosphereFrame, AtmospherePlugin,
    AtmosphereTextureAssets, ChunkRenderApplySet, ChunkRenderPlugin, ChunkTextureAssets,
    RuntimeStageProfiler, UiRenderPlugin, VisibilityDiagnosticsInput,
};
mod startup;

#[cfg(feature = "acceptance")]
use crate::acceptance::world_ready::emit_world_ready;
use crate::{
    args,
    asset_startup::{LoadedAssetKind, select_asset_path_from_environment},
    block_use::{BlockUseRuntime, produce_block_use},
    camera::{FlyCameraPlugin, FlyCameraUpdateSet},
    environment::{
        self, EnvironmentContext, EnvironmentProfileRoute, WeatherState, WorldClock,
        update_atmosphere_frame, update_lightning, update_precipitation_scene,
        update_seasonal_foliage,
    },
    install_layout::InstallLayout,
    local_player::{
        LocalPlayerFrameSet, publish_interaction_origin, publish_local_player_frame,
        resolve_camera_pose,
    },
    melee::{MeleeRuntime, SwingTracker, produce_melee},
    menu::{
        CoreProcessGuard, MenuRuntime, drive_menu_input, drive_menu_services,
        spawn_core_for_address, wait_for_core,
    },
    movement::{
        LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
        PhysicsAuthorityGate, advance_local_physics, send_movement_prediction_sync,
    },
    present_mode::{PresentModeRuntime, apply_runtime_vsync_setting},
    runtime::{
        endpoint::{preflight_bridge_endpoint, resolve_socket_dir},
        network::{
            NetworkConfig, NetworkHandle, ResourcePackAdmissionState, advance_actor_frame,
            prepare_actor_render_frame, publish_actor_render_frame, publish_entity_shadows,
            receive_network_events, spawn_network,
        },
        publication::{PublicationController, begin_publication_frame},
        shutdown::{exit_on_fatal_runtime_error, exit_on_window_close_requested},
        telemetry::{
            AcceptanceRuntimeConfig, frame_limited_winit_settings, publish_runtime_stage_profile,
            record_metrics, send_player_auth_inputs, update_visibility_diagnostics,
        },
        visibility::{
            AppMetrics, CaveVisibilityCache, DiagnosticQuads, apply_added_chunk_visibility,
            refresh_cave_visibility, remove_chunk_visibility,
        },
        world::{
            ClientWorld, SHUTDOWN_WATCHDOG_TIMEOUT, ShutdownWatchdog, TeardownWatchdog,
            WorldStreamFramePoll, app_exit_code, arm_shutdown_watchdog, drive_world_stream,
            reconcile_world_stream_before_physics, startup_biome_tints, update_camera_medium,
        },
    },
    semantic_controls::{
        collect_raw_input, finalize_semantic_input_after_ui_authority, route_semantic_input,
        synchronize_semantic_input_authority,
    },
    session::{SessionController, drive_session, follow_server_transfer, recover_session_failure},
    session_cleanup::{ScopedSessionDirectory, reclaim_stale_session_directories},
    survival_mining::{SurvivalMiningRuntime, produce_survival_mining},
    ui_runtime::{
        drain_inventory_authority, drive_chat_keyboard_input, drive_chat_ui_actions,
        drive_inventory_ui_actions, drive_server_form_input, drive_sign_editor,
        drive_world_inventory_keys, flush_chat_network, flush_inventory_network,
        flush_server_form_network,
        gameplay_touch::drive_gameplay_touch_targets,
        presentation::{
            drive_menu_panorama, observe_mount_jump_input, prepare_ui_runtime, publish_ui_runtime,
        },
    },
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use diagnostics::markers::{SHUTDOWN_COMPLETED, requested_present_mode};
use diagnostics::metrics::MetricsCollector;

#[cfg(feature = "acceptance")]
use crate::acceptance::model_witness::drive_model_witness;

mod render_setup;
use render_setup::render_plugin;

const PHYSICS_REGISTRY_SHA256: &str =
    include_str!("../../crates/assets/data/block-physics-v2193.sha256");
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

mod authority;
pub(crate) use authority::{configure_client_authority_systems, configure_client_frame_schedule};

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
    ActorPreparation,
    UiPreparation,
    NetworkSend,
    ActorFinalization,
    ActorPublication,
    UiPublication,
}

/// Registers the production actor observation and publication boundaries.
pub(crate) fn configure_actor_render_systems(app: &mut App) {
    app.init_resource::<client_presentation::actor_publication::ActorFrameState>()
        .add_systems(
            Update,
            advance_actor_frame.in_set(ClientFrameSet::ActorPreparation),
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
    app.init_resource::<crate::acceptance::AcceptanceRun>();
    #[cfg(feature = "acceptance")]
    app.init_resource::<Phase3EvidenceEmitter>();
    app.init_resource::<crate::runtime::network::PackReload>();
    configure_client_authority_systems(app);
    configure_actor_render_systems(app);
    crate::audio::configure(app);
    app.init_resource::<BlockUseRuntime>()
        .init_resource::<crate::item_use::ItemUseRuntime>()
        .init_resource::<SurvivalMiningRuntime>()
        .init_resource::<MeleeRuntime>()
        .init_resource::<SwingTracker>()
        .init_resource::<client_presentation::server_camera::ServerCameraInstructions>()
        .init_resource::<crate::session_audio::SessionAudio>()
        .init_resource::<crate::named_audio::NamedAudio>()
        .init_resource::<crate::audio::AudioEngine>()
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
            (publish_local_player_frame, publish_interaction_origin, crate::camera::aim_assist::publish_assisted_interaction, crate::camera::aim_highlight::publish, crate::block_use::retain_block_use_pick)
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
            crate::hotbar::select_hotbar_slot
                .after(ClientFrameSet::SemanticFinalize)
                .before(ClientFrameSet::ActorPreparation),
        )
        // Build actions resolve before the tick they precede, so its movement sees the block.
        .add_systems(
            Update,
            produce_block_use
                .after(ClientFrameSet::SemanticFinalize)
                .after(crate::hotbar::select_hotbar_slot)
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
                send_player_auth_inputs,
                crate::pick_block::produce_pick_block,
            )
                .chain()
                .in_set(ClientFrameSet::NetworkSend),
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
                apply_runtime_vsync_setting,
                record_metrics,
                publish_runtime_stage_profile,
            )
                .chain()
                .after(FlyCameraUpdateSet),
        )
        .add_systems(Last, arm_shutdown_watchdog);
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

pub fn run(args: args::ClientArgs) -> Result<()> {
    args.validate_acceptance_support(cfg!(feature = "acceptance"))?;
    #[cfg(feature = "developer-control")]
    crate::developer_control::prepare_native_application(
        args.address.is_some() || args.socket_dir_explicit,
    )?;
    crate::thread_budget::ThreadBudget::configure_global_rayon();
    // Declared first so it drops last: every spawned child is gone before `run` returns or unwinds.
    let _children = crate::lifecycle::children::StopOnDrop;
    crate::lifecycle::children::install_exit_hooks();
    UiRuntime::configure_crafting_observation(args.address.as_deref());
    render::ViewmodelCompletionGate::configure_observation(args.address.as_deref());
    let layout = InstallLayout::discover().context("resolve install and user runtime layout")?;
    let global_pack_root = layout.global_resource_packs_dir();
    crate::runtime::network::set_compile_cache_dir(layout.compiled_pack_cache_dir());
    crate::runtime::network::entity_pack::set_vanilla_pack_dir(layout.vanilla_pack_dir());
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
    let selected_assets =
        select_asset_path_from_environment(args.assets.as_deref(), &layout.world_assets());
    let carriers = startup::load_startup_carriers(selected_assets, &layout.physics_registry);
    let startup::CoreCarriers {
        assets: loaded_assets,
        actor: actor_catalog,
        equipment: equipment_catalog,
    } = carriers.core.context("load startup block assets")?;
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
    crate::runtime::network::set_vanilla_item_paths(&entity_runtime);
    let actor_catalog = actor_catalog.context("load exact entity-linked neutral actor artwork")?;
    let actor_artwork = crate::asset_startup::actor_artwork(&actor_catalog, &entity_runtime);
    let hand_geometry = render::ViewmodelGeometry::from_runtime(&entity_runtime, &actor_artwork);
    let hud_assets = carriers
        .hud
        .context("load pinned official Mojang sample HUD carrier")?;
    eprintln!("{}", hud_assets.startup_summary());
    let icon_assets = carriers
        .icons
        .context("load pinned official Mojang sample item-icon carrier")?;
    eprintln!("{}", icon_assets.startup_summary());
    // Optional: without the carrier, held items still draw as sprites and worn armor is skipped.
    let (equipment_runtime, actor_artwork, equipment_geometries) =
        crate::presentation::equipment::EquipmentRuntime::build(
            Arc::clone(&entity_runtime),
            equipment_catalog.clone(),
            Arc::clone(icon_assets.runtime()),
            Some(Arc::clone(&loaded_assets.runtime)),
            carriers.block_entities.clone(),
            actor_artwork,
        );
    let lang_assets = carriers
        .lang
        .context("load pinned official Mojang sample localization carrier")?;
    eprintln!("{}", lang_assets.startup_summary());
    let saved_settings = crate::menu::settings_options::SettingsOptions::load(
        &layout
            .server_file()
            .with_file_name(crate::menu::settings_options::SETTINGS_FILE),
    );
    let active_lang = crate::asset_startup::load_active_language(
        &loaded_assets.selected_path,
        args.language.as_deref().or(saved_settings.language()),
    );
    let startup::AudioCarriers {
        catalog: audio_catalog,
        pcm,
        sound_bank,
    } = carriers.audio?;
    let audio_device = if pcm.is_some() || sound_bank.is_some() {
        crate::named_audio::AudioDevice::open_default_once()
    } else {
        crate::named_audio::AudioDevice::disabled()
    };
    // The full engine supersedes the single-sample named path when a bank is present.
    let named_audio =
        crate::named_audio::NamedAudio::new(if sound_bank.is_some() { None } else { pcm });
    let audio_engine = crate::audio::AudioEngine::new(sound_bank);
    let particle_assets = carriers.particles;
    let particle_icons = crate::particles::ParticleIcons(Arc::clone(icon_assets.runtime()));
    let mut block_entity_scene =
        crate::block_entities::block_entity_scene(carriers.block_entities.as_deref());
    block_entity_scene.install_entity_assets(&entity_runtime);
    block_entity_scene.install_mob_assets(&entity_runtime, &actor_catalog);
    let font_runtime = loaded_assets.fonts.into_runtime();
    let block_entity_font = Arc::clone(&font_runtime);
    let font_runtime =
        crate::asset_startup::oreui_fonts::install(font_runtime, &layout.resource_root);
    let mut ui_presentation = UiPresentationRuntime::with_hud_and_icons(
        font_runtime,
        hud_assets.into_runtime(),
        icon_assets.into_runtime(),
    )
    .context("prepare bounded font, HUD, and item-icon texture arrays for UI rendering")?;
    // The gameplay HUD draws through the JSON-UI engine, so its carrier is required.
    let ui_assets = carriers.ui?;
    ui_presentation
        .enable_json_ui(ui_assets)
        .map_err(|reason| anyhow::anyhow!("JSON-UI engine failed to start: {reason}"))?;
    let ui_catalog = crate::runtime::network::PackUiCatalog(
        ui_presentation
            .pack_catalog_base()
            .context("JSON-UI engine is missing its carrier catalog")?,
    );
    ui_presentation.set_form_texture_fallbacks(&entity_runtime, layout.vanilla_pack_dir());
    // Installed OreUI artwork is discovered and decoded once for every native screen.
    if let Some(images) = client_ui::ui_runtime::oreui_assets::load_optional_oreui_images()
        && let Err(reason) = ui_presentation.enable_oreui_originals(images)
    {
        eprintln!("OreUI originals disabled ({reason})");
    }
    ui_presentation.set_equipment_catalog(equipment_catalog);
    ui_presentation
        .set_gui_models(&loaded_assets.runtime, &entity_runtime)
        .context("prepare native GUI model geometry and original texture pages")?;
    ui_presentation
        .set_gui_fire_texture(
            particle_assets
                .as_deref()
                .and_then(|assets| assets.texture(assets::ACTOR_FLAME_TEXTURE)),
        )
        .context("prepare native HUD actor flame texture frames")?;
    ui_presentation.set_gui_scale_preference(args.gui_scale);
    ui_presentation.set_safe_area(crate::ui_runtime::presentation::platform_safe_area_insets());
    let (atmosphere_runtime, atmosphere_identity) = loaded_assets.atmosphere.into_parts();
    let weather_textures = carriers.weather;
    let runtime_assets = loaded_assets.runtime;
    let asset_metrics = loaded_assets.metrics;
    let mut actor_render_scene = ActorRenderScene::with_runtime_entity_assets_and_equipment(
        &entity_runtime,
        &equipment_geometries,
    )
    .map_err(|error| {
        anyhow::anyhow!("prepare validated runtime entity geometry for actor rendering: {error:?}")
    })?;
    crate::runtime::network::set_base_actor_artwork(actor_artwork.clone(), entity_runtime.clone());
    actor_render_scene.configure_artwork(actor_artwork.clone());
    // A dedicated single-instance builder for the local player's first-person rig, sharing the
    // same validated geometry catalog as the third-person actor pass.
    let hand_rig_builder =
        crate::runtime::network::HandRigBuilder::from_runtime_assets(&entity_runtime)?;
    let collision_registries = carriers.collision?;
    eprintln!(
        "loaded {} authoritative collision records for local physics",
        collision_registries.available_record_count()
    );
    #[cfg(feature = "acceptance")]
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

    // Validate local carriers before starting a connection: an asset failure
    // must not launch the core or wait on a server the client cannot render.
    if let Some(address) = args.address.as_deref() {
        let child = spawn_core_for_address(
            &layout,
            &socket_dir,
            address,
            None,
            client_blob_cache.enables_upstream_client_cache(),
            false,
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

    // The local player's own skin, loaded once here and shared by Arc into the login upload, the
    // menu's reconnection config, and the render feed resource below. Cosmetic: never fatal.
    let local_player_skin = crate::player_skin::LocalPlayerSkin::load(&layout, &args.display_name);
    let network = if connection_requested {
        match spawn_network(NetworkConfig {
            session_generation: 1,
            socket_dir,
            display_name: args.display_name.clone(),
            client_blob_cache: client_blob_cache.cache(),
            player_skin: local_player_skin.clone(),
            actor_artwork: Some(actor_artwork.clone()),
            ui_catalog: Some(ui_catalog.0.clone()),
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
    let vsync_override = present_mode_runtime.vsync_override();
    let runtime_config = AcceptanceRuntimeConfig {
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    };
    let shutdown_watchdog = ShutdownWatchdog::process(SHUTDOWN_WATCHDOG_TIMEOUT);

    let primary_window = Window {
        title: launcher::window_title(std::env::var("CINNABAR_WINDOW_TITLE").ok().as_deref()),
        present_mode,
        ..default()
    };
    #[cfg(feature = "developer-control")]
    let primary_window = crate::developer_control::primary_window(primary_window);
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(primary_window),
            ..default()
        })
        .set(render_plugin())
        .set(crate::thread_budget::ThreadBudget::task_pool_plugin())
        // Vanilla resolves multisampled geometry without a screen-space AA filter.
        .disable::<AntiAliasPlugin>()
        // The launcher owns the production process lifecycle. Keeping the
        // OS default SIGINT action also preserves a real developer escape
        // hatch if graceful Bevy teardown is wedged.
        .disable::<TerminalCtrlCHandlerPlugin>();
    #[cfg(feature = "tracy")]
    let plugins = plugins.set(bevy::log::LogPlugin {
        custom_layer: crate::tracy::layer,
        ..default()
    });
    app.add_plugins(plugins);
    app.add_plugins(render::InputPacingPlugin::default());
    #[cfg(target_os = "macos")]
    crate::thread_budget::ThreadBudget::configure_render_thread(&mut app);
    app.add_systems(Update, crate::window_icon::apply);
    app.add_plugins(crate::local_worlds::LocalWorldsPlugin);
    app.add_plugins(crate::hud_tools::HudToolsPlugin {
        screenshots_dir: layout.screenshots_dir(),
        debug_overlay: args.dev_debug_overlay,
    });
    // Account feeds also serve Profile in direct-address and external-socket runs.
    app.init_resource::<crate::menu::LauncherCoreSlot>();
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
    app.insert_resource(frame_limited_winit_settings(
        args.frame_cap,
        args.acceptance_seconds.is_some(),
    ))
    .insert_resource(ClearColor(clear_color))
    .insert_resource(shutdown_watchdog.clone())
    .insert_resource(TeardownWatchdog(shutdown_watchdog.clone()))
    .insert_resource(present_mode_runtime)
    .insert_resource(SessionController::new(core_process).with_address(args.address.as_deref()))
    .insert_resource(ui_catalog)
    .insert_resource(client_blob_cache)
    .insert_resource(network)
    .insert_resource(ResourcePackAdmissionState::default())
    .insert_resource(actor_artwork)
    .insert_resource(ClientWorld::new_with_entity_assets(
        Arc::clone(&runtime_assets),
        entity_runtime,
    ))
    .insert_resource({
        let mut ui_runtime = UiRuntime::new(0);
        if let Some(address) = &args.address {
            ui_runtime.experiences.select_destination(address);
        }
        ui_runtime.set_lang_catalog(lang_assets.into_runtime());
        ui_runtime.set_active_language(active_lang);
        ui_runtime
    })
    .insert_resource(crate::player_runtime::PlayerRuntime::new(0))
    .insert_resource(ui_presentation)
    .insert_resource(WorldClock::default())
    .insert_resource(WeatherState::default())
    .init_resource::<environment::WeatherTickFrame>()
    .insert_resource(environment::CameraMediumState::default())
    .insert_resource(environment::LightningFlashState::default())
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
    .insert_resource(local_player_skin.clone())
    .insert_resource(
        MenuRuntime::new_with_layout(
            !connection_requested,
            args.gui_scale,
            args.display_name.clone(),
            layout,
            local_player_skin,
        )
        .with_language_assets(
            loaded_assets.selected_path.clone(),
            args.language.as_deref(),
        )
        .with_vsync_override(vsync_override),
    )
    .init_resource::<crate::menu::MenuClipboard>()
    .insert_resource(crate::session_audio::SessionAudioCatalog(audio_catalog))
    .insert_resource(named_audio)
    .insert_resource(audio_engine)
    .insert_non_send_resource(audio_device)
    .insert_resource(LocalPhysicsController::default())
    .insert_resource(LocalMovementEffectTimeline::default())
    .insert_resource(LocalMovementSpeedAuthority::default())
    .insert_resource(collision_registries)
    .insert_resource(actor_render_scene)
    .insert_resource(equipment_runtime)
    .insert_resource(hand_rig_builder)
    .insert_resource(AtmosphereFrame::default())
    .insert_resource(weather_textures)
    .insert_resource(
        crate::runtime::network::reload_environment::EnvironmentBase::new(
            AtmosphereTextureAssets::new(atmosphere_runtime.clone(), atmosphere_identity),
            particle_assets.clone(),
        ),
    )
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
    .insert_resource(block_entity_scene)
    .insert_resource(PublicationController::new(
        PublicationServiceConfig::PHASE2_GATE,
    ));
    #[cfg(feature = "acceptance")]
    app.add_plugins(acceptance::AcceptancePlugin {
        seconds: args.acceptance_seconds,
        metrics_out: args.metrics_out,
        full_view_teleport_gate: args.full_view_teleport_gate,
        require_transparent_presentation: args.require_transparent_presentation,
        transparent_witness_request: args.transparent_witness_request,
        model_witness_request: args.model_witness_request,
    });
    {
        const MAIN_FRAME: usize = render::RuntimeStage::MainFrame as usize;
        app.insert_resource(RuntimeStageProfiler::for_gameplay(
            stage_profile_enabled,
            std::env::var_os(crate::acceptance::markers::STAGE_PROFILE_FRAMES)
                .map(std::path::PathBuf::from),
        ))
        .init_resource::<render::RuntimeStageSpans>()
        .add_plugins(render::GpuTimingPlugin)
        .add_systems(
            First,
            (
                crate::runtime::frame_profile::track_frame_interval,
                crate::runtime::frame_profile::trace_frame_focus,
                render::begin_stage_span::<MAIN_FRAME>,
            )
                .chain(),
        )
        .add_systems(
            Last,
            (
                render::end_stage_span::<MAIN_FRAME>,
                crate::runtime::frame_profile::flush_trace_on_exit,
            )
                .chain()
                .after(arm_shutdown_watchdog),
        );
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
        render::ViewmodelRenderPlugin,
        render::HandRigRenderPlugin,
        render::DroppedItemRenderPlugin,
        render::ScreenOverlayRenderPlugin,
        render::AimAssistHighlightPlugin,
        render::ParticleRenderPlugin,
        render::BlockEntityRenderPlugin,
        render::EntityShadowRenderPlugin,
    ));
    app.add_plugins(crate::render_mode::RenderModePlugin::new(
        args.render_mode,
        diagnostics_enabled,
    ));
    app.add_plugins(render::PanoramaRenderPlugin);
    if let Some(particle_assets) = &particle_assets {
        app.insert_resource(render::ParticleSimulation(
            particles::ParticleSystem::from_assets(particle_assets),
        ));
    }
    app.insert_resource(particle_icons);
    crate::particles::configure_particles(&mut app);
    crate::block_entities::configure(&mut app, block_entity_font);
    crate::block_selection::configure(&mut app);
    crate::primitive_shapes::configure(&mut app);
    app.init_resource::<crate::presentation::viewmodel::HandAdapter>();
    if let Some(geometry) = hand_geometry {
        app.insert_resource(geometry);
    }
    #[cfg(feature = "acceptance")]
    if let Some(identity) = phase3_identity_source {
        app.insert_resource(identity);
    }
    crate::global_resources::configure(&mut app, global_pack_root, args.import_packs);
    configure_client_production_frame_systems(&mut app);
    configure_client_runtime_frame_systems(&mut app);
    crate::modding::configure_from_environment(&mut app);
    #[cfg(feature = "developer-control")]
    crate::developer_control::configure(&mut app);
    crate::server_experiences::configure(&mut app);
    crate::discord_presence::configure(&mut app);
    configure_acceptance_finish_system(&mut app);

    let exit = app.run();
    crate::discord_presence::shutdown(&mut app);
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
mod direct_session_directory_tests;

#[cfg(test)]
mod preg_startup_tests;
#[cfg(test)]
mod schedule_tests;
