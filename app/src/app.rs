#[cfg(not(feature = "acceptance"))]
use crate::acceptance::AcceptanceRun;
#[cfg(feature = "acceptance")]
use crate::runtime::movement_evidence::{MovementEvidenceEmitter, MovementEvidenceIdentitySource};
use crate::runtime::network::resource_packs::PackApplication;
#[cfg(feature = "acceptance")]
use crate::runtime::shutdown::finish_acceptance_run;
use crate::ui_runtime::outbound::flush_chat_network;
#[cfg(feature = "acceptance")]
use acceptance::model_witness::poll_model_witness_request;
use anyhow::{Context, Result, bail};
use assets::carriers::startup::select_asset_path_from_environment;
use bevy::{
    anti_alias::AntiAliasPlugin,
    app::TerminalCtrlCHandlerPlugin,
    prelude::{
        App, ClearColor, Color, DefaultPlugins, First, IntoScheduleConfigs, Last, PluginGroup,
        Update, default,
    },
    render::diagnostic::RenderDiagnosticsPlugin,
    window::WindowPlugin,
};
use client_presentation::environment::{
    EnvironmentContext, EnvironmentProfileRoute, WeatherState, WorldClock,
};
use client_runtime::blob_cache::ClientBlobCacheOwner;
use client_runtime::network::NetworkHandle;
use client_state::ClientFrameSet;
use launcher_host::core_process::{CoreProcessGuard, spawn_core_for_address, wait_for_core};
use launcher_host::session_cleanup::reclaim_stale_session_directories;
use render_api::PublicationServiceConfig;
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
        ChunkTextureAssets, VisibilityDiagnosticsInput,
    },
};
pub(crate) mod logging;
pub(crate) mod session;
mod startup;
use startup::bind_direct_session_directory;

