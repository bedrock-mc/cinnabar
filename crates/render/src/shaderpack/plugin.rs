//! Bevy wiring: the main-world load worker, extraction, the pack's id maps, per-view
//! preparation and the render-graph system.

use super::loader::{PackGpu, load};
use super::passes::{FrameIo, ViewState, record_frame};
use super::pipelines::GpuContext;
use super::resources::vanilla::VanillaTextures;
use super::uniforms::{FrameClock, FrameUniforms, ViewCamera};
use assets::facts::java_mapping::JavaDimension;
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
use render_scene::shaderpack::{
    ShaderPackFrameInputs, ShaderPackIdMaps, ShaderPackRendering, ShaderPackRequest,
    ShaderPackStatus,
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

/// Allocates or resizes view targets and writes this frame's uniforms.
#[allow(clippy::too_many_arguments, reason = "one frame's view inputs")]
fn prepare_views(
    mut views: ResMut<ShaderPackViews>,
    pack: Option<Res<LoadedShaderPack>>,
    inputs: Option<Res<ShaderPackFrameInputs>>,
    cameras: Query<PreparedView, With<ShaderPackRendering>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    time: Option<Res<Time>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
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
        let state = states.get_mut(&entity).expect("the view was prepared");
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
        state.write_uniforms(&queue, &frame, clock.delta_seconds);
    }
}

type PostQuery = (
    &'static ViewTarget,
    &'static ViewDepthStencilTexture,
    &'static ExtractedView,
);

/// Runs the pack's full-screen chain over the frame's Vanilla scene and presents `final`.
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
    let inputs = world
        .get_resource::<ShaderPackFrameInputs>()
        .copied()
        .unwrap_or_default();
    let device = world.resource::<RenderDevice>().wgpu_device().clone();
    let queue: wgpu::Queue = (**world.resource::<RenderQueue>()).clone();
    let destination_format = target.main_texture_format();
    let (_, rotation, position) = view.world_from_view.to_scale_rotation_translation();
    let colours = super::uniforms::sky_fog(&inputs, rotation, position);
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
        ))
        .add_systems(Update, (drive_loader, publish_id_maps).chain())
        .add_systems(PostUpdate, admit_cameras);
}

pub(super) fn finish(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app.init_resource::<ShaderPackViews>().add_systems(
        Render,
        prepare_views
            .in_set(RenderSystems::PrepareResources)
            .after(bevy::render::view::prepare_view_targets),
    );
    install_graph(render_app.world_mut());
}

#[cfg(test)]
mod camera_tests {
    use super::*;
    use bevy::render::view::Msaa;

    #[test]
    fn opted_in_cameras_admit_single_sample_depth_sampling() {
        let mut app = App::new();
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
