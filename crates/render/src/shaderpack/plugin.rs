//! Bevy wiring: the main-world load worker, extraction, the geometry adapter's settings and
//! block ids, the pack's id maps, the Vanilla hand-off, per-view preparation and the
//! render-graph system.

use super::atlas::{ShaderpackAtlas, ShaderpackAtlasPlugin};
use super::frame_geometry::{FrameGeometry, TerrainDraws};
use super::geometry::{
    GeometryCategory, ShaderpackBlockIds, ShaderpackGeometry, ShaderpackGeometryPlugin,
    ShaderpackGeometrySettings, ShadowFrustum, UNASSIGNED_BLOCK_ID, device_geometry_budget,
};
use super::loader::{PackGpu, load};
use super::passes::{DrawContext, FrameIo, ViewState, record_frame};
use super::pipelines::GpuContext;
use super::resources::vanilla::VanillaTextures;
use super::sky::SkyInputs;
use super::targets::AtlasViews;
use super::uniforms::{FrameClock, FrameUniforms, ViewCamera};
use assets::facts::java_mapping::JavaDimension;
use bevy::render::render_phase::TrackedRenderPass;
use bevy::{
    core_pipeline::{Core3d, Core3dSystems},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_component::ExtractComponentPlugin,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        renderer::{RenderAdapter, RenderContext, RenderDevice, RenderQueue, ViewQuery},
        view::{ExtractedView, ViewDepthStencilTexture, ViewTarget},
    },
};
use render_scene::device_memory::DedicatedGraphicsMemory;
use render_scene::shaderpack::{
    ShaderPackDrawnCategories, ShaderPackFrameInputs, ShaderPackIdMaps, ShaderPackRendering,
    ShaderPackRequest, ShaderPackStatus,
};
use render_scene::{RuntimeStage, RuntimeStageProfiler};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
};

/// Java ids of the built-in dimensions, indexed by Bedrock dimension id (Overworld, Nether,
/// End), which select the pack's dimension folder.
pub(crate) static JAVA_DIMENSIONS: std::sync::LazyLock<[&str; JavaDimension::ALL.len()]> =
    std::sync::LazyLock::new(|| JavaDimension::ALL.map(JavaDimension::identifier));

/// The least `far` a pack sees: Java's smallest render distance, two chunks.
const MIN_PACK_FAR: f32 = 32.0;

/// The loaded pack, shared with the render world.
#[derive(Resource, ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub(crate) struct LoadedShaderPack(pub Option<Arc<PackGpu>>);

type LoadResult = Result<PackGpu, String>;

/// The pack being loaded or shown, identified by path and dimension.
#[derive(Resource, Default)]
struct Loader {
    key: Option<(PathBuf, i32, u64)>,
    job: Option<Mutex<mpsc::Receiver<LoadResult>>>,
}

/// The render device, queue and adapter Bevy also publishes in the main world.
type MainWorldGpu<'w> = (
    Option<Res<'w, RenderDevice>>,
    Option<Res<'w, RenderQueue>>,
    Option<Res<'w, RenderAdapter>>,
);

