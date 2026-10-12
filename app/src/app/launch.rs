//! Starts the client: loads the carriers, starts the core it joins through, builds the app and
//! runs it until exit.

use super::*;
use launcher_host::session_cleanup::ScopedSessionDirectory;

/// How the client reaches the core its sessions join through.
pub enum CoreLink {
    /// A core this process starts for `--address`, or one already serving `--socket-dir`.
    Local,
    /// A web relay, as a browser joins through. The client starts at its menu: `sessions`
    /// issues the relay session each join opens, and `account` signs in on the relay and
    /// reports the joins it prepares.
    Relay {
        sessions: std::sync::Arc<dyn bridge::RelaySessions>,
        account: Box<dyn launcher_host::account_control::AccountControl + Send + Sync>,
    },
}

/// Runs the client against a local core.
pub fn run(args: args::ClientArgs) -> Result<()> {
    run_with(args, CoreLink::Local)
}

/// Runs the client against `link` until it exits; in a browser it returns once the page's event
/// loop owns the app.
pub fn run_with(args: args::ClientArgs, link: CoreLink) -> Result<()> {
    args.validate_acceptance_support(cfg!(feature = "acceptance"))?;
    #[cfg(feature = "enhanced-diagnostics")]
    let diagnostic_budget = crate::enhanced_diagnostics::configure(&args)?;
    #[cfg(feature = "developer-control")]
    crate::developer_control::prepare_native_application(
        args.address.is_some() || args.socket_dir_explicit,
    )?;
    // A browser cannot start threads; rayon then runs its work on the calling thread.
    #[cfg(not(target_arch = "wasm32"))]
    crate::thread_budget::ThreadBudget::configure_global_rayon();
    // Declared first so it drops last: every spawned child is gone before `run` returns or unwinds.
    let _children = launcher_host::lifecycle::children::StopOnDrop;
    launcher_host::lifecycle::children::install_exit_hooks(|| {
        diagnostics::console::flush_before_exit();
    });
    UiRuntime::configure_crafting_observation(args.address.as_deref());
    render_scene::ViewmodelCompletionGate::configure_observation(args.address.as_deref());
    crate::startup_progress::report("Loading game files", None);
    let layout = InstallLayout::discover().context("resolve install and user runtime layout")?;
    let global_pack_root = layout.global_resource_packs_dir();
    pack_runtime::compile_cache::set_compile_cache_dir(layout.compiled_pack_cache_dir());
    pack_runtime::entity_pack::set_vanilla_pack_dir(layout.vanilla_pack_dir());
    let (relay, relay_account) = match link {
        CoreLink::Local => (None, None),
        CoreLink::Relay { sessions, account } => (Some(sessions), Some(account)),
    };
    if relay.is_some() && args.connection_requested() {
        bail!("a web relay link starts at the menu and joins from there; it takes no --address");
    }
    // Reclaim leftovers of crashed earlier sessions before this process
    // binds anything new; failures are logged and never fatal. A relay
    // link owns no session directories.
    if relay.is_none() {
        reclaim_stale_session_directories(&layout);
    }
    let connection_requested = args.connection_requested();
    // A relay link reaches no local endpoint; the directory is never used.
    let socket_dir = if relay.is_some() {
        layout.runtime_root.clone()
    } else if args.address.is_some() && !args.socket_dir_explicit {
        layout.direct_socket_dir(launcher::install_layout::process_id())
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
    let _direct_session_directory = if relay.is_none() {
        bind_direct_session_directory(&args, socket_dir.clone())?
    } else {
        ScopedSessionDirectory::none()
    };
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
        diagnostics::log_stderr!("{notice}");
    } else if loaded_assets.kind == LoadedAssetKind::CompiledBlob {
        diagnostics::log_stderr!(
            "loaded compiled block assets from {} (sha256 {})",
            loaded_assets.selected_path.display(),
            loaded_assets.metrics.blob_sha256
        );
    }
    diagnostics::log_stderr!(
        "loaded required atmosphere assets from {}",
        loaded_assets.atmosphere.selected_path().display()
    );
    diagnostics::log_stderr!(
        "{}",
        crate::asset_bindings::AtmosphereEvidence::startup_summary(&loaded_assets.atmosphere)
    );
    diagnostics::log_stderr!(
        "loaded required entity assets from {}",
        loaded_assets.entities.selected_path().display()
    );
    diagnostics::log_stderr!("{}", loaded_assets.entities.startup_summary());
    diagnostics::log_stderr!("{}", loaded_assets.fonts.startup_summary());
    let entity_runtime = Arc::clone(loaded_assets.entities.runtime());
    crate::runtime::network::set_vanilla_item_paths(&entity_runtime);
    let actor_catalog = actor_catalog.context("load exact entity-linked neutral actor artwork")?;
    let actor_artwork = crate::asset_bindings::actor_artwork(&actor_catalog, &entity_runtime);
    let hand_geometry =
        render_model::ViewmodelGeometry::from_runtime(&entity_runtime, &actor_artwork);
    let hud_assets = carriers
        .hud
        .context("load pinned official Mojang sample HUD carrier")?;
    diagnostics::log_stderr!("{}", hud_assets.startup_summary());
    let icon_assets = carriers
        .icons
        .context("load pinned official Mojang sample item-icon carrier")?;
    diagnostics::log_stderr!("{}", icon_assets.startup_summary());
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
    diagnostics::log_stderr!("{}", lang_assets.startup_summary());
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
    let particle_icons = crate::particles::ParticleIcons(Arc::clone(icon_assets.runtime()));
    let mut block_entity_scene =
        crate::block_entities::block_entity_scene(carriers.block_entities.as_deref());
    block_entity_scene.install_entity_assets(&entity_runtime);
    block_entity_scene.install_mob_assets(&entity_runtime, &actor_catalog);
    let font_runtime = loaded_assets.fonts.into_runtime();
    let block_entity_font = Arc::clone(&font_runtime);
    let font_dir = loaded_assets.selected_path.parent();
    let font_runtime = oreui_fonts::install(font_runtime, font_dir.unwrap_or(Path::new(".")));
    // Every carrier is decoded now; a browser keeps in memory only the languages a player can
    // still switch to.
    let language_dir =
        assets::carriers::startup::language_carrier_dir(&loaded_assets.selected_path);
    assets::carriers::file::release_preloaded(|path| path.starts_with(&language_dir));
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
        diagnostics::log_stderr!("OreUI originals disabled ({reason})");
    }
    let food_catalog = equipment_catalog.clone();
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
    crate::runtime::network::set_base_actor_artwork(actor_artwork.clone(), entity_runtime.clone());
    actor_render_scene.configure_artwork(actor_artwork.clone());
    // A dedicated single-instance builder for the local player's first-person rig, sharing the
    // same validated geometry catalog as the third-person actor pass.
    let hand_rig_builder =
        client_presentation::actor_publication::HandRigBuilder::from_runtime_assets(
            &entity_runtime,
        )?;
    let collision_registries = carriers.collision?;
    diagnostics::log_stderr!(
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

    let client_random_id = connection_requested
        .then(|| launcher_host::client_random_id::load(&layout.user_data_root))
        .transpose()
        .context("load persistent login client ID")?;

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
            launcher_host::core_process::stop_core_then(&mut core_process, |_| ());
            return Err(error).with_context(|| format!("wait for Go core endpoint for {address}"));
        }
    } else if connection_requested {
        preflight_bridge_endpoint(&socket_dir)?;
    }

    // The local player's own skin, loaded once here and shared by Arc into the login upload, the
    // menu's reconnection config, and the render feed resource below. Cosmetic: never fatal.
    let local_player_skin =
        client_shell::player_skin::LocalPlayerSkin::load(&layout, &args.display_name);
    let screenshots_dir = layout.screenshots_dir();
    let menu = MenuRuntime::new_with_layout(
        !connection_requested,
        args.gui_scale,
        args.display_name.clone(),
        layout,
        local_player_skin.clone(),
    )
    .with_language_assets(
        loaded_assets.selected_path.clone(),
        args.language.as_deref(),
    );
    crate::runtime::network::await_overlay_texture_device();
    // The relay signs in itself, so no device-code helper runs.
    let menu = if relay_account.is_some() {
        menu.with_control_sign_in()
    } else {
        menu
    };
    let network = if connection_requested {
        match spawn_network(NetworkConfig {
            session_generation: 1,
            core: bridge::CoreEndpoint::SocketDir(socket_dir),
            target: None,
            display_name: args.display_name.clone(),
            client_random_id: client_random_id.expect("connection requested"),
            client_blob_cache: client_blob_cache.cache(),
            player_skin: local_player_skin.clone(),
            login_settings: crate::session::login_settings(&menu, None),
            actor_artwork: Some(actor_artwork.clone()),
            ui_catalog: Some(ui_catalog.0.clone()),
        })
        .context("spawn Bedrock network worker")
        {
            Ok(network) => network,
            Err(error) => {
                launcher_host::core_process::stop_core_then(&mut core_process, |_| ());
                return Err(error);
            }
        }
    } else {
        NetworkHandle::<PackApplication>::disconnected()
    };
    let movement_ticker = client_interaction::movement::MovementTicker::with_epoch_publisher(
        network.physics_epoch_publisher(),
    );
    let diagnostics_enabled = args.acceptance_seconds.is_some() || args.metrics_out.is_some();
    let stage_profile_enabled =
        std::env::var_os(diagnostics::markers::STAGE_PROFILE).as_deref() == Some(OsStr::new("1"));
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
        .unwrap_or_else(|| saved_settings.user_settings().video.vsync);
    let primary_window = render_setup::primary_window(
        launcher::window_title(std::env::var("CINNABAR_WINDOW_TITLE").ok().as_deref()),
        present_mode,
        render::frame_latency_for_vsync(startup_vsync),
    );
    #[cfg(feature = "developer-control")]
    let primary_window = crate::developer_control::primary_window(primary_window);
    crate::startup_progress::report("Building the game", None);
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(primary_window),
            ..default()
        })
        .set(render_plugin())
        .set(crate::thread_budget::ThreadBudget::task_pool_plugin())
        // The world camera installs only the opt-in depth-based spatial filter.
        .disable::<AntiAliasPlugin>();
    // The launcher owns the production process lifecycle. Keeping the
    // OS default SIGINT action also preserves a real developer escape
    // hatch if graceful Bevy teardown is wedged.
    #[cfg(not(target_arch = "wasm32"))]
    let plugins = plugins.disable::<TerminalCtrlCHandlerPlugin>();
    let plugins = plugins.set(logging::plugin());
    app.add_plugins(plugins);
    app.add_plugins(render::InputPacingPlugin::default())
        .init_resource::<crate::present_mode::DisplayRefresh>()
        .add_systems(First, crate::frame_pacing::track_display_refresh)
        .add_systems(Last, crate::frame_pacing::update_frame_pacing);
    #[cfg(target_os = "macos")]
    crate::thread_budget::ThreadBudget::configure_render_thread(&mut app);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, crate::window_icon::apply);
    #[cfg(target_arch = "wasm32")]
    {
        app.add_systems(Update, crate::startup_progress::report_ui_upload);
        crate::browser_links::configure(&mut app);
    }
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
            .with_relay(relay),
    )
    .insert_resource(ui_catalog)
    .insert_resource(client_blob_cache)
    .insert_resource(network)
    .insert_resource(ResourcePackAdmissionState::default())
    .insert_resource(render_scene::ActorArtworkResource(actor_artwork))
    .insert_resource(ClientWorld::new_with_entity_assets(
        Arc::clone(&runtime_assets),
        Arc::clone(&entity_runtime),
    ))
    .insert_resource(UiRuntimeResource({
        let mut ui_runtime = UiRuntime::new(0);
        ui_runtime.set_item_fuel_catalog(Arc::clone(&entity_runtime));
        ui_runtime.set_food_catalog(food_catalog);
        if let Some(address) = &args.address {
            ui_runtime.experiences.select_destination(address);
        }
        ui_runtime.set_lang_catalog(lang_assets.into_runtime());
        ui_runtime.set_active_language(active_lang);
        ui_runtime
    }))
    .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
        0,
    )))
    .insert_resource(UiPresentationResource(ui_presentation))
    .insert_resource(WorldClock::default())
    .insert_resource(WeatherState::default())
    .init_resource::<environment::WeatherTickFrame>()
    .insert_resource(environment::CameraMediumState::default())
    .insert_resource(environment::LightningFlashState::default())
    .init_resource::<environment::FogBindings>()
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
    .insert_resource(weather_textures)
    .insert_resource(
        crate::runtime::network::reload_environment::EnvironmentBase::new(
            atmosphere_assets.clone(),
            particle_assets.clone(),
        ),
    )
    .insert_resource(atmosphere_assets)
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
    .insert_resource(render_scene::BlockEntitySceneResource(block_entity_scene))
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
        const MAIN_FRAME: usize = render_scene::RuntimeStage::MainFrame as usize;
        app.insert_resource(RuntimeStageProfiler::for_gameplay(
            stage_profile_enabled,
            std::env::var_os(diagnostics::markers::STAGE_PROFILE_FRAMES)
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
    app.add_plugins((
        render::DepthSmaaPlugin,
        render::motion_blur::CameraMotionBlurPlugin,
    ));
    app.add_plugins(crate::render_mode::RenderModePlugin::new(
        args.render_mode,
        diagnostics_enabled,
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
    if let Some(identity) = phase3_identity_source {
        app.insert_resource(identity);
    }
    crate::global_resources::configure(&mut app, global_pack_root, args.import_packs);
    configure_client_production_frame_systems(&mut app);
    configure_client_runtime_frame_systems(&mut app);
    // A browser has no thread to service the stream between frames; the frame polls it.
    if cfg!(not(target_arch = "wasm32"))
        && let Some(service) = client_runtime::world_service::WorldServiceSlot::from_environment()?
    {
        app.insert_resource(service);
    }
    #[cfg(target_arch = "wasm32")]
    crate::runtime::world::frame_jobs::configure(&mut app);
    crate::modding::configure_from_environment(&mut app);
    #[cfg(feature = "developer-control")]
    crate::developer_control::configure(&mut app);
    app.add_plugins(client_extensions::ClientExtensionsPlugin);
    crate::server_experiences::configure(&mut app);
    if let Some(account) = relay_account {
        app.insert_resource(client_shell::menu::ControlAccount(account));
    }
    app.init_resource::<client_state::PresenceObservation>()
        .add_systems(
            Last,
            crate::shell_adapters::publish_presence_observation
                .before(client_state::ShellFrameSet::Presence),
        );
    configure_acceptance_finish_system(&mut app);

    #[cfg(feature = "enhanced-diagnostics")]
    crate::enhanced_diagnostics::install(&mut app, diagnostic_budget);
    executor::configure_frame_schedule_executors(&mut app);
    crate::startup_progress::report("Starting", None);
    let exit = app.run();
    finish_run(app, &exit, &shutdown_watchdog)
}

/// After the app exits: ends Discord presence and the network, then reports the exit.
#[cfg(not(target_arch = "wasm32"))]
fn finish_run(
    mut app: App,
    exit: &bevy::app::AppExit,
    shutdown_watchdog: &ShutdownWatchdog,
) -> Result<()> {
    client_shell::discord_presence::shutdown(&mut app);
    if let Some(mut network) = app
        .world_mut()
        .remove_resource::<NetworkHandle<PackApplication>>()
    {
        network.shutdown();
    }
    drop(app);
    let _ = writeln!(
        diagnostics::console::stderr(),
        "{SHUTDOWN_COMPLETED} exit_code={}",
        app_exit_code(exit)
    );
    diagnostics::console::flush_before_exit();
    shutdown_watchdog.complete();
    if exit.is_error() {
        bail!("Bevy app exited after a fatal runtime error");
    }
    Ok(())
}

/// A browser's event loop took the app and is still running it, so the page plays on.
#[cfg(target_arch = "wasm32")]
fn finish_run(
    _app: App,
    _exit: &bevy::app::AppExit,
    _shutdown_watchdog: &ShutdownWatchdog,
) -> Result<()> {
    Ok(())
}
