//! Prepares world resources after the first menu frame and installs them atomically.

use super::*;
use bevy::{
    ecs::world::CommandQueue,
    prelude::{App, IntoScheduleConfigs, Last, Resource, World},
};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use render_scene::startup_work::WorldWorkAdmission;

/// The world-only carriers, kept separate from menu fonts and interface data.
pub(in crate::app) struct WorldCarriers {
    pub core: Result<CoreCarriers, AssetStartupError>,
    pub block_entities: Option<Arc<RuntimeBlockEntityAssets>>,
    pub particles: Option<Arc<RuntimeParticleAssets>>,
    pub weather: render_scene::WeatherTextureAssets,
    pub collision: Result<PhysicsCollisionRegistries>,
}

/// An eager direct join or the still-unread inputs of a menu launch.
pub(in crate::app) enum WorldInput {
    Loaded(Box<WorldCarriers>),
    Deferred {
        selection: AssetSelection,
        physics: PathBuf,
    },
}

/// A join may consume world data only after the complete prepared bundle is installed.
#[derive(Resource)]
pub(crate) struct WorldReady;

/// Shared menu carriers needed to build static GUI item sources off the frame.
pub(in crate::app) struct WorldUi {
    pub menu: asset_startup::MenuAssets,
    pub icons: Arc<assets::RuntimeIconCatalog>,
    pub gui: client_ui::ui_runtime::presentation::GuiModelPreparation,
    pub vanilla_pack: PathBuf,
}

#[derive(Resource)]
struct PendingWorld {
    input: Option<Box<dyn FnOnce() -> Result<CommandQueue> + Send + Sync>>,
    worker: Option<std::thread::JoinHandle<Result<CommandQueue>>>,
}

/// Owns either a validated direct-join bundle or an unstarted menu preparation.
pub(in crate::app) enum WorldPreparation {
    Ready(CommandQueue),
    Deferred(Box<dyn FnOnce() -> Result<CommandQueue> + Send + Sync>),
}

impl WorldPreparation {
    /// Validates direct joins before any core starts; menu launches retain unread world inputs.
    pub(in crate::app) fn new(input: WorldInput, ui: WorldUi, direct: bool) -> Result<Self> {
        if direct {
            Ok(Self::Ready(prepare(input, ui)?))
        } else {
            Ok(Self::Deferred(Box::new(move || prepare(input, ui))))
        }
    }

    /// Installs complete direct resources or a deferred worker behind first-menu admission.
    pub(in crate::app) fn install(self, app: &mut App) {
        match self {
            Self::Ready(mut queue) => {
                app.insert_resource(WorldWorkAdmission::default());
                queue.apply(app.world_mut());
            }
            Self::Deferred(prepare) => {
                app.insert_resource(WorldWorkAdmission::after_menu())
                    .insert_resource(PendingWorld {
                        input: Some(prepare),
                        worker: None,
                    })
                    .add_systems(Last, poll.after(super::timing::log_presented_menu));
            }
        }
    }
}

/// Starts after the menu report and publishes a complete bundle for the next Update.
fn poll(world: &mut World) {
    if !world.resource::<WorldWorkAdmission>().is_admitted() {
        return;
    }
    let Some(mut pending) = world.remove_resource::<PendingWorld>() else {
        return;
    };
    if let Some(prepare) = pending.input.take() {
        pending.worker = Some(std::thread::spawn(prepare));
    }
    if pending
        .worker
        .as_ref()
        .is_some_and(std::thread::JoinHandle::is_finished)
    {
        let result = pending
            .worker
            .take()
            .expect("finished worker")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        match result {
            Ok(mut queue) => queue.apply(world),
            Err(error) => {
                crate::runtime::shutdown::record_fatal_error(
                    &mut world
                        .resource_mut::<crate::runtime::shutdown::FatalError>()
                        .0,
                    format!("World startup failed: {error:#}"),
                );
            }
        }
    } else {
        world.insert_resource(pending);
    }
}