/// Starts a load when the requested pack or the dimension changes and publishes results.
/// A dimension change of the shown pack keeps it on screen until the new folder is ready.
/// The pack's `minecraft:` texture references resolve against the vanilla textures loaded
/// when its load starts.
fn drive_loader(
    request: Res<ShaderPackRequest>,
    inputs: Res<ShaderPackFrameInputs>,
    mut status: ResMut<ShaderPackStatus>,
    mut loaded: ResMut<LoadedShaderPack>,
    mut loader: ResMut<Loader>,
    gpu: MainWorldGpu,
    atmosphere: Option<Res<render_scene::AtmosphereTextureAssets>>,
) {
    let wanted = request.pack.clone().map(|path| {
        (
            path,
            inputs.dimension.clamp(0, 2),
            request.reload_generation,
        )
    });
    if wanted != loader.key {
        let Some((path, dimension, _)) = wanted.clone() else {
            loader.key = None;
            loader.job = None;
            status.set_if_neq(ShaderPackStatus::Idle);
            loaded.0 = None;
            return;
        };
        let (Some(device), Some(queue), Some(adapter)) = gpu else {
            return;
        };
        loader.key = wanted;
        loader.job = None;
        let gpu = GpuContext {
            device: device.wgpu_device().clone(),
            queue: (**queue).clone(),
            adapter: (**adapter).clone(),
        };
        let reloading = matches!(&*status, ShaderPackStatus::Ready(shown) if *shown == path);
        if !reloading {
            status.set_if_neq(ShaderPackStatus::Loading(path.clone()));
            loaded.0 = None;
        }
        let cache_dir = request.cache_dir.clone();
        let vanilla = atmosphere
            .as_deref()
            .map(VanillaTextures::from_atmosphere)
            .unwrap_or_default();
        let (send, receive) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("shader pack load".into())
            .spawn(move || {
                let id = JAVA_DIMENSIONS[dimension as usize];
                let _ = send.send(load(&path, id, cache_dir.as_deref(), &gpu, &vanilla));
            });
        match spawned {
            Ok(_) => loader.job = Some(Mutex::new(receive)),
            Err(error) => {
                if let Some((path, _, _)) = &loader.key {
                    *status = ShaderPackStatus::Failed {
                        pack: path.clone(),
                        reason: error.to_string(),
                    };
                }
            }
        }
        return;
    }
    let Some(job) = loader.job.as_mut() else {
        return;
    };
    let received = job.get_mut().unwrap_or_else(|e| e.into_inner()).try_recv();
    let outcome = match received {
        Ok(outcome) => outcome,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => Err("the load worker stopped".to_string()),
    };
    loader.job = None;
    let Some((path, _, _)) = loader.key.clone() else {
        return;
    };
    match outcome {
        Ok(pack) => {
            loaded.0 = Some(Arc::new(pack));
            *status = ShaderPackStatus::Ready(path);
        }
        Err(reason) => {
            warn!(
                target: "cinnabar::shaderpack",
                pack = %path.display(),
                "shader pack unavailable, rendering Vanilla: {reason}"
            );
            loaded.0 = None;
            *status = ShaderPackStatus::Failed { pack: path, reason };
        }
    }
}

/// Render-world state of every view drawing through the pack.
#[derive(Resource, Default)]
struct ShaderPackViews {
    states: Mutex<HashMap<Entity, ViewState>>,
    failed: HashMap<Entity, (Arc<PackGpu>, [u32; 2])>,
}

type PreparedView = (Entity, &'static ExtractedView);

/// Enables the geometry adapter while a pack with terrain programs is shown on some camera,
/// with the pack's `separateAo`/`oldLighting` and a vertex budget sized to the device,
/// publishes its block ids and tells the Vanilla terrain passes whether the pack draws
/// terrain itself.
#[allow(clippy::too_many_arguments, reason = "a render-world system's inputs")]
fn configure_geometry(
    mut commands: Commands,
    pack: Option<Res<LoadedShaderPack>>,
    cameras: Query<(), With<ShaderPackRendering>>,
    mut settings: ResMut<ShaderpackGeometrySettings>,
    memory: Option<Res<DedicatedGraphicsMemory>>,
    (mut shown, mut revisions): (Local<Option<usize>>, Local<BlockIdRevisions>),
) {
    let pack = pack
        .and_then(|p| p.0.clone())
        .filter(|p| !p.draws.is_empty() && !cameras.is_empty());
    let wanted = ShaderpackGeometrySettings {
        enabled: pack.is_some(),
        separate_ao: pack.as_ref().is_some_and(|p| p.separate_ao),
        old_lighting: pack.as_ref().is_some_and(|p| p.old_lighting),
        budget_bytes: device_geometry_budget(memory.map_or(0, |memory| memory.0)),
        layout: pack
            .as_ref()
            .map_or(super::geometry::TerrainLayout::WIDE, |p| p.terrain_layout),
    };
    if *settings != wanted {
        *settings = wanted;
    }
    let key = pack.as_ref().map(|p| Arc::as_ptr(p) as usize);
    if *shown == key {
        return;
    }
    *shown = key;
    match pack.and_then(|p| p.block_ids.clone()) {
        Some(table) => commands.insert_resource(revisions.publish(table)),
        None => commands.remove_resource::<ShaderpackBlockIds>(),
    }
}

/// Whether native layers need the scene-copy path until the geometry host can compose them.
fn needs_native_scene(
    atmosphere: Option<&render_scene::AtmosphereFrame>,
    particles: Option<&render_scene::ParticleGpuFrame>,
    weather: Option<&render_scene::PrecipitationScene>,
    overlays: Option<&render_scene::ScreenOverlayScene>,
    nametags: Option<&render_scene::NametagSceneResource>,
) -> bool {
    atmosphere.is_some_and(|frame| frame.sky_kind() == render_scene::SkyKind::Overworld)
        || particles.is_some_and(|frame| !frame.blend.is_empty() || !frame.add.is_empty())
        || weather.is_some_and(|scene| !scene.layers.is_empty())
        || overlays.is_some_and(|scene| !scene.layers.is_empty())
        || nametags.is_some_and(|scene| !scene.records.is_empty())
}

/// Native layers whose pixels cannot yet be composed over replacement terrain.
#[derive(bevy::ecs::system::SystemParam)]
struct NativeSceneLayers<'w> {
    atmosphere: Option<Res<'w, render_scene::AtmosphereFrame>>,
    particles: Option<Res<'w, render_scene::ParticleGpuFrame>>,
    weather: Option<Res<'w, render_scene::PrecipitationScene>>,
    overlays: Option<Res<'w, render_scene::ScreenOverlayScene>>,
    nametags: Option<Res<'w, render_scene::NametagSceneResource>>,
}

