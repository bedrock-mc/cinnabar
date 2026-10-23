//! Browser composition of the existing game owners from verified, decoded carriers.
use std::sync::Arc;

use anyhow::Result;
use assets::carriers::startup::delivery::StartupCarriers;
use bevy::prelude::*;
use client_interaction::movement::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
    MovementTicker, PhysicsAuthorityGate, PhysicsCollisionRegistries,
};
use client_runtime::{
    network::NetworkHandle, pack_admission::ResourcePackAdmissionState,
    publication::PublicationController,
};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::{
    UiRuntime,
    presentation::{UiPresentationRuntime, forms::PreparedJsonUi},
};
use render_scene::{
    ActorRenderScene, AtmosphereFrame, AtmosphereTextureAssets, ChunkTextureAssets,
    WeatherTextureAssets,
};

use super::session::{Host, HostCommands};
use crate::browser_input::BrowserOptions;
use crate::{
    environment,
    runtime::{network, world::ClientWorld},
};

/// An unavailable optional remote image is an explicit failed source, never a filesystem path.
struct UnavailableImages;
impl ui_engine::sources::RemoteSource for UnavailableImages {
    fn state(&self, _: &str) -> ui_engine::sources::RemoteImage {
        ui_engine::sources::RemoteImage::Failed
    }
}