/// Loads and prepares every world-only owner; no live UI or session state is borrowed here.
fn prepare(input: WorldInput, ui: WorldUi) -> Result<CommandQueue> {
    let times = LoadTimes::default();
    let WorldCarriers {
        core,
        block_entities,
        particles,
        weather,
        collision,
    } = match input {
        WorldInput::Loaded(carriers) => *carriers,
        WorldInput::Deferred { selection, physics } => {
            let path = selection.path.clone();
            std::thread::scope(|scope| {
                let collision = scope.spawn(|| load_collision_registries(&physics, &path));
                let block_entities =
                    scope.spawn(|| crate::block_entities::load_block_entity_carrier(&path));
                let particles = scope.spawn(|| crate::particles::load_optional_carrier(&path));
                let weather =
                    scope.spawn(|| crate::environment::load_optional_weather_textures(&path));
                let core = asset_startup::load_world_assets_observed(
                    selection.clone(),
                    ui.menu.clone(),
                    &times,
                )
                .and_then(|assets| finish_core(assets, &times));
                WorldCarriers {
                    core,
                    collision: join(collision),
                    block_entities: join(block_entities),
                    particles: join(particles),
                    weather: join(weather),
                }
            })
        }
    };
    let mut core = core.context("load startup world assets")?;
    core.publish();
    let CoreCarriers {
        assets,
        actor,
        equipment,
        authored_texture_loading,
    } = core;
    super::super::logging::loaded_assets(&assets);
    let entities = Arc::clone(assets.entities.runtime());
    let actor = actor.context("load exact entity-linked neutral actor artwork")?;
    let artwork = crate::asset_bindings::actor_artwork(&actor, &entities);
    let hand_geometry = render_model::ViewmodelGeometry::from_runtime(&entities, &artwork);
    crate::runtime::network::install_vanilla_item_assets(&entities, &ui.icons);
    let (equipment_runtime, artwork, geometries) =
        client_presentation::presentation::equipment::EquipmentRuntime::build(
            Arc::clone(&entities),
            equipment.clone(),
            Arc::clone(&ui.icons),
            Some(Arc::clone(&assets.runtime)),
            block_entities.clone(),
            artwork,
        );
    let flame = particles
        .as_deref()
        .and_then(|assets| assets.texture(assets::ACTOR_FLAME_TEXTURE));
    let (flames, artwork) =
        client_presentation::actor_publication::WorldActorFlames::prepare(artwork, flame);
    let mut block_scene = crate::block_entities::block_entity_scene(block_entities.as_deref());
    block_scene.install_entity_assets(&entities);
    crate::runtime::network::block_entity_art::install_base(block_entities, &block_scene);
    crate::runtime::network::set_base_actor_artwork(artwork.clone(), entities.clone());
    let gui = render_scene::startup_timing::measure("gui_models", || {
        ui.gui
            .prepare(&assets.runtime, &entities, equipment.as_deref())
    })
    .context("prepare native GUI model geometry and original texture pages")?;
    let mut actors = render_scene::ActorRenderScene::with_runtime_entity_assets_and_equipment(
        &entities,
        &geometries,
    )
    .map_err(|error| anyhow::anyhow!("prepare actor geometry: {error:?}"))?;
    actors.configure_artwork(artwork.clone());
    if let Some(geometry) = flames.geometry() {
        actors
            .insert_geometry(geometry)
            .map_err(|error| anyhow::anyhow!("prepare actor flames: {error:?}"))?;
    }
    let hands =
        client_presentation::actor_publication::HandRigBuilder::from_runtime_assets(&entities)?;
    let collision = collision?;
    bevy::log::info!(
        "loaded {} authoritative collision records for local physics",
        collision.available_record_count()
    );
    let biome = crate::runtime::world::startup_biome_tints(&assets.runtime);
    let metrics = assets.metrics.into();
    let runtime = assets.runtime;
    let mut queue = CommandQueue::default();
    queue.push(move |world: &mut World| {
        let mut presentation = world.resource_mut::<UiPresentationResource>();
        presentation.set_equipment_catalog(equipment.clone());
        presentation.set_form_texture_fallbacks(&entities, ui.vanilla_pack);
        presentation.set_gui_particle_assets(particles.clone());
        let flame = particles
            .as_deref()
            .and_then(|assets| assets.texture(assets::ACTOR_FLAME_TEXTURE));
        if let Err(error) = presentation
            .install_gui_model_sources(gui)
            .and_then(|()| presentation.set_gui_fire_texture(flame))
        {
            world
                .resource_mut::<crate::runtime::shutdown::FatalError>()
                .0 = Some(format!("GUI startup failed: {error}"));
            return;
        }
        let mut ui_runtime = world.resource_mut::<UiRuntimeResource>();
        ui_runtime.set_item_fuel_catalog(Arc::clone(&entities));
        ui_runtime.set_food_catalog(equipment);
        world
            .resource_mut::<crate::runtime::visibility::AppMetrics>()
            .0
            .set_asset_metrics(metrics);
        world.insert_resource(crate::runtime::world::ClientWorld::new_with_entity_assets(
            Arc::clone(&runtime),
            entities,
        ));
        world.insert_resource(render_scene::ActorArtworkResource(artwork));
        world.insert_resource(flames);
        world.insert_resource(actors);
        world.insert_resource(equipment_runtime);
        world.insert_resource(hands);
        world.insert_resource(collision);
        world.insert_resource(biome);
        world.insert_resource(render_scene::ChunkTextureAssets::new(runtime));
        world.insert_resource(authored_texture_loading);
        world.insert_resource(render_scene::BlockEntitySceneResource(block_scene));
        if let Some(geometry) = hand_geometry {
            world.insert_resource(render_scene::ViewmodelGeometryResource(geometry));
        }
        if let Some(assets) = &particles {
            world.insert_resource(render_scene::ParticleSimulation(
                particles::ParticleSystem::from_assets(assets),
            ));
        }
        let atmosphere = world
            .resource::<render_scene::AtmosphereTextureAssets>()
            .clone();
        world.insert_resource(
            crate::runtime::network::reload_environment::EnvironmentBase::new(
                atmosphere,
                particles,
                weather.clone(),
            ),
        );
        world.insert_resource(weather);
        world.insert_resource(WorldReady);
    });
    Ok(queue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn world_preparation_never_starts_before_menu_completion_and_publishes_once() {
        let mut world = World::new();
        let admission = WorldWorkAdmission::after_menu();
        world.insert_resource(admission.clone());
        let count = Arc::new(AtomicUsize::new(0));
        let loading = count.clone();
        world.insert_resource(PendingWorld {
            input: Some(Box::new(move || {
                loading.fetch_add(1, Ordering::Relaxed);
                let mut queue = CommandQueue::default();
                queue.push(|world: &mut World| {
                    world.insert_resource(WorldReady);
                });
                Ok(queue)
            })),
            worker: None,
        });
        world.run_system_once(poll).unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert!(world.resource::<PendingWorld>().worker.is_none());
        assert!(!world.contains_resource::<WorldReady>());
        admission.admit();
        test_time::eventually("complete world bundle published", || {
            world.run_system_once(poll).unwrap();
            world.contains_resource::<WorldReady>()
        });
        world.run_system_once(poll).unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