impl NativeSceneLayers<'_> {
    /// Whether this frame needs the native scene colour to preserve its layers.
    fn needed(&self) -> bool {
        needs_native_scene(
            self.atmosphere.as_deref(),
            self.particles.as_deref(),
            self.weather.as_deref(),
            self.overlays.as_deref(),
            self.nametags.as_deref(),
        )
    }
}

/// Uses one replacement decision for view uniforms and the native queue handoff.
fn replacement_ready(
    pack: &Arc<PackGpu>,
    cameras: impl Iterator<Item = Entity>,
    geometry: Option<&ShaderpackGeometry>,
    states: &HashMap<Entity, ViewState>,
    native_scene: bool,
) -> bool {
    let mut cameras = cameras.peekable();
    !native_scene
        && geometry.is_some_and(|geometry| {
            geometry.supported() && geometry.origin().is_some() && geometry.all_sections_fit()
        })
        && super::draws::complete_terrain(|kind| pack.draws.iter().any(|draw| draw.kind == kind))
        && cameras.peek().is_some()
        && cameras.all(|entity| {
            states.get(&entity).is_some_and(|state| {
                Arc::ptr_eq(&state.pack, pack) && state.geometry_targets_ready()
            })
        })
}

/// Replaces native queues only after every pack view and terrain category is ready.
#[allow(clippy::too_many_arguments, reason = "one frame’s replacement inputs")]
fn configure_replacement(
    pack: Option<Res<LoadedShaderPack>>,
    cameras: Query<Entity, With<ShaderPackRendering>>,
    geometry: Option<Res<ShaderpackGeometry>>,
    views: Res<ShaderPackViews>,
    mut drawn: ResMut<ShaderPackDrawnCategories>,
    native: NativeSceneLayers,
) {
    let states = views.states.lock().unwrap_or_else(|e| e.into_inner());
    let terrain = pack
        .as_ref()
        .and_then(|pack| pack.0.as_ref())
        .is_some_and(|pack| {
            replacement_ready(
                pack,
                cameras.iter(),
                geometry.as_deref(),
                &states,
                native.needed(),
            )
        });
    drawn.set_if_neq(ShaderPackDrawnCategories { terrain });
}

/// Hands out the revisions [`ShaderpackBlockIds`] are published under, each one no earlier
/// table had. The geometry adapter keeps the revision it last derived terrain attributes from
/// while Vanilla shows between two packs, so counting on from the current resource, which
/// Vanilla removes, would hand the next pack that same revision and leave its terrain with the
/// previous pack's ids.
#[derive(Default)]
pub(super) struct BlockIdRevisions(u64);

impl BlockIdRevisions {
    /// `table`'s ids by sequential block state ([`UNASSIGNED_BLOCK_ID`] past its end) under a
    /// revision no earlier table had.
    pub(super) fn publish(&mut self, table: Arc<Vec<i32>>) -> ShaderpackBlockIds {
        self.0 += 1;
        ShaderpackBlockIds {
            block_entity_id: Arc::new(move |state| {
                table
                    .get(state as usize)
                    .copied()
                    .unwrap_or(UNASSIGNED_BLOCK_ID)
            }),
            revision: self.0,
        }
    }
}