use crate::runtime::telemetry::AcceptanceRuntimeConfig;
use client_presentation::{camera::FlyCameraUpdateSet, local_player::LocalPlayerFrameSet};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::{PlatformClipboard, UiRuntime, presentation::UiPresentationRuntime};
use diagnostics::markers::SHUTDOWN_COMPLETED;
use diagnostics::metrics::MetricsCollector;
use launcher::install_layout::InstallLayout;
use {
    crate::{
        args,
        block_use::produce_block_use,
        camera::FlyCameraPlugin,
        environment::{
            self, update_atmosphere_frame, update_lightning, update_precipitation_scene,
            update_seasonal_foliage,
        },
        local_player::publish_local_player_frame,
        present_mode::{PresentModeRuntime, apply_present_mode},
        runtime::{
            network::receive_network_events,
            shutdown::{
                TerminalCause, exit_on_fatal_runtime_error, exit_on_window_close_requested,
            },
            telemetry::{
                publish_runtime_stage_profile, record_metrics, update_visibility_diagnostics,
            },
            visibility::{
                AppMetrics, CaveVisibilityCache, DiagnosticQuads, apply_added_chunk_visibility,
                refresh_cave_visibility, remove_chunk_visibility,
            },
            world::{
                ClientWorld, WorldStreamFramePoll, camera_environment::update_camera_medium,
                drive_world_stream, reconcile_world_stream_before_physics, startup_biome_tints,
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
        ui_runtime::{
            drain_inventory_authority, drive_chat_keyboard_input, drive_chat_ui_actions,
            drive_inventory_ui_actions, drive_server_form_input, drive_sign_editor,
            drive_world_inventory_keys, flush_server_form_network,
            gameplay_touch::drive_gameplay_touch_targets,
            presentation::{
                dials::advance_dials, drive_menu_panorama, observe_mount_jump_input,
                prepare_ui_runtime, publish_ui_runtime,
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

mod render_setup;
use render_setup::RendererSetup;

const PHYSICS_REGISTRY_SHA256: &str =
    include_str!("../../crates/assets/data/block-physics-v2193.sha256");
const PHYSICS_REGISTRY_GENERATION_GUIDANCE: &str =
    "run `make assets` (normal `make client` does this automatically)";

pub(crate) mod authority;
mod executor;
use crate::game_frame::configure_client_frame_schedule;
use authority::configure_client_authority_systems;

pub(crate) fn configure_client_production_frame_systems(app: &mut App) {
    #[cfg(not(feature = "acceptance"))]
    app.init_resource::<AcceptanceRun>();
    #[cfg(feature = "acceptance")]
    app.init_resource::<MovementEvidenceEmitter>();
    configure_client_authority_systems(app);
    crate::game_frame::configure_world(app);
    crate::audio::configure(app);
    app.add_plugins(client_interaction::ClientInteractionPlugin)
        .init_resource::<crate::melee::MeleeTarget>()
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
            crate::far_terrain::feed_far_terrain
                .after(drive_world_stream)
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
            advance_dials
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
            (publish_ui_runtime, drive_menu_panorama)
                .chain()
                .in_set(ClientFrameSet::UiPublication),
        );
    #[cfg(feature = "tracy")]
    app.init_resource::<crate::tracy::PhysicsEnd>().add_systems(
        Update,
        crate::tracy::mark_physics_end
            .after(crate::movement::advance_local_physics)
            .in_set(ClientFrameSet::Physics),
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

pub fn run(args: args::ClientArgs) -> Result<()> {
    args.validate_acceptance_support(cfg!(feature = "acceptance"))?;
    let feedback = crate::startup_feedback::Feedback::start();
    let app = App::new();
    #[cfg(feature = "enhanced-diagnostics")]
    let diagnostic_budget = crate::enhanced_diagnostics::configure(&args)?;
    #[cfg(feature = "developer-control")]
    crate::developer_control::prepare_native_application(
        args.address.is_some() || args.socket_dir_explicit,
    )?;
    crate::thread_budget::ThreadBudget::configure_global_rayon();
    // Keep the clipboard hook registered until the child guard runs on return or unwind.
    let _clipboard_exit =
        launcher_host::lifecycle::children::on_exit(PlatformClipboard::shutdown_if_idle);
    // Drop after all process owners so every spawned child is stopped before `run` ends.
    let _children = launcher_host::lifecycle::children::StopOnDrop;
    launcher_host::lifecycle::children::install_exit_hooks(|| {
        diagnostics::console::flush_before_exit();
    });
    UiRuntime::configure_crafting_observation(args.address.as_deref());
    render_scene::ViewmodelCompletionGate::configure_observation(args.address.as_deref());
    let layout = InstallLayout::discover().context("resolve install and user runtime layout")?;
    let global_pack_root = layout.global_resource_packs_dir();
    let minecraft_import_config_root = layout.user_config_root.clone();
    pack_runtime::compile_cache::set_compile_cache_dir(layout.compiled_pack_cache_dir());
    pack_runtime::entity_pack::set_vanilla_pack_dir(layout.vanilla_pack_dir());
    let far_terrain_dir = layout.far_terrain_cache_dir();
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
    let client_blob_cache = ClientBlobCacheOwner::persistent(layout.client_blob_cache_file());
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
        authored_texture_loading,
    } = carriers.core.context("load startup block assets")?;
    if feedback.cancelled() {
        return Ok(());
    }
    logging::loaded_assets(&loaded_assets);
    let entity_runtime = Arc::clone(loaded_assets.entities.runtime());
    let actor_catalog = actor_catalog.context("load exact entity-linked neutral actor artwork")?;
    let actor_artwork = crate::asset_bindings::actor_artwork(&actor_catalog, &entity_runtime);
    let hand_geometry =
        render_model::ViewmodelGeometry::from_runtime(&entity_runtime, &actor_artwork);
    let hud_assets = carriers
        .hud
        .context("load pinned official Mojang sample HUD carrier")?;
    bevy::log::info!("{}", hud_assets.startup_summary());
    let icon_assets = carriers
        .icons
        .context("load pinned official Mojang sample item-icon carrier")?;
    bevy::log::info!("{}", icon_assets.startup_summary());
    crate::runtime::network::install_vanilla_item_assets(&entity_runtime, icon_assets.runtime());
    // Optional: without the carrier, held items still draw as sprites and worn armor is skipped.
    let (equipment_runtime, actor_artwork, equipment_geometries) =
        client_presentation::presentation::equipment::EquipmentRuntime::build(
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
    bevy::log::info!("{}", lang_assets.startup_summary());
    let saved_settings = launcher::menu::settings_options::SettingsOptions::load(
        &layout
            .server_file()
            .with_file_name(launcher::menu::settings_options::SETTINGS_FILE),
    );
    let language = assets::carriers::startup::active_language(
        args.language.as_deref().or(saved_settings.language()),
    );
    pack_runtime::pack_language::set_active_language(&language);
    let active_lang = assets::carriers::startup::load_optional_language(
        &loaded_assets.selected_path,
        &language,
        assets::carriers::startup::vanilla_source_manifest_json(),
    );
    let startup::AudioCarriers {
        catalog: audio_catalog,
        pcm,
        sound_bank,
    } = carriers.audio?;
    let audio_device = if pcm.is_some() || sound_bank.is_some() {
        client_audio::named_audio::AudioDevice::open_default_once()
    } else {
        client_audio::named_audio::AudioDevice::disabled()
    };
    // The full engine supersedes the single-sample named path when a bank is present.
    let named_audio =
        client_audio::named_audio::NamedAudio::new(if sound_bank.is_some() { None } else { pcm });
    let audio_engine = client_audio::audio::AudioEngine::new(sound_bank);
    let particle_assets = carriers.particles;
    let flame_texture = particle_assets
        .as_deref()
        .and_then(|assets| assets.texture(assets::ACTOR_FLAME_TEXTURE));
    let (actor_flames, actor_artwork) =
        client_presentation::actor_publication::WorldActorFlames::prepare(
            actor_artwork,
            flame_texture,
        );
    let particle_icons = crate::particles::ParticleIcons(Arc::clone(icon_assets.runtime()));
    let mut block_entity_scene =
        crate::block_entities::block_entity_scene(carriers.block_entities.as_deref());
    block_entity_scene.install_entity_assets(&entity_runtime);
    feedback.progress(launcher::startup::Stage::PreparingInterface);
    let font_runtime = loaded_assets.fonts.into_runtime();
    let block_entity_font = Arc::clone(&font_runtime);
    let font_dir = loaded_assets.selected_path.parent();
    let font_runtime = oreui::fonts::install(font_runtime, font_dir.unwrap_or(Path::new(".")));
    let mut ui_presentation = UiPresentationRuntime::with_hud_and_icons(
        font_runtime,
        hud_assets.into_runtime(),
        icon_assets.into_runtime(),
    )
    .context("prepare bounded font, HUD, and item-icon texture arrays for UI rendering")?;
    // The gameplay HUD draws through the JSON-UI engine, so its carrier is required.
    let json_ui = carriers.ui?;
    json_ui
        .join()
        .and_then(|prepared| ui_presentation.enable_prepared_json_ui(prepared))
        .map_err(|reason| anyhow::anyhow!("JSON-UI engine failed to start: {reason}"))?;
    let ui_catalog = crate::runtime::network::PackUiCatalog(
        ui_presentation
            .pack_catalog_base()
            .context("JSON-UI engine is missing its carrier catalog")?,
    );
    ui_presentation.set_form_texture_fallbacks(&entity_runtime, layout.vanilla_pack_dir());
    ui_presentation.enable_touch_controls(cfg!(target_os = "android"));
    // OreUI artwork is decoded once for every native screen.
    if let Some(images) = oreui::images::load_optional_oreui_images(
        loaded_assets
            .selected_path
            .parent()
            .unwrap_or(Path::new(".")),
    ) && let Err(reason) = ui_presentation.enable_oreui_originals(images)
    {
        bevy::log::warn!("OreUI originals disabled ({reason})");
    }
    let food_catalog = equipment_catalog.clone();
    ui_presentation.set_equipment_catalog(equipment_catalog);
    ui_presentation
        .set_gui_models(&loaded_assets.runtime, &entity_runtime)
        .context("prepare native GUI model geometry and original texture pages")?;
    ui_presentation.set_gui_particle_assets(particle_assets.clone());
    ui_presentation
        .set_gui_fire_texture(flame_texture)
        .context("prepare native HUD actor flame texture frames")?;
    ui_presentation.set_gui_scale_preference(args.gui_scale);
    ui_presentation.set_safe_area(crate::ui_runtime::presentation::platform_safe_area_insets());
    let (atmosphere_runtime, atmosphere_identity) = loaded_assets.atmosphere.into_parts();
    let atmosphere_assets = AtmosphereTextureAssets::new(atmosphere_runtime, atmosphere_identity);
    let weather_textures = carriers.weather;
    let runtime_assets = loaded_assets.runtime;
    let asset_metrics = loaded_assets.metrics.into();
    let mut actor_render_scene = ActorRenderScene::with_runtime_entity_assets_and_equipment(
        &entity_runtime,
        &equipment_geometries,
    )
    .map_err(|error| {
        anyhow::anyhow!("prepare validated runtime entity geometry for actor rendering: {error:?}")
    })?;
    crate::runtime::network::block_entity_art::install_base(
        carriers.block_entities.clone(),
        &block_entity_scene,
    );
    crate::runtime::network::set_base_actor_artwork(actor_artwork.clone(), entity_runtime.clone());
    actor_render_scene.configure_artwork(actor_artwork.clone());
    if let Some(geometry) = actor_flames.geometry() {
        actor_render_scene
            .insert_geometry(geometry)
            .map_err(|error| anyhow::anyhow!("prepare world actor flame geometry: {error:?}"))?;
    }
    // A dedicated single-instance builder for the local player's first-person rig, sharing the
    // same validated geometry catalog as the third-person actor pass.
    let hand_rig_builder =
        client_presentation::actor_publication::HandRigBuilder::from_runtime_assets(
            &entity_runtime,
        )?;
    let collision_registries = carriers.collision?;
    bevy::log::info!(
        "loaded {} authoritative collision records for local physics",
        collision_registries.available_record_count()
    );
    #[cfg(feature = "acceptance")]
    let movement_identity_source = args
        .movement_evidence_target
        .map(|target| {
            MovementEvidenceIdentitySource::from_build(
                target,
                args.movement_candidate_physics,
                &collision_registries,
            )
        })
        .transpose()
        .context("bind movement evidence to this exact build and collision registry")?;

    let client_random_id = connection_requested
        .then(|| launcher_host::client_random_id::load(&layout.user_data_root))
        .transpose()
        .context("load persistent login client ID")?;

    // Validate local carriers before admitting a connection, including a cancelled launch.
    if let Some(address) = args.address.as_deref() {
        let admitted = crate::startup_feedback::while_active(
            || feedback.cancelled(),
            || {
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
                wait_for_core(&socket_dir)
                    .with_context(|| format!("wait for Go core endpoint for {address}"))
            },
        );
        match admitted {
            Ok(Some(())) => {}
            outcome => {
                launcher_host::core_process::stop_core_then(&mut core_process, |_| ());
                outcome?;
                return Ok(());
            }
        }
    } else if connection_requested
        && crate::startup_feedback::while_active(
            || feedback.cancelled(),
            || preflight_bridge_endpoint(&socket_dir),
        )?
        .is_none()
    {
        return Ok(());
    }

    // The local player's own skin, loaded once here and shared by Arc into the login upload, the
    // menu's reconnection config, and the render feed resource below. Cosmetic: never fatal.
    let local_player_skin =
        client_shell::player_skin::LocalPlayerSkin::load(&layout, &args.display_name);
    let screenshots_dir = layout.screenshots_dir();
    let renderer_data_root = layout.user_data_root.clone();
    let minecraft_import = launcher_host::minecraft_import::startup(&layout.user_config_root);
    let menu = MenuRuntime::new_with_import_snapshot(
        !connection_requested,
        args.gui_scale,
        args.display_name.clone(),
        layout,
        local_player_skin.clone(),
        minecraft_import,
    )
    .with_language_assets(
        loaded_assets.selected_path.clone(),
        args.language.as_deref(),
    );
    crate::runtime::network::await_overlay_texture_device();
    let packet_mods = crate::modding::packets::from_environment();
    let network = NetworkHandle::<PackApplication>::disconnected();
    let movement_ticker = client_interaction::movement::MovementTicker::with_epoch_publisher(
        network.physics_epoch_publisher(),
    );
    let diagnostics_enabled = args.acceptance_seconds.is_some() || args.metrics_out.is_some();
    let stage_profile_enabled =
        env_compat::var_os(diagnostics::markers::STAGE_PROFILE).as_deref() == Some(OsStr::new("1"));
    #[cfg(feature = "developer-control")]
    let hidden_surface = crate::developer_control::hidden_window_requested();
    #[cfg(not(feature = "developer-control"))]
    let hidden_surface = false;
    let present_mode_runtime = PresentModeRuntime::from_startup(
        args.force_vsync,
        args.no_vsync,
        diagnostics_enabled,
        hidden_surface,
    )
    .with_launch_frame_cap(args.frame_cap);
    let present_mode = present_mode_runtime.window_present_mode();
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

    let startup_vsync = present_mode_runtime
        .vsync_override()
        .unwrap_or_else(|| saved_settings.user_settings().video.vsync.enabled());
    if feedback.cancelled() {
        return Ok(());
    }
    feedback.progress(launcher::startup::Stage::OpeningWindow);
    let primary_window = render_setup::primary_window(
        launcher::window_title(std::env::var("CINNABAR_WINDOW_TITLE").ok().as_deref()),
        present_mode,
        render::frame_latency_for_vsync(startup_vsync),
    );
    #[cfg(feature = "developer-control")]
    let primary_window = crate::developer_control::primary_window(primary_window);
    // Rebind after the cleanup guards so app-owned processes stop before their directories drop.
    let mut app = app;
    app.insert_resource(packet_mods);
    app.insert_resource(feedback.clone());
    configure_client_frame_schedule(&mut app);
    let renderer = RendererSetup {
        data_root: renderer_data_root,
        video_override: saved_settings.graphics_api(),
        graphics_api_revision: saved_settings.graphics_api_revision(),
    };
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(primary_window),
            ..default()
        })
        .add_before::<bevy::render::RenderPlugin>(renderer)
        .disable::<bevy::render::RenderPlugin>()
        .set(crate::thread_budget::ThreadBudget::task_pool_plugin())
        // The world camera installs only the opt-in depth-based spatial filter.
        .disable::<AntiAliasPlugin>()
        // The launcher owns the production process lifecycle. Keeping the
        // OS default SIGINT action also preserves a real developer escape
        // hatch if graceful Bevy teardown is wedged.
        .disable::<TerminalCtrlCHandlerPlugin>();
    logging::add_default_plugins(&mut app, plugins);
    render_setup::finish(&mut app)?;
    app.add_plugins(render::InputPacingPlugin::default())
        .init_resource::<crate::present_mode::DisplayRefresh>()
        .init_resource::<crate::frame_pacing::PowerSource>()
        .add_systems(First, crate::frame_pacing::track_display_refresh)
        .add_systems(Last, crate::frame_pacing::update_frame_pacing);
    #[cfg(target_os = "macos")]
    crate::thread_budget::ThreadBudget::configure_render_thread(&mut app);
    #[cfg(target_os = "macos")]
    crate::frame_pacing::macos_display_link::install(&mut app);
    #[cfg(target_os = "macos")]
    crate::frame_pacing::macos_presentation::install(&mut app);
    app.add_systems(Update, crate::window_icon::apply);
    app.add_systems(Update, logging::apply_verbose_setting_changes);
    app.add_plugins(crate::hud_tools::HudToolsPlugin {
        screenshots_dir,
        debug_overlay: args.dev_debug_overlay,
    });
    // Account feeds also serve Profile in direct-address and external-socket runs.
    app.init_resource::<client_shell::menu::LauncherCoreSlot>();
    app.add_plugins(render::PresentModePolicyPlugin::new(present_mode_policy));
    if diagnostics_enabled {
        app.add_plugins(RenderDiagnosticsPlugin);
    }
    let clear_color = if connection_requested {
        Color::srgb(0.46, 0.70, 0.92)
    } else {
        Color::srgb(0.035, 0.043, 0.059)
    };
    app.insert_resource(crate::frame_pacing::FramePacingRuntime::new(
        hidden_surface || args.acceptance_seconds.is_some(),
    ))
    // Every update passes the pacer's single admission wait, so input never adds frames.
    .insert_resource(bevy::winit::WinitSettings::continuous())
    .insert_resource(ClearColor(clear_color))
    .insert_resource(shutdown_watchdog.clone())
    .insert_resource(TeardownWatchdog(shutdown_watchdog.clone()))
    .insert_resource(present_mode_runtime)
    .insert_resource(
        SessionController::new(core_process)
            .with_client_random_id(client_random_id)
            .with_address(args.address.as_deref())
            .with_startup_endpoint(
                connection_requested.then_some(socket_dir),
                args.address.as_deref(),
            ),
    )
    .insert_resource(ui_catalog)
    .insert_resource(client_blob_cache)
    .insert_resource(network)
    .insert_resource(ResourcePackAdmissionState::default())
    .insert_resource(render_scene::ActorArtworkResource(actor_artwork))
    .insert_resource(actor_flames)
    .insert_resource(ClientWorld::new_with_entity_assets(
        Arc::clone(&runtime_assets),
        Arc::clone(&entity_runtime),
    ))
    .init_resource::<crate::runtime::shutdown::FatalError>()
    .insert_resource(UiRuntimeResource({
        let mut ui_runtime = UiRuntime::new(0);
        ui_runtime.set_item_fuel_catalog(Arc::clone(&entity_runtime));
        ui_runtime.set_food_catalog(food_catalog);
        if let Some(address) = &args.address {
            ui_runtime.experiences.select_destination(address);
        }
        ui_runtime.set_lang_catalog(lang_assets.into_runtime());
        ui_runtime.set_active_language(active_lang);
        ui_runtime.set_hide_toasts(args.hide_toasts);
        ui_runtime
    }))
    .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
        0,
    )))
    .insert_resource(UiPresentationResource(ui_presentation))
    .insert_resource(WorldClock::default())
    .insert_resource(WeatherState::default())
    .init_resource::<client_presentation::environment::weather::WeatherTickFrame>()
    .insert_resource(environment::CameraMediumState::default())
    .insert_resource(client_presentation::environment::weather::LightningFlashState::default())
    .init_resource::<client_presentation::environment::fog::FogBindings>()
    .insert_resource(EnvironmentContext::default())
    .insert_resource(EnvironmentProfileRoute::default())
    .insert_resource(movement_ticker)
    .insert_resource(if args.freecam || args.auto_fly {
        PhysicsAuthorityGate::ProductionDisabled
    } else if args.movement_candidate_physics {
        PhysicsAuthorityGate::CandidateEvidence
    } else {
        PhysicsAuthorityGate::ProductionEnabled
    })
    .insert_resource(local_player_skin)
    .insert_resource(menu.with_vsync_override(vsync_override))
    .insert_resource(client_audio::session_audio::SessionAudioCatalog(
        audio_catalog,
    ))
    .insert_resource(named_audio)
    .insert_resource(audio_engine)
    .insert_non_send(audio_device)
    .insert_resource(LocalPhysicsController::default())
    .insert_resource(LocalMovementEffectTimeline::default())
    .insert_resource(LocalMovementSpeedAuthority::default())
    .insert_resource(collision_registries)
    .insert_resource(actor_render_scene)
    .insert_resource(equipment_runtime)
    .insert_resource(hand_rig_builder)
    .insert_resource(AtmosphereFrame::default())
    .insert_resource(weather_textures.clone())
    .insert_resource(
        crate::runtime::network::reload_environment::EnvironmentBase::new(
            atmosphere_assets.clone(),
            particle_assets.clone(),
            weather_textures,
        ),
    )
    .insert_resource(atmosphere_assets)
    .insert_resource(startup_biome_tints(&runtime_assets))
    .insert_resource(ChunkTextureAssets::new(runtime_assets))
    .insert_resource(authored_texture_loading)
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
    .insert_resource(render_scene::BlockEntitySceneResource(block_entity_scene))
    .insert_resource(PublicationController::new(
        PublicationServiceConfig::DEFAULT,
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
        const MAIN_FRAME: usize = render_scene::RuntimeStage::MainFrame as usize;
        app.insert_resource(crate::runtime::frame_profile::profiler(
            stage_profile_enabled,
            env_compat::var_os(diagnostics::markers::STAGE_PROFILE_FRAMES)
                .map(std::path::PathBuf::from),
        ))
        .init_resource::<render_scene::RuntimeStageSpans>()
        .add_plugins(render::GpuTimingPlugin)
        .add_systems(
            render::FrameStart,
            (
                crate::runtime::frame_profile::track_frame_interval,
                crate::runtime::frame_profile::trace_frame_focus,
                render_scene::begin_stage_span::<MAIN_FRAME>,
            )
                .chain(),
        )
        .add_systems(
            Last,
            (
                render_scene::end_stage_span::<MAIN_FRAME>,
                crate::runtime::frame_profile::flush_trace_on_exit,
            )
                .chain()
                .after(arm_shutdown_watchdog),
        );
        render::install_main_phase_spans(&mut app);
    }
    app.add_plugins((
        ActorRenderPlugin,
        AtmospherePlugin,
        ChunkRenderPlugin::with_budget(
            PublicationController::new(PublicationServiceConfig::DEFAULT).budget(),
        ),
        FlyCameraPlugin::with_startup_capture(
            args.auto_fly,
            args.auto_fly || args.freecam || args.movement_candidate_physics,
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
    app.add_plugins((
        render::DepthSmaaPlugin,
        render::motion_blur::CameraMotionBlurPlugin,
        render::FarTerrainRenderPlugin,
    ));
    app.add_plugins(
        crate::render_mode::RenderModePlugin::new(args.render_mode, diagnostics_enabled)
            .with_shader_pack(args.shader_pack.clone()),
    );
    app.insert_resource(crate::environment::FixedTime(args.fixed_time));
    app.add_plugins(crate::shader_pack_inputs::ShaderPackInputsPlugin);
    app.insert_resource(crate::shader_pack_inputs::FrameTimeCounterPin(
        args.frame_time_counter_ms.map(|ms| ms as f32 / 1000.0),
    ));
    app.add_plugins(render::PanoramaRenderPlugin);
    if let Some(particle_assets) = &particle_assets {
        app.insert_resource(render_scene::ParticleSimulation(
            particles::ParticleSystem::from_assets(particle_assets),
        ));
    }
    app.insert_resource(particle_icons);
    crate::particles::configure_particles(&mut app);
    crate::block_entities::configure(&mut app, block_entity_font);
    crate::block_selection::configure(&mut app);
    crate::primitive_shapes::configure(&mut app);
    app.init_resource::<client_presentation::presentation::viewmodel::HandAdapter>();
    if let Some(geometry) = hand_geometry {
        app.insert_resource(render_scene::ViewmodelGeometryResource(geometry));
    }
    #[cfg(feature = "acceptance")]
    if let Some(identity) = movement_identity_source {
        app.insert_resource(identity);
    }
    crate::global_resources::configure(
        &mut app,
        global_pack_root,
        minecraft_import_config_root,
        args.import_packs,
    );
    configure_client_production_frame_systems(&mut app);
    configure_client_runtime_frame_systems(&mut app);
    if let Some(service) = client_runtime::world_service::WorldServiceSlot::from_environment()? {
        app.insert_resource(service);
    }
    app.insert_resource(crate::far_terrain::FarTerrain::new(Some(far_terrain_dir)))
        .init_resource::<render_scene::FarTerrainScene>()
        .add_systems(
            Last,
            crate::far_terrain::finish_far_terrain_on_exit.after(arm_shutdown_watchdog),
        );
    crate::modding::configure_from_environment(&mut app);
    #[cfg(feature = "developer-control")]
    crate::developer_control::configure(&mut app);
    app.add_plugins(client_extensions::ClientExtensionsPlugin);
    crate::server_experiences::configure(&mut app);
    crate::shell_adapters::configure_observations(&mut app);
    session::configure_finish_system(&mut app);

    #[cfg(feature = "enhanced-diagnostics")]
    crate::enhanced_diagnostics::install(&mut app, diagnostic_budget);
    executor::configure_frame_schedule_executors(&mut app);
    app.add_plugins(render::startup_presentation::StartupPresentationPlugin)
        .add_systems(
            First,
            (
                crate::lifecycle::acknowledge_startup,
                (
                    crate::startup_feedback::synchronize_surface,
                    crate::startup_feedback::finish,
                )
                    .chain(),
            ),
        );
    crate::lifecycle::heartbeat::configure(&mut app);
    startup::install_first_frame_log(&mut app);
    let terminal_cause = TerminalCause::install(&mut app);
    feedback.progress(launcher::startup::Stage::WaitingForContent);
    let exit = crate::runtime::frame_profile::run(&mut app);
    PlatformClipboard::shutdown();
    client_shell::discord_presence::shutdown(&mut app);
    if let Some(mut network) = app
        .world_mut()
        .remove_resource::<NetworkHandle<PackApplication>>()
    {
        network.shutdown();
    }
    drop(app);
    bridge::latency_trace::finish();
    let _ = writeln!(
        diagnostics::console::stderr(),
        "{SHUTDOWN_COMPLETED} exit_code={}",
        app_exit_code(&exit)
    );
    diagnostics::console::flush_before_exit();
    shutdown_watchdog.complete();
    if exit.is_error() {
        bail!("{}", terminal_cause.exit_message());
    }
    Ok(())
}

#[cfg(test)]
mod direct_session_directory_tests;
#[cfg(test)]
mod preg_startup_tests;
#[cfg(test)]
mod schedule_tests;
