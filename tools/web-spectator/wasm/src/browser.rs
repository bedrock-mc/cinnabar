//! Thin browser host for Cinnabar's native GPU plugins. The iframe owns the event loop.
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    ecs::system::SystemParam,
    platform::time::Instant,
    prelude::*,
    render::{
        RenderPlugin,
        settings::{Backends, RenderCreation, WgpuFeatures, WgpuSettings, WgpuSettingsPriority},
    },
    window::{PresentMode, PrimaryWindow, WindowPlugin},
    winit::{UpdateMode, WinitSettings},
};
// Bevy's derive manifest resolver does not inspect target-specific dependencies.
use bevy::ecs as bevy_ecs;
use meshing::{ChunkMesh, PackedBiomeRecord};
use render::{
    ActorPresentationGate, ActorRenderFrame, ActorRenderPlugin, ChunkBiomeTints,
    ChunkRenderApplySet, ChunkRenderPlugin, ChunkRenderQueue, ChunkTextureAssets,
    ChunkUploadBudget, ChunkUploadPriority, PresentedFrameGate, RenderViewCohort, UiRenderPlugin,
    UiRenderSceneResource, UiRenderStatsResource, VisibilityDiagnostics,
    VisibilityDiagnosticsInput,
};
use wasm_bindgen::prelude::*;
use world::SubChunkKey;

use crate::{
    TerrainAssets, browser_actor::BrowserActors, browser_camera::PovMotion,
    browser_diagnostics::Diagnostics, browser_hud::BrowserHud, browser_interpolation::FrameMotion,
    browser_model::Frame, model::Arena, terrain_runtime,
};

#[derive(Clone, Copy)]
enum CameraMode {
    Orbit,
    Follow,
    Pov,
}

// A wider spectator POV makes close-range combat readable in an embedded canvas.
// Orbit and follow retain the native camera default.
const POV_HORIZONTAL_FOV_RADIANS: f32 = 110_f32.to_radians();

struct CameraControl {
    mode: CameraMode,
    player_id: String,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Default for CameraControl {
    fn default() -> Self {
        Self {
            mode: CameraMode::Orbit,
            player_id: String::new(),
            yaw: 0.65,
            pitch: 0.55,
            distance: 28.0,
        }
    }
}

struct SkinUpload {
    id: String,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    slim: bool,
}
struct ArenaPublication {
    meshes: VecDeque<(SubChunkKey, ChunkMesh)>,
    center: Vec3,
}

struct ViewerState {
    arena: Option<ArenaPublication>,
    terrain_updates: VecDeque<(SubChunkKey, ChunkMesh)>,
    current: Option<Frame>,
    previous: Option<Frame>,
    received: Instant,
    previous_received: Instant,
    wall_millis: u64,
    camera: CameraControl,
    skins: VecDeque<SkinUpload>,
    geometry_uploads: VecDeque<(String, String, String)>,
    cape_uploads: VecDeque<SkinUpload>,
    animation_uploads: VecDeque<(String, protocol::SkinAnimation)>,
    listener_position: [f32; 3],
    listener_right: [f32; 3],
    ready: bool,
    error: Option<String>,
    submitted_chunks: usize,
    rendered_frames: u64,
    arena_ready: bool,
    diagnostics: Diagnostics,
}

struct BrowserRuntime {
    state: Arc<Mutex<ViewerState>>,
    actors: BrowserActors,
    hud: BrowserHud,
    effects: crate::browser_effects::BrowserEffects,
    terrain_scene: Arc<Mutex<Option<terrain_runtime::TerrainScene>>>,
    terrain_assets: TerrainAssets,
    items: crate::browser_items::BrowserItems,
    pending: VecDeque<(SubChunkKey, ChunkMesh)>,
    arena_center: Vec3,
    generation: u64,
    expectation_set: bool,
    camera_motion: PovMotion,
    replay_epoch: u64,
}

#[derive(Component)]
struct ViewerCamera;

#[derive(SystemParam)]
struct ViewerOutput<'w> {
    queue: ResMut<'w, ChunkRenderQueue>,
    actors: ResMut<'w, ActorRenderFrame>,
    ui: ResMut<'w, UiRenderSceneResource>,
    stats: Res<'w, UiRenderStatsResource>,
    presented: Res<'w, PresentedFrameGate>,
    hands: ResMut<'w, render::HandRigScene>,
    tints: Res<'w, ChunkBiomeTints>,
    visibility: Res<'w, VisibilityDiagnostics>,
    actor_presented: Res<'w, ActorPresentationGate>,
    nametags: ResMut<'w, render::NametagSceneResource>,
    particles: ResMut<'w, render::ParticleGpuFrame>,
    items: ResMut<'w, render::DroppedItemScene>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuRequirements {
    max_texture_array_layers: u32,
    max_storage_buffers_per_shader_stage: u32,
}

