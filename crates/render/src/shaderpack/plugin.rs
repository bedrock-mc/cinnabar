//! Bevy wiring: the main-world load worker, extraction, the geometry adapter's settings and
//! block ids, the actor adapter's id maps and Vanilla hand-off, per-view preparation and the
//! render-graph system.

use super::actors::{self, ActorSkinPipelines, ActorSources, RigSource};
use super::atlas::{ShaderpackAtlas, ShaderpackAtlasPlugin};
use super::draws::DrawKind;
use super::effects::textures::{ShaderpackEffectTextures, prepare_effect_textures};
use super::effects::{Effect, EffectCamera, EffectInputs};
use super::frame_geometry::{FrameGeometry, TerrainDraws};
use super::geometry::{
    GeometryCategory, ShaderpackBlockIds, ShaderpackGeometry, ShaderpackGeometryPlugin,
    ShaderpackGeometrySettings, ShadowFrustum, UNASSIGNED_BLOCK_ID, device_geometry_budget,
};
use super::load_failures::{ViewFailures, receive_view_failures};
use super::loader::{PackGpu, load};
use super::passes::{DrawContext, FrameIo, ViewState, record_frame};
use super::pipelines::GpuContext;
use super::resources::vanilla::VanillaTextures;
use super::sky::SkyInputs;
use super::targets::AtlasViews;
use super::uniforms::{FAR_PLANE_SCALE, FrameClock, FrameUniforms, ViewCamera};
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
    ShaderPackActorIds, ShaderPackDrawnCategories, ShaderPackEffects, ShaderPackFrameInputs,
    ShaderPackIdMaps, ShaderPackLoadProgress, ShaderPackRendering, ShaderPackRequest,
    ShaderPackSettledRevision, ShaderPackStatus,
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

/// Java's first-person field of view, for the hand projection while no hand rig draws.
const DEFAULT_HAND_FOV_DEGREES: f32 = 70.0;
/// The least `far` a pack sees: Java's smallest render distance, two chunks.
const MIN_PACK_FAR: f32 = 32.0;

/// The loaded pack, shared with the render world.
#[derive(Resource, ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub(crate) struct LoadedShaderPack(pub Option<Arc<PackGpu>>);

type LoadResult = Result<PackGpu, String>;

/// Pack path, option-file path, dimension and revision of one load.
type LoaderKey = (PathBuf, Option<PathBuf>, i32, u64);

/// The pack being loaded or shown, including the option file it reads.
#[derive(Resource, Default)]
struct Loader {
    key: Option<LoaderKey>,
    job: Option<Mutex<mpsc::Receiver<LoadResult>>>,
    retiring: bool,
}

/// Identifies the request whose load can be reused in the current dimension.
fn loader_key(request: &ShaderPackRequest, dimension: i32) -> Option<LoaderKey> {
    request.pack.clone().map(|path| {
        (
            path,
            request.options.clone(),
            dimension.clamp(0, 2),
            request.revision,
        )
    })
}

/// The render device, queue and adapter Bevy also publishes in the main world.
type MainWorldGpu<'w> = (
    Option<Res<'w, RenderDevice>>,
    Option<Res<'w, RenderQueue>>,
    Option<Res<'w, RenderAdapter>>,
);