/// Allocates or resizes view targets, writes this frame's uniforms and builds each view's
/// shadow-pass terrain list from the pack's shadow culling.
#[allow(clippy::too_many_arguments, reason = "one frame's view inputs")]
fn prepare_views(
    mut views: ResMut<ShaderPackViews>,
    pack: Option<Res<LoadedShaderPack>>,
    inputs: Option<Res<ShaderPackFrameInputs>>,
    cameras: Query<PreparedView, With<ShaderPackRendering>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    time: Option<Res<Time>>,
    mut geometry: Option<ResMut<ShaderpackGeometry>>,
    atlas: Option<Res<ShaderpackAtlas>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
    native: NativeSceneLayers,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::ShaderpackUniforms));
    let ShaderPackViews { states, failed } = &mut *views;
    let states = states.get_mut().unwrap_or_else(|e| e.into_inner());
    let (Some(pack), Some(inputs)) = (pack.and_then(|p| p.0.clone()), inputs) else {
        states.clear();
        failed.clear();
        return;
    };
    states.retain(|entity, _| cameras.contains(*entity));
    let render_distance = inputs.render_distance;
    if let Some(geometry) = geometry.as_deref_mut() {
        geometry.retain_shadow_views(|view| cameras.contains(view) && pack.shadow.is_some());
    }
    failed.retain(|entity, _| cameras.contains(*entity));
    let clock = FrameClock {
        delta_seconds: time.map_or(0.0, |t| t.delta_secs()),
    };
    for (entity, view) in &cameras {
        let screen = [view.viewport.z.max(1), view.viewport.w.max(1)];
        if failed
            .get(&entity)
            .is_some_and(|(previous, size)| Arc::ptr_eq(previous, &pack) && *size == screen)
        {
            continue;
        }
        failed.remove(&entity);
        if states
            .get(&entity)
            .is_none_or(|state| !state.matches(&pack, screen))
        {
            match ViewState::new(device.wgpu_device(), Arc::clone(&pack), screen) {
                Ok(state) => {
                    states.insert(entity, state);
                }
                Err(error) => {
                    states.remove(&entity);
                    failed.insert(entity, (Arc::clone(&pack), screen));
                    warn!(pack = %pack.name, "shader pack view could not be prepared: {error}");
                    continue;
                }
            }
        }
    }
    let replacement = replacement_ready(
        &pack,
        cameras.iter().map(|(entity, _)| entity),
        geometry.as_deref(),
        states,
        native.needed(),
    );
    let horizon = geometry
        .as_deref()
        .and_then(ShaderpackGeometry::budget_horizon);
    let inputs = pack_inputs(&inputs, horizon, replacement);
    for (entity, view) in &cameras {
        let Some(state) = states.get_mut(&entity) else {
            continue;
        };
        let screen = [view.viewport.z.max(1), view.viewport.w.max(1)];
        let (_, rotation, position) = view.world_from_view.to_scale_rotation_translation();
        let camera = ViewCamera {
            position,
            rotation,
            focal: [view.clip_from_view.x_axis.x, view.clip_from_view.y_axis.y],
            screen,
        };
        state.enter_world(inputs.biome.is_some());
        let frame = FrameUniforms::new(
            camera,
            &inputs,
            &state.pack.settings,
            clock,
            &mut state.history,
        );
        let chunk_offset = geometry
            .as_ref()
            .and_then(|g| g.chunk_offset(position.as_dvec3()))
            .unwrap_or(Vec3::ZERO);
        let atlas_size = atlas.as_ref().map_or([0; 2], |atlas| {
            atlas.size.to_array().map(|side| side as i32)
        });
        let context = DrawContext {
            chunk_offset,
            atlas_size,
        };
        state.write_uniforms(&queue, &frame, clock.delta_seconds, &context);
        if let Some(geometry) = geometry.as_deref_mut()
            && pack.shadow.is_some()
        {
            let light = frame
                .shadow_model_view()
                .inverse()
                .transform_vector3(Vec3::Z);
            let frustum = ShadowFrustum::new(
                &pack.settings.shadow,
                render_distance,
                position.as_dvec3(),
                frame.projection() * frame.model_view(),
                light,
            );
            geometry.prepare_shadow_view(entity, &frustum, &device, &queue);
        }
    }
}

type PostQuery = (
    &'static ViewTarget,
    &'static ViewDepthStencilTexture,
    &'static ExtractedView,
);