fn gpu_requirements_for(terrain: &TerrainAssets) -> GpuRequirements {
    GpuRequirements {
        max_texture_array_layers: terrain
            .runtime
            .texture_pages()
            .iter()
            .map(|page| page.texture.layers)
            .max()
            .unwrap_or(1),
        max_storage_buffers_per_shader_stage: render::required_vertex_storage_buffers(),
    }
}

/// A read-only native Cinnabar renderer running on a WebGPU canvas.
/// Destroy the owning iframe to dispose the Bevy/Winit loop and its GPU resources.
#[wasm_bindgen]
pub struct Viewer {
    state: Arc<Mutex<ViewerState>>,
    terrain: TerrainAssets,
    terrain_scene: Arc<Mutex<Option<terrain_runtime::TerrainScene>>>,
}

#[wasm_bindgen]
impl Viewer {
    /// Device limits required by the actual carrier and native GPU binding layout.
    pub fn gpu_requirements(terrain: &TerrainAssets) -> Result<String, String> {
        serde_json::to_string(&gpu_requirements_for(terrain)).map_err(|error| error.to_string())
    }

    #[wasm_bindgen(constructor)]
    // The JS boundary keeps each required compiled carrier as a typed byte slice.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        canvas_selector: &str,
        terrain: &TerrainAssets,
        entities: &[u8],
        actor_artwork: &[u8],
        particles: &[u8],
        equipment: &[u8],
        hud: &[u8],
        icons: &[u8],
        json_ui: &[u8],
        font: &[u8],
    ) -> Result<Viewer, String> {
        if canvas_selector.is_empty() || canvas_selector.len() > 128 {
            return Err("invalid spectator canvas selector".into());
        }
        let global = js_sys::global();
        let navigator = js_sys::Reflect::get(&global, &JsValue::from_str("navigator"))
            .map_err(|_| "browser navigator is unavailable")?;
        let gpu = js_sys::Reflect::get(&navigator, &JsValue::from_str("gpu"))
            .map_err(|_| "browser WebGPU is unavailable")?;
        if gpu.is_null() || gpu.is_undefined() {
            return Err("This Cinnabar spectator requires a browser with WebGPU enabled.".into());
        }
        let actors = BrowserActors::new(entities, actor_artwork, equipment, icons)?;
        let hud = BrowserHud::new(json_ui, hud, icons, font)?;
        let terrain_scene = Arc::new(Mutex::new(None));
        let effects = crate::browser_effects::BrowserEffects::new(particles, icons)?;
        let items = crate::browser_items::BrowserItems::new(icons, terrain)?;
        let now = Instant::now();
        let state = Arc::new(Mutex::new(ViewerState {
            arena: None,
            terrain_updates: VecDeque::new(),
            current: None,
            previous: None,
            received: now,
            previous_received: now,
            wall_millis: 0,
            camera: CameraControl::default(),
            skins: VecDeque::new(),
            geometry_uploads: VecDeque::new(),
            cape_uploads: VecDeque::new(),
            animation_uploads: VecDeque::new(),
            listener_position: [0.0; 3],
            listener_right: [1.0, 0.0, 0.0],
            ready: false,
            error: None,
            submitted_chunks: 0,
            rendered_frames: 0,
            arena_ready: false,
            diagnostics: Diagnostics::default(),
        }));
        let requirements = gpu_requirements_for(terrain);
        let mut settings = WgpuSettings {
            backends: Some(Backends::BROWSER_WEBGPU),
            priority: WgpuSettingsPriority::Compatibility,
            features: WgpuFeatures::empty(),
            ..default()
        };
        settings.limits.max_texture_array_layers = settings
            .limits
            .max_texture_array_layers
            .max(requirements.max_texture_array_layers);
        settings.limits.max_storage_buffers_per_shader_stage = settings
            .limits
            .max_storage_buffers_per_shader_stage
            .max(requirements.max_storage_buffers_per_shader_stage);
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Live spectator".into(),
                        canvas: Some(canvas_selector.into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: false,
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(settings),
                    ..default()
                }),
        );
        let resolved = terrain
            .runtime
            .biome_assets()
            .resolve_live(&[])
            .map_err(|error| error.to_string())?;
        app.insert_resource(ChunkTextureAssets::new(Arc::clone(&terrain.runtime)))
            .insert_resource(ChunkBiomeTints::from_resolved(&resolved, 1))
            .insert_resource(VisibilityDiagnosticsInput::new(true))
            .insert_resource(ClearColor(Color::srgb(0.48, 0.70, 0.94)))
            .insert_resource(WinitSettings {
                // Browser redraws follow requestAnimationFrame and AutoVsync. An
                // iframe need not have keyboard focus to present smooth movement.
                focused_mode: UpdateMode::Continuous,
                unfocused_mode: UpdateMode::Continuous,
            })
            .insert_non_send_resource(BrowserRuntime {
                state: Arc::clone(&state),
                actors,
                hud,
                effects,
                terrain_scene: Arc::clone(&terrain_scene),
                terrain_assets: terrain.clone(),
                items,
                pending: VecDeque::new(),
                arena_center: Vec3::ZERO,
                generation: 0,
                expectation_set: false,
                camera_motion: PovMotion::default(),
                replay_epoch: 0,
            })
            .add_plugins((
                ChunkRenderPlugin::with_budget(ChunkUploadBudget::new(8, 8 * 1024 * 1024)),
                ActorRenderPlugin,
                UiRenderPlugin,
                render::HandRigRenderPlugin,
                render::ParticleRenderPlugin,
                render::DroppedItemRenderPlugin,
            ))
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, update_viewer.before(ChunkRenderApplySet));
        app.run();
        Ok(Self {
            state,
            terrain: terrain.clone(),
            terrain_scene,
        })
    }

    /// Validates the canonical block states and meshes with native Cinnabar assets.
    pub fn set_arena(&self, input: &str) -> Result<(), String> {
        let arena = Arena::parse(input)?;
        let terrain_scene = terrain_runtime::TerrainScene::new(&arena, &self.terrain)?;
        let meshes = terrain_scene.initial(&self.terrain)?;
        *self
            .terrain_scene
            .lock()
            .map_err(|_| "arena state is unavailable")? = Some(terrain_scene);
        let center = Vec3::new(
            (arena.bounds[0] + arena.bounds[3]) as f32 * 0.5,
            (arena.bounds[1] + arena.bounds[4]) as f32 * 0.5,
            (arena.bounds[2] + arena.bounds[5]) as f32 * 0.5,
        );
        let mut state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        if state.error.is_some() {
            return Err("spectator renderer has stopped".into());
        }
        state.terrain_updates.clear();
        state.arena = Some(ArenaPublication { meshes, center });
        Ok(())
    }

    pub fn set_frame(&self, input: &str, received_timestamp_ms: f64) -> Result<(), String> {
        if !received_timestamp_ms.is_finite() || received_timestamp_ms < 0.0 {
            return Err("spectator frame timestamp is invalid".into());
        }
        let frame = Frame::parse(input)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        if state.error.is_some() {
            return Err("spectator renderer has stopped".into());
        }
        if state
            .current
            .as_ref()
            .is_some_and(|current| current.id != frame.id)
        {
            return Err("spectator frame belongs to a different duel".into());
        }
        if state
            .current
            .as_ref()
            .is_none_or(|current| current.blocks != frame.blocks)
            && let Some(terrain) = self
                .terrain_scene
                .lock()
                .map_err(|_| "arena state is unavailable")?
                .as_mut()
        {
            for (key, mesh) in terrain.apply(&frame.blocks, &self.terrain)? {
                state.terrain_updates.retain(|(pending, _)| *pending != key);
                state.terrain_updates.push_back((key, mesh));
            }
        }
        state.previous = state
            .current
            .take()
            .filter(|current| current.replay_epoch == frame.replay_epoch);
        state.previous_received = state.received;
        state.current = Some(frame);
        state.received = Instant::now();
        state.wall_millis = received_timestamp_ms as u64;
        Ok(())
    }

    pub fn set_camera(&self, mode: &str, player_id: &str) -> Result<(), String> {
        let mode = match mode {
            "orbit" => CameraMode::Orbit,
            "follow" => CameraMode::Follow,
            "pov" => CameraMode::Pov,
            _ => return Err("unknown spectator camera mode".into()),
        };
        if player_id.len() > 128 {
            return Err("invalid spectator camera player".into());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        state.camera.mode = mode;
        state.camera.player_id = player_id.into();
        Ok(())
    }

    /// Restores the default orbit framing around the current fighters or arena.
    pub fn reset_camera(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        state.camera = CameraControl::default();
        Ok(())
    }

    pub fn orbit(&self, delta_yaw: f32, delta_pitch: f32, delta_zoom: f32) -> Result<(), String> {
        if ![delta_yaw, delta_pitch, delta_zoom]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err("invalid spectator orbit input".into());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        state.camera.yaw = (state.camera.yaw + delta_yaw).rem_euclid(std::f32::consts::TAU);
        state.camera.pitch = (state.camera.pitch + delta_pitch).clamp(0.03, 1.45);
        state.camera.distance =
            (state.camera.distance * delta_zoom.clamp(-2.0, 2.0).exp()).clamp(2.0, 512.0);
        Ok(())
    }

    pub fn listener_pose(&self) -> Result<String, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        Ok(
            serde_json::json!({"position":state.listener_position,"right":state.listener_right})
                .to_string(),
        )
    }

    // This flat signature is the browser/WASM upload ABI.
    #[allow(clippy::too_many_arguments)]
    pub fn set_skin_animation(
        &self,
        player_id: &str,
        kind: u32,
        width: u32,
        height: u32,
        frames: u32,
        blinking: bool,
        pixels: Vec<u8>,
    ) -> Result<(), String> {
        if player_id.is_empty()
            || player_id.len() > 128
            || width == 0
            || height == 0
            || width > 512
            || height > 512
            || frames == 0
            || frames > height
            || frames > 256
            || pixels.len() != width as usize * height as usize * 4
        {
            return Err("invalid persona animation".into());
        }
        let kind = match kind {
            0 => protocol::SkinAnimationKind::Face,
            1 => protocol::SkinAnimationKind::Body32,
            2 => protocol::SkinAnimationKind::Body128,
            _ => return Err("invalid persona animation kind".into()),
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        state
            .animation_uploads
            .retain(|(id, image)| id != player_id || image.kind != kind);
        if state.animation_uploads.len() >= 24 {
            return Err("persona upload queue full".into());
        }
        state.animation_uploads.push_back((
            player_id.into(),
            protocol::SkinAnimation {
                kind,
                width,
                height,
                frames,
                blinking,
                rgba8: pixels.into(),
            },
        ));
        Ok(())
    }

    pub fn set_cape(
        &self,
        player_id: &str,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<(), String> {
        if player_id.is_empty()
            || player_id.len() > 128
            || width > 256
            || height > 256
            || pixels.len() != width as usize * height as usize * 4
        {
            return Err("invalid cape upload".into());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        state.cape_uploads.retain(|skin| skin.id != player_id);
        if state.cape_uploads.len() >= 32 {
            return Err("cape upload queue full".into());
        }
        state.cape_uploads.push_back(SkinUpload {
            id: player_id.into(),
            width,
            height,
            pixels,
            slim: false,
        });
        Ok(())
    }

    pub fn set_skin_geometry(
        &self,
        player_id: &str,
        resource_patch: &str,
        geometry: &str,
    ) -> Result<(), String> {
        if player_id.is_empty()
            || player_id.len() > 128
            || resource_patch.len() > 16384
            || geometry.len() > 262144
        {
            return Err("invalid skin geometry bounds".into());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        state.geometry_uploads.retain(|(id, _, _)| id != player_id);
        state.animation_uploads.retain(|(id, _)| id != player_id);
        if state.geometry_uploads.len() >= 32 {
            return Err("skin geometry queue is full".into());
        }
        state.geometry_uploads.push_back((
            player_id.into(),
            resource_patch.into(),
            geometry.into(),
        ));
        Ok(())
    }

    pub fn set_skin(
        &self,
        player_id: &str,
        width: u32,
        height: u32,
        rgba_pixels: Vec<u8>,
        slim: bool,
    ) -> Result<(), String> {
        if player_id.len() > 128
            || width > 512
            || height > 512
            || width == 0
            || height == 0
            || rgba_pixels.len() != width as usize * height as usize * 4
        {
            return Err("invalid spectator skin raster".into());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        state.skins.retain(|skin| skin.id != player_id);
        if state.skins.len() >= 32 {
            return Err("spectator skin queue is full".into());
        }
        state.skins.push_back(SkinUpload {
            id: player_id.into(),
            width,
            height,
            pixels: rgba_pixels,
            slim,
        });
        Ok(())
    }

    pub fn status(&self) -> Result<String, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "spectator state is unavailable")?;
        serde_json::to_string(&serde_json::json!({"ready":state.ready,"error":state.error,
            "submittedChunks":state.submitted_chunks,"renderedFrames":state.rendered_frames,
            "arenaReady":state.arena_ready,"renderer":"cinnabar-webgpu",
            "diagnostics":state.diagnostics}))
        .map_err(|error| error.to_string())
    }
}

fn spawn_camera(mut commands: Commands, window: Single<&Window, With<PrimaryWindow>>) {
    commands.spawn((
        Camera3d::default(),
        ViewerCamera,
        Msaa::Off,
        Tonemapping::None,
        Projection::Perspective(PerspectiveProjection {
            far: 2048.0,
            fov: view_presentation::camera::projection_fov_radians(ui::DEFAULT_FOV_DEGREES as f32),
            aspect_ratio: view_presentation::camera::projection_aspect(
                window.width(),
                window.height(),
            ),
            near: render_api::CAMERA_NEAR_PLANE_BLOCKS,
            near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -render_api::CAMERA_NEAR_PLANE_BLOCKS),
            ..default()
        }),
        Transform::from_xyz(20.0, 18.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn update_viewer(
    mut runtime: NonSendMut<BrowserRuntime>,
    output: ViewerOutput,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<&mut Transform, With<ViewerCamera>>,
    mut visibility: Single<&mut Camera, With<ViewerCamera>>,
    mut projection: Single<&mut Projection, With<ViewerCamera>>,
) {
    let ViewerOutput {
        mut queue,
        mut actors,
        mut ui,
        stats,
        presented,
        mut hands,
        tints,
        visibility: render_visibility,
        actor_presented,
        mut nametags,
        mut particles,
        mut items,
    } = output;
    let state_arc = Arc::clone(&runtime.state);
    let Ok(mut state) = state_arc.lock() else {
        return;
    };
    state.diagnostics.main_updates = state.diagnostics.main_updates.saturating_add(1);
    state.diagnostics.pending_chunks = queue.pending_len();
    state
        .diagnostics
        .observe_visibility(render_visibility.snapshot());
    state.diagnostics.observe_hud(stats.snapshot());
    for acknowledgement in actor_presented.drain() {
        state.diagnostics.observe_actor(&acknowledgement);
    }
    if state.error.is_some() {
        runtime.camera_motion.reset();
        *actors = ActorRenderFrame::default();
        **nametags = render_model::NametagScene::default();
        state.diagnostics.nametag_records = 0;
        hands.clear();
        items.clear();
        *particles = render::ParticleGpuFrame::default();
        ui.input = None;
        visibility.is_active = false;
        state.ready = false;
        return;
    }
    if state.current.is_none() || state.received.elapsed() > std::time::Duration::from_secs(5) {
        runtime.camera_motion.reset();
        *actors = ActorRenderFrame::default();
        **nametags = render_model::NametagScene::default();
        state.diagnostics.nametag_records = 0;
        hands.clear();
        items.clear();
        *particles = render::ParticleGpuFrame::default();
        ui.input = None;
        visibility.is_active = false;
        state.ready = false;
        return;
    }
    visibility.is_active = true;
    if let Some(arena) = state.arena.take() {
        queue.reset_session();
        runtime.pending = arena.meshes;
        runtime.arena_center = arena.center;
        presented.clear();
        runtime.generation = runtime.generation.wrapping_add(1);
        runtime.expectation_set = false;
        state.submitted_chunks = 0;
        state.ready = false;
        state.arena_ready = false;
        state.rendered_frames = 0;
        state.diagnostics = Diagnostics::default();
    }
    while let Some((key, mesh)) = state.terrain_updates.pop_front() {
        runtime.pending.retain(|(pending, _)| *pending != key);
        runtime.pending.push_back((key, mesh));
    }
    for _ in 0..8 {
        let Some((key, mesh)) = runtime.pending.pop_front() else {
            break;
        };
        if mesh.is_empty() {
            if queue.try_remove(key).is_err() {
                runtime.pending.push_front((key, mesh));
                break;
            }
            continue;
        }
        if let Err((mesh, _)) = queue.try_insert_with_biome_identity(
            key,
            mesh,
            PackedBiomeRecord::fallback(),
            tints.table_identity(),
            ChunkUploadPriority::from_camera(key, camera.translation),
        ) {
            runtime.pending.push_front((key, mesh));
            break;
        }
        state.submitted_chunks += 1;
    }
    if runtime.pending.is_empty() && state.submitted_chunks > 0 && !runtime.expectation_set {
        let center = [
            (runtime.arena_center.x / 16.0).floor() as i32,
            (runtime.arena_center.z / 16.0).floor() as i32,
        ];
        presented.set_expectation(queue.freeze_target_expectation(
            RenderViewCohort::new(0, center, 64),
            None,
            runtime.generation,
            Instant::now(),
        ));
        runtime.expectation_set = true;
    }
    for acknowledgement in presented.drain() {
        state.diagnostics.observe_chunk(&acknowledgement);
        if acknowledgement.is_exact() && !acknowledgement.drawn_manifest.is_empty() {
            state.rendered_frames = state.rendered_frames.saturating_add(1);
            state.arena_ready = true;
            state.ready = true;
        }
    }
    state.diagnostics.pending_chunks = queue.pending_len();
    state.diagnostics.expected_chunks = presented
        .expectation()
        .map_or(0, |expectation| expectation.manifest.len());
    while let Some(skin) = state.skins.pop_front() {
        if let Err(error) =
            runtime
                .actors
                .set_skin(&skin.id, skin.width, skin.height, skin.pixels, skin.slim)
        {
            state.error = Some(error);
            return;
        }
    }
    while let Some(skin) = state.cape_uploads.pop_front() {
        if let Err(error) = runtime
            .actors
            .set_cape(&skin.id, skin.width, skin.height, skin.pixels)
        {
            state.error = Some(error);
            return;
        }
    }
    while let Some((id, patch, data)) = state.geometry_uploads.pop_front() {
        if let Err(error) = runtime.actors.set_geometry(&id, &patch, &data) {
            state.error = Some(error);
            return;
        }
    }
    while let Some((id, image)) = state.animation_uploads.pop_front() {
        if let Err(error) = runtime.actors.set_animation(&id, image) {
            state.error = Some(error);
            return;
        }
    }
    if let Some(frame) = state.current.as_ref()
        && frame.replay_epoch != runtime.replay_epoch
    {
        runtime.camera_motion.reset();
        runtime.replay_epoch = frame.replay_epoch;
    }
    let current = state.current.as_ref();
    let fighter = current.and_then(|frame| {
        frame
            .fighters
            .iter()
            .find(|fighter| fighter.id == state.camera.player_id)
    });
    let interval = state
        .received
        .duration_since(state.previous_received)
        .as_secs_f32()
        .clamp(0.05, 1.0);
    let partial = (state.received.elapsed().as_secs_f32() / interval).clamp(0.0, 1.0);
    let motion = current.map(|frame| FrameMotion::new(frame, state.previous.as_ref(), partial));
    let position = fighter
        .zip(motion.as_ref())
        .map(|(fighter, motion)| Vec3::from_array(motion.position(fighter)));
    let mut target = motion
        .as_ref()
        .and_then(FrameMotion::center)
        .map_or(runtime.arena_center, |center| {
            Vec3::from_array(center) + Vec3::Y
        });
    let mut hand_motion = Mat4::IDENTITY;
    match state.camera.mode {
        CameraMode::Pov if fighter.is_some() => {
            let Some(fighter) = fighter else {
                return;
            };
            let Some(pov) = fighter.pov.as_ref() else {
                state.error = Some("player POV data is unavailable".into());
                return;
            };
            let [yaw, pitch] = motion
                .as_ref()
                .map_or([fighter.yaw, fighter.pitch], |motion| {
                    motion.angles(fighter)
                });
            let base = Transform {
                translation: position.unwrap_or(target) + Vec3::Y * pov.eye_height,
                rotation: view_presentation::camera::bedrock_camera_rotation(yaw, pitch),
                ..Transform::IDENTITY
            };
            let speed = current.map_or(1.0, Frame::visual_speed);
            let (posed, motion) = runtime.camera_motion.update(fighter, base, speed);
            **camera = posed;
            hand_motion = motion;
        }
        mode => {
            runtime.camera_motion.reset();
            if matches!(mode, CameraMode::Follow) {
                target = position.unwrap_or(target) + Vec3::Y;
            }
            let (yaw, pitch, distance) =
                (state.camera.yaw, state.camera.pitch, state.camera.distance);
            let offset = Vec3::new(
                yaw.sin() * pitch.cos(),
                pitch.sin(),
                yaw.cos() * pitch.cos(),
            ) * distance;
            **camera = Transform::from_translation(target + offset).looking_at(target, Vec3::Y);
        }
    }
    let hidden = if matches!(state.camera.mode, CameraMode::Pov) {
        fighter.map(|fighter| fighter.id.as_str())
    } else {
        None
    };
    if let Some(current) = current {
        *actors = runtime.actors.update(
            current,
            motion.as_ref().and_then(FrameMotion::previous),
            partial,
            hidden,
            {
                let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
                [
                    -pitch.to_degrees(),
                    (180.0 - yaw.to_degrees()).rem_euclid(360.0),
                ]
            },
            camera.translation.to_array(),
        );
        let anchors = current
            .fighters
            .iter()
            .filter(|fighter| !fighter.dead && hidden != Some(fighter.id.as_str()))
            .filter_map(|fighter| {
                let feet = Vec3::from_array(
                    motion
                        .as_ref()
                        .map_or(fighter.position, |motion| motion.position(fighter)),
                );
                view_presentation::nametags::player_nametag_anchor(
                    &fighter.name,
                    feet,
                    camera.translation,
                    fighter.sneaking,
                )
            })
            .collect::<Vec<_>>();
        **nametags = runtime.hud.nametag_scene(&anchors);
    }
    let hud_fighter = if matches!(state.camera.mode, CameraMode::Pov) {
        fighter
    } else {
        None
    };
    let viewport = [
        window.physical_width().max(1),
        window.physical_height().max(1),
    ];
    let Projection::Perspective(perspective) = &mut **projection else {
        state.error = Some("spectator perspective projection is unavailable".into());
        return;
    };
    perspective.aspect_ratio =
        view_presentation::camera::projection_aspect(window.width(), window.height());
    perspective.fov = if matches!(state.camera.mode, CameraMode::Pov) {
        let vertical =
            2.0 * ((POV_HORIZONTAL_FOV_RADIANS * 0.5).tan() / perspective.aspect_ratio).atan();
        view_presentation::camera::projection_fov_radians(vertical.to_degrees())
    } else {
        view_presentation::camera::projection_fov_radians(ui::DEFAULT_FOV_DEGREES as f32)
    };
    *hands = runtime
        .actors
        .update_hands(hud_fighter, partial, perspective.fov, hand_motion);
    if let Some(current) = current {
        runtime
            .items
            .update(current, state.previous.as_ref(), partial, &mut items);
        let view = render::particle_view(&GlobalTransform::from(**camera), &projection);
        let terrain_arc = Arc::clone(&runtime.terrain_scene);
        let terrain_assets = runtime.terrain_assets.clone();
        let terrain = terrain_arc.lock().ok();
        runtime.effects.update(
            current,
            &mut particles,
            &view,
            state
                .wall_millis
                .saturating_add(state.received.elapsed().as_millis() as u64),
            terrain.as_ref().and_then(|guard| guard.as_ref()),
            &terrain_assets,
        );
    }
    let now_millis = state
        .wall_millis
        .saturating_add(state.received.elapsed().as_millis() as u64);
    match runtime
        .hud
        .update(hud_fighter, viewport, now_millis, state.wall_millis)
    {
        Ok(input) => {
            if let Err(error) = ui.publish(input, &stats) {
                state.error = Some(error.to_string());
            }
        }
        Err(error) => state.error = Some(error),
    }
    state.listener_position = camera.translation.to_array();
    state.listener_right = (camera.rotation * Vec3::X).to_array();
    state.diagnostics.nametag_records = nametags.records.len();
}