/// Starts a load when the requested pack, its revision or the dimension changes and
/// publishes results. A dimension change or reload of the shown pack keeps it on screen
/// until the new load is ready. The pack's `minecraft:` texture references resolve against
/// the vanilla textures loaded when its load starts.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading each resource the load starts from and publishes to"
)]
fn drive_loader(
    request: Res<ShaderPackRequest>,
    inputs: Res<ShaderPackFrameInputs>,
    mut status: ResMut<ShaderPackStatus>,
    mut settled: ResMut<ShaderPackSettledRevision>,
    mut loaded: ResMut<LoadedShaderPack>,
    mut progress: ResMut<ShaderPackLoadProgress>,
    mut loader: ResMut<Loader>,
    gpu: MainWorldGpu,
    atmosphere: Option<Res<render_scene::AtmosphereTextureAssets>>,
    compiler: Option<Res<render_scene::shaderpack::ShaderPackCompilerExecutable>>,
) {
    let wanted = loader_key(&request, inputs.dimension);
    if loader.job.is_some() && (wanted != loader.key || loader.retiring) {
        progress.finish();
        loader.retiring = true;
        let keep_shown = wanted.as_ref().is_some_and(
            |(path, ..)| matches!(&*status, ShaderPackStatus::Ready(shown) if shown == path),
        );
        if !keep_shown {
            loaded.0 = None;
            status.set_if_neq(
                wanted
                    .as_ref()
                    .map_or(ShaderPackStatus::Idle, |(path, ..)| {
                        ShaderPackStatus::Loading(path.clone())
                    }),
            );
        }
        // Keep the receiver until the old worker exits. Rapid selections coalesce into the
        // latest request rather than accumulating detached CPU and GPU load workers.
        if loader.job.as_mut().is_some_and(|job| {
            matches!(
                job.get_mut().unwrap_or_else(|e| e.into_inner()).try_recv(),
                Err(mpsc::TryRecvError::Empty)
            )
        }) {
            return;
        }
        loader.job = None;
        loader.key = None;
        loader.retiring = false;
    }
    if wanted != loader.key {
        let Some((path, _, dimension, _)) = wanted.clone() else {
            loader.key = None;
            loader.job = None;
            progress.finish();
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
        let options = request.options.clone();
        let compiler = compiler.as_deref().map(|compiler| compiler.0.clone());
        let vanilla = atmosphere
            .as_deref()
            .map(VanillaTextures::from_atmosphere)
            .unwrap_or_default();
        let (send, receive) = mpsc::channel();
        let worker_progress = progress.start();
        let spawned = std::thread::Builder::new()
            .name("shader pack load".into())
            .spawn(move || {
                let id = JAVA_DIMENSIONS[dimension as usize];
                let outcome = load(
                    &path,
                    options.as_deref(),
                    id,
                    cache_dir.as_deref(),
                    &gpu,
                    &vanilla,
                    &worker_progress,
                    compiler.as_deref(),
                );
                let _ = send.send(outcome);
            });
        match spawned {
            Ok(_) => loader.job = Some(Mutex::new(receive)),
            Err(error) => {
                progress.finish();
                if let Some((path, _, _, revision)) = &loader.key {
                    *status = ShaderPackStatus::Failed {
                        pack: path.clone(),
                        reason: error.to_string(),
                    };
                    settled.0 = *revision;
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
    progress.finish();
    let Some((path, _, _, revision)) = loader.key.clone() else {
        return;
    };
    settled.0 = revision;
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
pub(crate) struct ShaderPackViews {
    pub(crate) states: Mutex<HashMap<Entity, ViewState>>,
    failed: HashMap<Entity, (Arc<PackGpu>, [u32; 2])>,
}

type PreparedView = (Entity, &'static ExtractedView);

/// Enables the geometry adapter while a pack with terrain programs is shown on some camera,
/// with the pack's `separateAo`/`oldLighting` and a vertex budget sized to the device,
/// publishes its block ids, tells the Vanilla terrain, actor, block-entity and hand passes
/// which of them the pack draws itself, and publishes the effects it draws in place of
/// Vanilla's.
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

/// The Vanilla effects `pack` replaces: those it draws, and clouds and weather its
/// directives turn off.
fn replaced_effects(
    pack: &PackGpu,
    textures: Option<&ShaderpackEffectTextures>,
) -> ShaderPackEffects {
    let mut coverage = effect_coverage(
        |effect| {
            pack.draws
                .iter()
                .any(|d| d.kind == DrawKind::Effect(effect))
                && textures.is_some_and(|textures| textures.views(effect).is_some())
        },
        |effect| !pack.effects.draws(effect),
    );
    // Uncovered celestial layers leave the pack's sky intact.
    coverage.clouds = true;
    coverage
}

/// Keeps each native effect until that effect has a replacement or is explicitly disabled.
fn effect_coverage(
    draws: impl Fn(Effect) -> bool,
    disabled: impl Fn(Effect) -> bool,
) -> ShaderPackEffects {
    ShaderPackEffects {
        particles_opaque: draws(Effect::Particles),
        particles_translucent: draws(Effect::ParticlesTranslucent),
        particles_additive: draws(Effect::ParticlesAdditive),
        weather: draws(Effect::Weather) || disabled(Effect::Weather),
        lightning: draws(Effect::Lightning),
        clouds: draws(Effect::Clouds) || disabled(Effect::Clouds),
        selection: draws(Effect::Outline),
        cracks: draws(Effect::DamagedBlock),
    }
}

#[path = "native_scene.rs"]
mod native_scene;
use native_scene::replacement_ready;

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

/// Replaces native queues only after every pack view and terrain category is ready.
#[allow(clippy::too_many_arguments, reason = "native queue replacement inputs")]
pub(super) fn configure_replacement(
    pack: Option<Res<LoadedShaderPack>>,
    cameras: Query<Entity, With<ShaderPackRendering>>,
    geometry: Option<Res<ShaderpackGeometry>>,
    views: Res<ShaderPackViews>,
    mut drawn: ResMut<ShaderPackDrawnCategories>,
    mut replaced: ResMut<ShaderPackEffects>,
    textures: Option<Res<ShaderpackEffectTextures>>,
) {
    let Some(pack) = pack
        .as_ref()
        .and_then(|pack| pack.0.as_ref())
        .filter(|_| !cameras.is_empty())
    else {
        drawn.set_if_neq(ShaderPackDrawnCategories::default());
        replaced.set_if_neq(ShaderPackEffects::default());
        return;
    };
    let states = views.states.lock().unwrap_or_else(|e| e.into_inner());
    let ready = Some(pack).filter(|pack| {
        cameras.iter().all(|entity| {
            states.get(&entity).is_some_and(|state| {
                Arc::ptr_eq(&state.pack, pack) && state.geometry_targets_ready()
            })
        })
    });
    let replaces = ready
        .is_some_and(|pack| replacement_ready(pack, cameras.iter(), geometry.as_deref(), &states));
    drawn.set_if_neq(ShaderPackDrawnCategories {
        terrain: replaces,
        actors: replaces,
        block_entities: replaces,
        hand: replaces,
    });
    replaced.set_if_neq(
        ready
            .filter(|_| drawn.terrain)
            .map_or_else(Default::default, |pack| {
                replaced_effects(pack, textures.as_deref())
            }),
    );
}

/// Reads the atlas dimensions that current geometry bindings will sample.
fn atlas_context(
    atlas: Option<&ShaderpackAtlas>,
    effects: Option<&ShaderpackEffectTextures>,
) -> DrawContext {
    DrawContext {
        chunk_offset: Vec3::ZERO,
        hand_projection: Mat4::IDENTITY,
        atlas_size: atlas.map_or([0; 2], |atlas| {
            atlas.size.to_array().map(|side| side as i32)
        }),
        particle_atlas_size: effects.map_or([0; 2], ShaderpackEffectTextures::particle_atlas_size),
        crack_atlas_size: effects.map_or([0; 2], ShaderpackEffectTextures::crack_atlas_size),
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
    effect_textures: Option<Res<ShaderpackEffectTextures>>,
    hand: Option<Res<render_scene::HandRigScene>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
    failures: Option<Res<ViewFailures>>,
) {
    let ShaderPackViews { states, failed } = &mut *views;
    let states = states.get_mut().unwrap_or_else(|e| e.into_inner());
    let (Some(pack), Some(inputs)) = (pack.and_then(|p| p.0.clone()), inputs) else {
        states.clear();
        failed.clear();
        return;
    };
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::ShaderpackUniforms));
    let hand_fov = hand
        .as_ref()
        .and_then(|hand| hand.frame.as_ref())
        .map_or(DEFAULT_HAND_FOV_DEGREES.to_radians(), |frame| {
            frame.fov_radians
        });
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
                    if let Some(failures) = &failures {
                        failures.record(Arc::clone(&pack), error);
                    }
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
        let far = inputs.render_distance.max(16.0) * FAR_PLANE_SCALE;
        let context = DrawContext {
            chunk_offset,
            hand_projection: actors::hand_projection(hand_fov, screen, far),
            ..atlas_context(atlas.as_deref(), effect_textures.as_deref())
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
        .filter(|geometry| {
            geometry.supported() && geometry.all_sections_fit() && geometry.origin().is_some()
        })
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
    state.sync_draw_atlas_sizes(
        &queue,
        &atlas_context(
            world.get_resource::<ShaderpackAtlas>(),
            world.get_resource::<ShaderpackEffectTextures>(),
        ),
    );
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
    let effects = EffectInputs {
        coverage: world
            .get_resource::<ShaderPackEffects>()
            .copied()
            .unwrap_or_default(),
        camera: EffectCamera {
            position: view.world_from_view.translation(),
            model_view: Mat4::from_quat(view.world_from_view.rotation().inverse()),
            focal: [view.clip_from_view.x_axis.x, view.clip_from_view.y_axis.y],
            screen: [view.viewport.z.max(1), view.viewport.w.max(1)],
        },
        frame: inputs,
        atmosphere: world.get_resource::<render_scene::AtmosphereFrame>(),
        atmosphere_view: world
            .get_resource::<render_scene::AtmosphereViewInputs>()
            .copied(),
        atmosphere_textures: world.get_resource::<render_scene::AtmosphereTextureAssets>(),
        clouds_visible: world
            .get_resource::<render_scene::CloudVisibility>()
            .is_some_and(|visible| visible.0),
        particles: world.get_resource::<render_scene::ParticleGpuFrame>(),
        particle_atlas_side: world
            .get_resource::<ShaderpackEffectTextures>()
            .map_or(0, |textures| textures.particle_atlas_size()[0] as u32),
        precipitation: world.get_resource::<render_scene::PrecipitationScene>(),
        lightning: world.get_resource::<render_scene::LightningScene>(),
        selection: world
            .get_resource::<render_scene::BlockSelectionFrameResource>()
            .map(|frame| &frame.0),
        block_entities: world
            .get_resource::<render_scene::BlockEntityFrameResource>()
            .map(|frame| &frame.0),
        seconds: world
            .get_resource::<Time>()
            .map_or(0.0, |time| time.elapsed_secs()),
    };
    let actor_rig = crate::actor_render::shaderpack_source::shaderpack_actors(world);
    let hand_rig = crate::hand_rig_render::shaderpack_source::shaderpack_hand(world).filter(|_| {
        super::native_actors::hand_supported(world.get_resource(), world.get_resource())
    });
    let kernels = world
        .get_resource::<ActorSkinPipelines>()
        .zip(
            world
                .get_resource::<super::kernels::Kernels>()
                .map(|kernels| &kernels.cache),
        )
        .and_then(|(pipelines, cache)| pipelines.kernels(cache));
    let actors = ActorSources {
        kernels,
        actors: actor_rig.as_ref().map(|rig| rig as &dyn RigSource),
        hand: hand_rig.as_ref().map(|rig| rig as &dyn RigSource),
        block_entities: crate::block_entity::shaderpack_block_entities(world).filter(|_| {
            actors::compatibility::frame_supported(
                None,
                None,
                world
                    .get_resource::<render_scene::BlockEntityFrameResource>()
                    .map(|frame| &frame.0),
            )
        }),
        entity_ids: world.get_resource::<ShaderPackActorIds>(),
        held_items: [inputs.held_item_id, inputs.held_item_id2],
        camera: view.world_from_view.translation(),
        view_forward: view.world_from_view.rotation() * Vec3::NEG_Z,
    };
    let geometry = terrain.as_ref().map(|terrain| FrameGeometry {
        device: render_device,
        terrain,
        atlas,
        effect_textures: world.get_resource::<ShaderpackEffectTextures>(),
        sky: SkyInputs {
            sky_color: colours.sky,
            fog_color: colours.fog,
            render_distance: inputs.render_distance,
            celestial_angle: inputs.celestial_angle,
            camera_y: position.y,
            sunrise: inputs.dimension == 0,
        },
        effects,
        actors,
    });
    let post = target.post_process_write();
    let io = FrameIo {
        viewport: view.viewport.to_array(),
        scene_colour: post.source,
        scene_depth: crate::scene_sampling::view_depth(depth),
        scene_near: view.clip_from_view.w_axis.z,
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
    if io.geometry.is_some() {
        super::overlay_depth::restore(&device, context.command_encoder(), state, &io);
    }
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
            schedule.add_systems(shaderpack_post.in_set(ShaderpackPass).run_if(pack_loaded));
            super::native_passes::install(schedule);
        })
        .is_ok();
    if installed {
        world.insert_resource(GraphInstalled);
    }
}

/// Whether the render world holds a loaded pack; pack passes have nothing to draw otherwise.
pub(super) fn pack_loaded(pack: Option<Res<LoadedShaderPack>>) -> bool {
    pack.is_some_and(|pack| pack.0.is_some())
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
        .init_resource::<ShaderPackLoadProgress>()
        .init_resource::<ShaderPackSettledRevision>()
        .init_resource::<ShaderPackFrameInputs>()
        .init_resource::<ShaderPackIdMaps>()
        .init_resource::<ShaderPackActorIds>()
        .init_resource::<LoadedShaderPack>()
        .init_resource::<Loader>()
        .init_resource::<ViewFailures>()
        .add_plugins((
            ExtractComponentPlugin::<ShaderPackRendering>::default(),
            ExtractResourcePlugin::<LoadedShaderPack>::default(),
            ExtractResourcePlugin::<ShaderPackFrameInputs>::default(),
            ExtractResourcePlugin::<ShaderPackActorIds>::default(),
            ExtractResourcePlugin::<DedicatedGraphicsMemory>::default(),
            ShaderpackGeometryPlugin,
            ShaderpackAtlasPlugin,
        ))
        .add_systems(
            Update,
            (drive_loader, receive_view_failures, publish_id_maps).chain(),
        )
        .add_systems(PostUpdate, admit_cameras);
}

/// Queues actor conversion kernels only for a loaded pack with an opted-in camera.
fn prepare_actor_pipelines(
    mut commands: Commands,
    pack: Option<Res<LoadedShaderPack>>,
    cameras: Query<(), With<ShaderPackRendering>>,
    pipelines: Option<Res<ActorSkinPipelines>>,
    kernels: Option<Res<super::kernels::Kernels>>,
) {
    if pack.is_some_and(|pack| pack.0.is_some()) && !cameras.is_empty() && kernels.is_some() {
        if pipelines.is_none() {
            commands.init_resource::<ActorSkinPipelines>();
        }
    } else if pipelines.is_some() {
        commands.remove_resource::<ActorSkinPipelines>();
    }
}

pub(super) fn finish(app: &mut App) {
    app.init_resource::<ViewFailures>();
    let failures = app.world().resource::<ViewFailures>().clone();
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .insert_resource(failures)
        .init_resource::<ShaderPackViews>()
        .init_resource::<ShaderPackEffects>()
        .init_resource::<ShaderpackEffectTextures>()
        .init_resource::<ShaderPackDrawnCategories>()
        .init_resource::<crate::actor::gpu::ArtworkRawViews>()
        .add_systems(
            bevy::render::ExtractSchedule,
            crate::actor::gpu::extract_artwork_raw_views,
        )
        .add_systems(
            Render,
            (
                prepare_actor_pipelines
                    .after(super::kernels::prepare_kernels)
                    .before(prepare_views),
                configure_geometry
                    .after(RenderSystems::PrepareViews)
                    .before(RenderSystems::Queue)
                    .before(super::geometry::prepare_geometry_resources)
                    .before(super::geometry::prepare_shaderpack_geometry),
                prepare_effect_textures
                    .in_set(RenderSystems::PrepareResources)
                    .after(crate::particle_render::prepare_particle_resources)
                    .after(crate::block_entity::prepare_block_entity_resources),
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
#[path = "plugin_first_person_tests.rs"]
mod first_person_tests;

#[cfg(test)]
#[path = "plugin_camera_tests.rs"]
mod camera_tests;

#[cfg(test)]
#[path = "plugin_native_tests.rs"]
pub(super) mod native_tests;