/// Restricts the far plane to expanded terrain only while the pack replaces that terrain.
/// Scene-copy frames retain the native scene's full distance for projection and depth reads.
fn pack_inputs(
    inputs: &ShaderPackFrameInputs,
    horizon: Option<f32>,
    replacement: bool,
) -> ShaderPackFrameInputs {
    let mut inputs = *inputs;
    if let Some(horizon) = horizon.filter(|_| replacement) {
        inputs.render_distance = inputs.render_distance.min(horizon.max(MIN_PACK_FAR));
    }
    inputs
}

/// The camera view's terrain through the geometry adapter.
struct ViewTerrain<'a> {
    geometry: &'a ShaderpackGeometry,
    view: Entity,
}

impl TerrainDraws for ViewTerrain<'_> {
    fn draw<'w>(
        &'w self,
        pass: &mut TrackedRenderPass<'w>,
        category: GeometryCategory,
        shadow: bool,
    ) -> u32 {
        self.geometry.draw(pass, category, self.view, shadow)
    }

    fn layout(&self) -> super::geometry::TerrainLayout {
        self.geometry.layout()
    }
}

/// The terrain the pack's frames draw, once it has an origin; until then frames copy the
/// Vanilla scene instead (`passes::copy_scene`).
fn pack_terrain(world: &World) -> Option<&ShaderpackGeometry> {
    world
        .get_resource::<ShaderPackDrawnCategories>()
        .filter(|drawn| drawn.terrain)?;
    world
        .get_resource::<ShaderpackGeometry>()
        .filter(|geometry| geometry.supported() && geometry.origin().is_some())
}

/// Whether the pack draws `view`'s sky and world itself this frame, so nothing of the Vanilla
/// scene behind its geometry reaches the screen: [`shaderpack_post`] takes the geometry
/// path, and the Vanilla overlay only keeps what is nearer than the pack's depth.
pub(crate) fn draws_world(world: &World, view: Entity) -> bool {
    pack_terrain(world).is_some()
        && world
            .get_resource::<ShaderPackViews>()
            .is_some_and(|views| {
                views
                    .states
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&view)
                    .is_some_and(|state| !state.pack.draws.is_empty())
            })
}

/// Runs the pack over the frame: its shadow pass, geometry and full-screen chain, and
/// presents `final`.
fn shaderpack_post(
    world: &World,
    query: ViewQuery<PostQuery, With<ShaderPackRendering>>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let entity = query.entity();
    let (target, depth, view) = query.into_inner();
    let Some(views) = world.get_resource::<ShaderPackViews>() else {
        return Ok(());
    };
    let mut states = views.states.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = states.get_mut(&entity) else {
        return Ok(());
    };
    let inputs = pack_inputs(
        &world
            .get_resource::<ShaderPackFrameInputs>()
            .copied()
            .unwrap_or_default(),
        world
            .get_resource::<ShaderpackGeometry>()
            .and_then(ShaderpackGeometry::budget_horizon),
        world
            .get_resource::<ShaderPackDrawnCategories>()
            .is_some_and(|drawn| drawn.terrain),
    );
    let render_device = world.resource::<RenderDevice>();
    let device = render_device.wgpu_device().clone();
    let queue: wgpu::Queue = (**world.resource::<RenderQueue>()).clone();
    let destination_format = target.main_texture_format();
    if !state.supports_presentation(destination_format) {
        return Ok(());
    }
    let terrain = pack_terrain(world).map(|geometry| ViewTerrain {
        geometry,
        view: entity,
    });
    let atlas = world.get_resource::<ShaderpackAtlas>().map(|atlas| {
        (
            AtlasViews {
                color: &atlas.color,
                normals: &atlas.normals,
                specular: &atlas.specular,
            },
            atlas.generation,
        )
    });
    let (_, rotation, position) = view.world_from_view.to_scale_rotation_translation();
    let colours = super::uniforms::sky_fog(&inputs, rotation, position);
    let geometry = terrain.as_ref().map(|terrain| FrameGeometry {
        device: render_device,
        terrain,
        atlas,
        sky: SkyInputs {
            sky_color: colours.sky,
            fog_color: colours.fog,
            render_distance: inputs.render_distance,
            celestial_angle: inputs.celestial_angle,
            camera_y: position.y,
            sunrise: inputs.dimension == 0,
        },
        overlay_radius: terrain.geometry.complete_radius().unwrap_or(f32::MAX),
    });
    let post = target.post_process_write();
    let io = FrameIo {
        viewport: view.viewport.to_array(),
        scene_colour: post.source,
        scene_depth: crate::scene_sampling::view_depth(depth),
        scene_near: view.clip_from_view.w_axis.z,
        inverse_focal: [
            view.clip_from_view.x_axis.x.recip(),
            view.clip_from_view.y_axis.y.recip(),
        ],
        render_distance: inputs.render_distance,
        fog_color: colours.fog,
        destination: post.destination,
        destination_format,
        geometry,
        timestamps: world.get_resource::<crate::gpu_timing::GpuTimestamps>(),
    };
    let _timer = world
        .get_resource::<RuntimeStageProfiler>()
        .map(|profiler| profiler.time(RuntimeStage::ShaderpackRecord));
    record_frame(&device, &queue, context.command_encoder(), state, &io);
    Ok(())
}