/// Constructs the same world, collision, UI and actor owners used by the native application.
pub(super) fn install(
    app: &mut App,
    startup: StartupCarriers,
    identity: protocol::ClientRandomId,
    audio: Option<client_audio::audio::SoundBank>,
    output: client_audio::named_audio::AudioDevice,
) -> Result<(Host, HostCommands)> {
    let entities = Arc::new(startup.entities);
    network::resource_packs::set_base_world_assets(Arc::clone(&startup.world));
    let artwork = crate::asset_bindings::actor_artwork(&startup.actors, &entities);
    let hand_geometry = render_model::ViewmodelGeometry::from_runtime(&entities, &artwork);
    let icons = Arc::new(startup.icons);
    network::install_vanilla_item_assets(&entities, &icons);
    let (equipment, artwork, geometries) =
        client_presentation::presentation::equipment::EquipmentRuntime::build(
            Arc::clone(&entities),
            None,
            Arc::clone(&icons),
            Some(Arc::clone(&startup.world)),
            None,
            artwork,
        );
    let (flames, artwork) =
        client_presentation::actor_publication::WorldActorFlames::prepare(artwork, None);
    let mut scene =
        ActorRenderScene::with_runtime_entity_assets_and_equipment(&entities, &geometries)
            .map_err(|error| anyhow::anyhow!("prepare actor geometry: {error:?}"))?;
    scene.configure_artwork(artwork.clone());
    network::set_base_actor_artwork(artwork.clone(), Arc::clone(&entities));
    let rig =
        client_presentation::actor_publication::HandRigBuilder::from_runtime_assets(&entities)?;
    let mut block_entities = crate::block_entities::block_entity_scene(None);
    block_entities.install_entity_assets(&entities);
    network::block_entity_art::install_base(None, &block_entities);
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )?;
    let collisions = PhysicsCollisionRegistries::from_decoded(&records, &startup.physics)?;
    let font = match startup.font {
        Some(font) => font,
        None => Arc::new(assets::carriers::startup::diagnostic_font_catalog()?),
    };
    let mut ui = UiPresentationRuntime::with_hud_and_icons(
        Arc::clone(&font),
        Arc::new(startup.hud),
        Arc::clone(&icons),
    )?;
    let glint = startup.item_glint;
    let prepared =
        PreparedJsonUi::from_loaded(client_ui::ui_runtime::json_ui_assets::LoadedUiAssets {
            assets: Arc::new(startup.ui),
            item_glint: Arc::new(
                render_model::UiTexturePage::owned([glint.width, glint.height], glint.rgba8)
                    .map_err(|error| anyhow::anyhow!("invalid item glint: {error:?}"))?,
            ),
        })
        .map_err(anyhow::Error::msg)?;
    let origin = web_time::Instant::now();
    ui.enable_prepared_json_ui_with_sources(
        prepared,
        Arc::new(UnavailableImages),
        ui_engine::sources::ImageWork {
            clock: Some(Arc::new(move || origin.elapsed())),
            workers: true,
        },
    )
    .map_err(anyhow::Error::msg)?;
    ui.set_equipment_catalog(None);
    ui.set_gui_models(&startup.world, &entities)?;
    let catalog = network::PackUiCatalog(
        ui.pack_catalog_base()
            .ok_or_else(|| anyhow::anyhow!("missing JSON-UI catalog"))?,
    );
    let skin =
        client_shell::player_skin::LocalPlayerSkin::generated_default(launcher::PRODUCT_NAME);
    let preparation = network::session::Preparation {
        registry_base: network::session_registries::RegistryBase::new(&startup.world, &collisions),
        session_generation: simulation_clock::identity::SessionGeneration::new(1),
        actor_artwork: Some(artwork.clone()),
        ui_catalog: Some(Arc::clone(&catalog.0)),
        physical_memory_bytes: 0,
    };
    let host = Host::new(preparation, skin.to_client_skin(), identity);
    let atmosphere = AtmosphereTextureAssets::new(startup.atmosphere, startup.atmosphere_identity);
    let weather = WeatherTextureAssets::default();
    let mut runtime = UiRuntime::new(0);
    runtime.set_item_fuel_catalog(Arc::clone(&entities));
    runtime.set_food_catalog(None);
    runtime.set_lang_catalog(Arc::new(startup.language));
    let network = NetworkHandle::<network::resource_packs::PackApplication>::disconnected();
    let movement = MovementTicker::with_epoch_publisher(network.physics_epoch_publisher());
    app.insert_resource(BrowserOptions::default())
        .insert_resource(catalog)
        .insert_resource(network)
        .init_resource::<ResourcePackAdmissionState>()
        .insert_resource(render_scene::ActorArtworkResource(artwork))
        .insert_resource(flames)
        .insert_resource(ClientWorld::new_with_entity_assets(
            Arc::clone(&startup.world),
            entities,
        ))
        .init_resource::<crate::runtime::shutdown::FatalError>()
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            0,
        )))
        .insert_resource(UiPresentationResource(ui))
        .init_resource::<client_presentation::environment::WorldClock>()
        .init_resource::<client_presentation::environment::WeatherState>()
        .init_resource::<client_presentation::environment::weather::WeatherTickFrame>()
        .init_resource::<environment::CameraMediumState>()
        .init_resource::<client_presentation::environment::weather::LightningFlashState>()
        .init_resource::<client_presentation::environment::fog::FogBindings>()
        .init_resource::<client_presentation::environment::EnvironmentContext>()
        .init_resource::<client_presentation::environment::EnvironmentProfileRoute>()
        .insert_resource(movement)
        .init_resource::<client_audio::session_audio::SessionAudioCatalog>()
        .insert_resource(client_audio::named_audio::NamedAudio::new(None))
        .insert_resource(client_audio::audio::AudioEngine::new(audio))
        .insert_non_send(output)
        .insert_resource(PhysicsAuthorityGate::ProductionEnabled)
        .insert_resource(skin)
        .init_resource::<LocalPhysicsController>()
        .init_resource::<LocalMovementEffectTimeline>()
        .init_resource::<LocalMovementSpeedAuthority>()
        .insert_resource(collisions)
        .insert_resource(equipment)
        .insert_resource(rig)
        .init_resource::<AtmosphereFrame>()
        .insert_resource(weather.clone())
        .insert_resource(network::reload_environment::EnvironmentBase::new(
            atmosphere.clone(),
            None,
            weather,
        ))
        .insert_resource(atmosphere)
        .insert_resource(crate::runtime::world::startup_biome_tints(&startup.world))
        .insert_resource(ChunkTextureAssets::new(startup.world))
        .init_resource::<crate::runtime::visibility::CaveVisibilityCache>()
        .insert_resource(render_scene::VisibilityDiagnosticsInput::new(false))
        .insert_resource(crate::runtime::visibility::AppMetrics(
            diagnostics::metrics::MetricsCollector::default(),
        ))
        .init_resource::<crate::runtime::visibility::DiagnosticQuads>()
        .insert_resource(render_scene::BlockEntitySceneResource(block_entities))
        .insert_resource(PublicationController::new(
            render_api::PublicationServiceConfig::DEFAULT,
        ))
        .insert_resource(crate::particles::ParticleIcons(icons))
        .init_resource::<client_presentation::presentation::viewmodel::HandAdapter>();
    network::install_actor_scene(app.world_mut(), scene);
    if let Some(geometry) = hand_geometry {
        app.insert_resource(render_scene::ViewmodelGeometryResource(geometry));
    }
    crate::block_entities::configure(app, font);
    crate::block_selection::configure(app);
    crate::particles::configure_particles(app);
    Ok(host)
}