/// The pack's render-graph system: after the Vanilla scene (whose colour and depth stand
/// in for geometry the pack does not draw) and before tonemapping and the UI.
#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(crate) struct ShaderpackPass;

#[derive(Resource)]
struct GraphInstalled;

fn install_graph(world: &mut World) {
    if world.contains_resource::<GraphInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            use bevy::core_pipeline::tonemapping::tonemapping;
            schedule.configure_sets(
                ShaderpackPass
                    .after(bevy::post_process::bloom::bloom)
                    .before(tonemapping)
                    .in_set(Core3dSystems::PostProcess),
            );
            schedule.add_systems(shaderpack_post.in_set(ShaderpackPass));
        })
        .is_ok();
    if installed {
        world.insert_resource(GraphInstalled);
    }
}

/// Publishes the loaded pack's entity and item ids for the app's actor and held-item ids.
fn publish_id_maps(loaded: Res<LoadedShaderPack>, mut maps: ResMut<ShaderPackIdMaps>) {
    if !loaded.is_changed() {
        return;
    }
    let revision = maps.revision + 1;
    *maps = match &loaded.0 {
        Some(pack) => pack.actor_ids.lookups(revision),
        None => {
            let mut empty = ShaderPackIdMaps::default();
            empty.revision = revision;
            empty
        }
    };
}

/// Keeps opted-in cameras compatible with the host's single-sample depth reads.
fn admit_cameras(
    mut cameras: Query<(&mut bevy::render::view::Msaa, &mut Camera3d), With<ShaderPackRendering>>,
) {
    for (mut msaa, mut camera) in &mut cameras {
        msaa.set_if_neq(bevy::render::view::Msaa::Off);
        let usage = wgpu::TextureUsages::from(camera.depth_texture_usages)
            | wgpu::TextureUsages::TEXTURE_BINDING;
        if wgpu::TextureUsages::from(camera.depth_texture_usages) != usage {
            camera.depth_texture_usages = usage.into();
        }
    }
}

pub(super) fn build(app: &mut App) {
    app.init_resource::<ShaderPackRequest>()
        .init_resource::<ShaderPackStatus>()
        .init_resource::<ShaderPackFrameInputs>()
        .init_resource::<ShaderPackIdMaps>()
        .init_resource::<LoadedShaderPack>()
        .init_resource::<Loader>()
        .add_plugins((
            ExtractComponentPlugin::<ShaderPackRendering>::default(),
            ExtractResourcePlugin::<LoadedShaderPack>::default(),
            ExtractResourcePlugin::<ShaderPackFrameInputs>::default(),
            ExtractResourcePlugin::<DedicatedGraphicsMemory>::default(),
            ShaderpackGeometryPlugin,
            ShaderpackAtlasPlugin,
        ))
        .add_systems(Update, (drive_loader, publish_id_maps).chain())
        .add_systems(PostUpdate, admit_cameras);
}

pub(super) fn finish(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<ShaderPackViews>()
        .init_resource::<ShaderPackDrawnCategories>()
        .add_systems(
            Render,
            (
                configure_geometry
                    .after(RenderSystems::PrepareViews)
                    .before(RenderSystems::Queue)
                    .before(super::geometry::prepare_shaderpack_geometry),
                prepare_views
                    .after(RenderSystems::PrepareViews)
                    .before(RenderSystems::Queue)
                    .after(bevy::render::view::prepare_view_targets)
                    .after(super::geometry::prepare_shaderpack_geometry),
                crate::chunk::shaderpack_sources::before_native_terrain_queues(
                    configure_replacement.after(prepare_views),
                ),
                super::lightmap::update_lightmap.in_set(RenderSystems::PrepareResources),
            ),
        );
    install_graph(render_app.world_mut());
}

#[cfg(test)]
#[path = "plugin_tests.rs"]
mod post_tests;

#[cfg(test)]
mod camera_tests {
    use super::*;
    use bevy::render::view::Msaa;

    #[test]
    fn depthless_native_layers_keep_the_scene_copy_path() {
        let mut overlays = render_scene::ScreenOverlayScene::default();
        let mut nametags = render_scene::NametagSceneResource::default();
        assert!(!needs_native_scene(
            None,
            None,
            None,
            Some(&overlays),
            Some(&nametags)
        ));
        overlays.layers.push(render_scene::ScreenOverlayLayer {
            kind: render_scene::ScreenOverlayKind::SpyglassScope,
            rgb: [1.0; 3],
            alpha: 1.0,
        });
        assert!(needs_native_scene(None, None, None, Some(&overlays), None));
        nametags
            .0
            .records
            .push(render_model::NametagRecord::default());
        assert!(needs_native_scene(None, None, None, None, Some(&nametags)));
    }

    #[test]
    fn scene_copy_inputs_retain_the_native_far_plane() {
        let inputs = ShaderPackFrameInputs {
            render_distance: MIN_PACK_FAR * 8.0,
            ..default()
        };
        let horizon = MIN_PACK_FAR * 2.0;
        assert_eq!(
            pack_inputs(&inputs, Some(horizon), false).render_distance,
            inputs.render_distance
        );
        assert_eq!(
            pack_inputs(&inputs, Some(horizon), true).render_distance,
            horizon
        );
        assert_eq!(
            pack_inputs(&inputs, None, true).render_distance,
            inputs.render_distance
        );
    }

    #[test]
    fn unresolved_native_layers_keep_the_scene_copy_path() {
        assert!(!needs_native_scene(None, None, None, None, None));
        assert!(needs_native_scene(
            Some(&render_scene::AtmosphereFrame::default()),
            None,
            None,
            None,
            None
        ));
        for additive in [false, true] {
            let mut particles = render_scene::ParticleGpuFrame::default();
            let instances = std::sync::Arc::from([particles::ParticleInstance::default()]);
            if additive {
                particles.add = instances;
            } else {
                particles.blend = instances;
            }
            assert!(needs_native_scene(None, Some(&particles), None, None, None));
        }
    }

    #[test]
    fn replacement_is_published_before_native_queue_consumers() {
        #[derive(Resource, Default)]
        struct QueuedTerrain(bool);
        /// Captures the replacement decision observed by a native queue.
        fn native_queue(drawn: Res<ShaderPackDrawnCategories>, mut queued: ResMut<QueuedTerrain>) {
            queued.0 = drawn.terrain;
        }
        let mut world = World::new();
        world.init_resource::<ShaderPackViews>();
        world.insert_resource(ShaderPackDrawnCategories { terrain: true });
        world.init_resource::<QueuedTerrain>();
        let mut schedule = bevy::ecs::schedule::Schedule::default();
        schedule.configure_sets(
            (
                RenderSystems::PrepareViews,
                RenderSystems::Queue,
                RenderSystems::PrepareResources,
            )
                .chain(),
        );
        schedule.add_systems((
            crate::chunk::shaderpack_sources::before_native_terrain_queues(configure_replacement),
            native_queue.in_set(RenderSystems::Queue),
        ));
        schedule.run(&mut world);
        assert!(!world.resource::<QueuedTerrain>().0);
    }

    #[test]
    fn opted_in_cameras_admit_single_sample_depth_sampling() {
        let mut app = App::new();
        app.init_resource::<Assets<bevy::shader::Shader>>();
        build(&mut app);
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), Msaa::Sample4, ShaderPackRendering))
            .id();
        app.update();
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
        assert!(
            wgpu::TextureUsages::from(
                app.world()
                    .get::<Camera3d>(camera)
                    .unwrap()
                    .depth_texture_usages
            )
            .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        );
        *app.world_mut().get_mut::<Msaa>(camera).unwrap() = Msaa::Sample8;
        app.update();
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
    }
}
