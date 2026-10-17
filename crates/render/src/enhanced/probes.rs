//! Amortized geometry captures for reflections beyond the screen boundary.
use super::{EnhancedRendering, PROBE_SHADER, frame::EnhancedFrameGpu};
#[path = "probe_draw.rs"]
pub(crate) mod probe_draw;
use probe_draw::{ProbePass, draw};
#[path = "probe_cache.rs"]
mod probe_cache;
#[path = "probe_filter.rs"]
mod probe_filter;
#[path = "probe_history.rs"]
mod probe_history;
use super::actor_signature::frame_signature as actor_signature;
use bevy::{
    camera::{Camera3dDepthTextureUsage, Hdr, RenderTarget},
    core_pipeline::FullscreenShader,
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
    },
};
use probe_cache::{ProbeLighting, ProbeSchedule};
use render_scene::enhanced::RenderedWorld;
use std::{
    collections::HashMap,
    hash::Hash,
    num::NonZeroU64,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const SIZE: u32 = 128;
const MIPS: u32 = 8;
const CUBE_FACE_COUNT: u32 = 6;
/// Keep the capture origin stable through ordinary camera motion; relocation
/// still occurs well inside the 32-block local reflection volume.
const RELOCATION_DISTANCE: f32 = 16.0;

#[derive(Component, ExtractComponent, Clone, Copy)]
#[extract_app(bevy::render::RenderApp)]
pub(crate) struct ProbeFace(pub u32);
#[derive(Resource, ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub(crate) struct ProbeOrigin {
    pub position: Vec3,
    pub epoch: u64,
    pub quality: super::EnhancedQuality,
    pub(crate) source: Option<Entity>,
    world: Option<RenderedWorld>,
    completed_faces: Arc<AtomicU64>,
    lighting_signature: Arc<AtomicU64>,
}
impl ProbeOrigin {
    fn reset_faces(&self) {
        self.completed_faces
            .store(self.epoch << 6, Ordering::Relaxed);
    }

    fn face_mask(&self) -> u8 {
        let completed = self.completed_faces.load(Ordering::Relaxed);
        if completed >> 6 == self.epoch {
            (completed & 63) as u8
        } else {
            0
        }
    }

    fn complete_face(&self, face: u32) {
        let _ = self
            .completed_faces
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                (old >> 6 == self.epoch).then_some(old | (1 << face))
            });
    }

    pub(crate) fn publish_lighting_signature(&self, signature: u64) {
        self.lighting_signature.store(signature, Ordering::Relaxed);
    }
}
#[derive(Resource, Default)]
struct Cameras {
    entities: Vec<Entity>,
    images: Vec<Handle<Image>>,
    history: super::world_cache::WorldCache<CaptureHistory>,
}

#[derive(Default)]
struct CaptureHistory {
    schedule: ProbeSchedule,
    lighting: Option<ProbeLighting>,
    material: Option<render_scene::ChunkTextureAssetIdentity>,
    actor_signature: u64,
    settings: Option<EnhancedRendering>,
    lighting_signature: u64,
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Cameras>()
        .init_resource::<ProbeOrigin>()
        .add_plugins((
            ExtractComponentPlugin::<ProbeFace>::default(),
            ExtractResourcePlugin::<ProbeOrigin>::default(),
        ))
        .add_systems(
            PostUpdate,
            sync_cameras.after(bevy::transform::TransformSystems::Propagate),
        );
}

fn face_basis(face: u32) -> (Vec3, Vec3) {
    match face {
        0 => (Vec3::X, Vec3::NEG_Y),
        1 => (Vec3::NEG_X, Vec3::NEG_Y),
        2 => (Vec3::Y, Vec3::Z),
        3 => (Vec3::NEG_Y, Vec3::NEG_Z),
        4 => (Vec3::Z, Vec3::NEG_Y),
        _ => (Vec3::NEG_Z, Vec3::NEG_Y),
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_cameras(
    mut commands: Commands,
    mut cameras: ResMut<Cameras>,
    mut origin: ResMut<ProbeOrigin>,
    mut images: ResMut<Assets<Image>>,
    time: Option<Res<Time>>,
    atmosphere: Option<Res<render_scene::AtmosphereFrame>>,
    environment: Option<Res<render_scene::AtmosphereViewInputs>>,
    clouds: Option<Res<render_scene::CloudVisibility>>,
    materials: Option<Res<render_scene::ChunkTextureAssets>>,
    actors: Option<Res<render_scene::ActorRenderFrame>>,
    changed_chunks: Query<(), Changed<render_scene::ChunkRenderInstance>>,
    mut removed_chunks: RemovedComponents<render_scene::ChunkRenderInstance>,
    source: Query<(Entity, &Camera, &GlobalTransform, &EnhancedRendering), Without<ProbeFace>>,
    mut captures: Query<
        (
            &mut Camera,
            &mut Transform,
            &mut GlobalTransform,
            &mut EnhancedRendering,
            &ProbeFace,
        ),
        With<ProbeFace>,
    >,
) {
    let Some((source_entity, source_camera, pose, settings)) =
        source.iter().find(|(_, camera, _, s)| {
            camera.is_active && !s.reflection_capture && (s.water_reflections || s.physically_based)
        })
    else {
        for entity in cameras.entities.drain(..) {
            commands.entity(entity).despawn();
        }
        for image in cameras.images.drain(..) {
            images.remove(image.id());
        }
        cameras.history = default();
        if origin.source.take().is_some() {
            origin.epoch = origin.epoch.wrapping_add(1);
            origin.reset_faces();
        }
        return;
    };
    let applied = super::frame::applied_cloud_settings(*settings, clouds.as_deref());
    let settings = &applied;
    let world = RenderedWorld::of(environment.as_deref());
    cameras.history.enter_world(world);
    let source_changed = origin.source != Some(source_entity) || origin.world != world;
    origin.source = Some(source_entity);
    origin.world = world;
    origin.quality = settings.quality;
    let budget = render_scene::enhanced::enhanced_quality_budget(settings.quality);
    cameras
        .history
        .schedule
        .set_intervals(budget.reflection_refresh, budget.reflection_animation);
    let capture_order = source_camera.order.saturating_add(1);
    let now = time.as_ref().map_or(0.0, |time| time.elapsed_secs_f64());
    if changed_chunks.iter().next().is_some() || removed_chunks.read().count() != 0 {
        cameras.history.schedule.invalidate();
    }
    let material = materials.as_ref().map(|materials| materials.identity());
    let enhanced = capture_settings(*settings);
    let inputs_changed =
        cameras.history.material != material || cameras.history.settings != Some(enhanced);
    if inputs_changed {
        cameras.history.material = material;
        cameras.history.settings = Some(enhanced);
        cameras.history.schedule.invalidate();
    }
    if let Some(atmosphere) = atmosphere {
        let next = ProbeLighting {
            sun: Vec3::from_array(atmosphere.sun_direction()),
            phase: atmosphere.moon_phase(),
            rain: atmosphere.rain_level(),
            thunder: atmosphere.thunder_level(),
            zenith: Vec3::from_array(atmosphere.sky_zenith()),
            horizon: Vec3::from_array(atmosphere.sky_horizon()),
            fog: (!atmosphere.physical_atmosphere_allowed()).then_some((
                Vec3::from_array(atmosphere.fog_color()).extend(atmosphere.fog_start()),
                atmosphere.fog_end(),
            )),
        };
        if cameras
            .history
            .lighting
            .is_none_or(|old| !old.reusable(next))
        {
            if cameras
                .history
                .lighting
                .is_some_and(|old| old.discontinuity(next))
            {
                origin.epoch = origin.epoch.wrapping_add(1);
                origin.reset_faces();
                cameras
                    .history
                    .schedule
                    .relocate(preferred_face(pose.forward().as_vec3()));
            }
            cameras.history.lighting = Some(next);
            cameras.history.schedule.invalidate();
        }
    }
    if now >= cameras.history.schedule.next_inputs {
        let signature = actors.as_ref().map_or(0, |frame| actor_signature(frame));
        if signature != cameras.history.actor_signature {
            cameras.history.actor_signature = signature;
            cameras.history.schedule.invalidate();
        }
        let lighting_signature = origin.lighting_signature.load(Ordering::Relaxed);
        if cameras.history.lighting_signature != lighting_signature {
            cameras.history.lighting_signature = lighting_signature;
            cameras.history.schedule.invalidate();
        }
        cameras.history.schedule.next_inputs = now + budget.reflection_refresh;
    }
    cameras
        .history
        .schedule
        .animate(now, settings.volumetric_clouds || settings.waving);
    let position = pose.translation();
    let relocated =
        cameras.entities.is_empty() || source_changed || should_relocate(origin.position, position);
    if relocated {
        origin.position = position;
        origin.epoch = origin.epoch.wrapping_add(1);
        origin.reset_faces();
        cameras
            .history
            .schedule
            .relocate(preferred_face(pose.forward().as_vec3()));
    } else if inputs_changed {
        origin.epoch = origin.epoch.wrapping_add(1);
        origin.reset_faces();
        cameras
            .history
            .schedule
            .relocate(preferred_face(pose.forward().as_vec3()));
    }
    cameras.history.schedule.completed(origin.face_mask());
    let active_face = cameras.history.schedule.next_face(now);
    if cameras.entities.is_empty() {
        for face in 0..6 {
            let mut image = Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba16Float, None);
            image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            let image = images.add(image);
            let (direction, up) = face_basis(face);
            let entity = commands
                .spawn((
                    Camera3d {
                        depth_texture_usages: Camera3dDepthTextureUsage::from(
                            TextureUsages::RENDER_ATTACHMENT
                                | TextureUsages::TEXTURE_BINDING
                                | TextureUsages::COPY_SRC,
                        ),
                        ..default()
                    },
                    Camera {
                        order: capture_order,
                        is_active: active_face == Some(face),
                        ..default()
                    },
                    RenderTarget::Image(image.clone().into()),
                    Projection::Perspective(PerspectiveProjection {
                        fov: std::f32::consts::FRAC_PI_2,
                        aspect_ratio: 1.0,
                        near: 0.05,
                        far: 128.0,
                        ..default()
                    }),
                    Transform::from_translation(origin.position).looking_to(direction, up),
                    GlobalTransform::from(
                        Transform::from_translation(origin.position).looking_to(direction, up),
                    ),
                    Hdr,
                    bevy::core_pipeline::tonemapping::Tonemapping::None,
                    enhanced,
                    ProbeFace(face),
                ))
                .id();
            cameras.entities.push(entity);
            cameras.images.push(image);
        }
    }
    for (mut camera, mut transform, mut global, mut capture_settings, face) in &mut captures {
        if camera.order != capture_order {
            camera.order = capture_order;
        }
        let active = active_face == Some(face.0);
        if camera.is_active != active {
            camera.is_active = active;
        }
        if *capture_settings != enhanced {
            *capture_settings = enhanced;
        }
        if relocated {
            let (direction, up) = face_basis(face.0);
            *transform = Transform::from_translation(origin.position).looking_to(direction, up);
            *global = GlobalTransform::from(*transform);
        }
    }
}

fn preferred_face(direction: Vec3) -> u32 {
    let absolute = direction.abs();
    if absolute.x >= absolute.y && absolute.x >= absolute.z {
        u32::from(direction.x < 0.0)
    } else if absolute.y >= absolute.z {
        2 + u32::from(direction.y < 0.0)
    } else {
        4 + u32::from(direction.z < 0.0)
    }
}

fn should_relocate(origin: Vec3, camera: Vec3) -> bool {
    origin.distance_squared(camera) > RELOCATION_DISTANCE * RELOCATION_DISTANCE
}

fn capture_settings(mut settings: EnhancedRendering) -> EnhancedRendering {
    settings.reflection_capture = true;
    settings.temporal_aa = false;
    settings.bloom = false;
    settings.light_shafts = false;
    settings.ssao = false;
    settings.water_reflections = false;
    settings.shadow_debug = super::EnhancedShadowDebug::Off;
    settings
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct DrawBindingKey {
    frame: BufferId,
    source: TextureViewId,
    depth: TextureViewId,
    sky: TextureViewId,
}

#[derive(Resource)]
pub(crate) struct ProbeGpu {
    pub array: TextureView,
    _texture: Texture,
    pub faces: Vec<Vec<TextureView>>,
    target_texture: Texture,
    diffuse_target_texture: Texture,
    target_faces: Vec<Vec<TextureView>>,
    target_diffuse_faces: Vec<TextureView>,
    history: Mutex<probe_history::ReflectionHistory>,
    raw_cube: TextureView,
    diffuse_faces: Vec<TextureView>,
    sky_view: TextureView,
    sky_specular: Vec<TextureView>,
    sky_diffuse: TextureView,
    epoch: AtomicU64,
    pending_refilter: AtomicU64,
    draw_groups: Mutex<HashMap<DrawBindingKey, BindGroup>>,
}
impl ProbeGpu {
    pub(crate) fn is_current(&self, origin: &ProbeOrigin) -> bool {
        origin.source.is_some()
            && self.epoch.load(Ordering::Relaxed) == origin.epoch
            && origin.face_mask() == (1 << CUBE_FACE_COUNT) - 1
            && self.pending_refilter.load(Ordering::Relaxed) == 0
    }
}
impl FromWorld for ProbeGpu {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("enhanced reflection probe"),
            size: Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: probe_filter::ARRAY_LAYERS,
            },
            mip_level_count: MIPS,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let target_texture = device.create_texture(&TextureDescriptor {
            label: Some("enhanced replacement reflection targets"),
            size: Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: CUBE_FACE_COUNT,
            },
            mip_level_count: MIPS,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let diffuse_target_texture = device.create_texture(&TextureDescriptor {
            label: Some("enhanced replacement diffuse targets"),
            size: Extent3d {
                width: SIZE >> probe_filter::DIFFUSE_MIP,
                height: SIZE >> probe_filter::DIFFUSE_MIP,
                depth_or_array_layers: CUBE_FACE_COUNT,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let array = texture.create_view(&TextureViewDescriptor {
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        });
        let layer_view = |layer: u32, mip: u32| {
            texture.create_view(&TextureViewDescriptor {
                dimension: Some(TextureViewDimension::D2),
                base_mip_level: mip,
                mip_level_count: Some(1),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..default()
            })
        };
        let raw_cube = target_texture.create_view(&TextureViewDescriptor {
            dimension: Some(TextureViewDimension::D2Array),
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(CUBE_FACE_COUNT),
            ..default()
        });
        let diffuse_faces = (0..CUBE_FACE_COUNT)
            .map(|face| layer_view(CUBE_FACE_COUNT + face, probe_filter::DIFFUSE_MIP))
            .collect();
        let sky_view = layer_view(probe_filter::RAW_SKY_LAYER, 0);
        let sky_specular = (0..MIPS)
            .map(|mip| layer_view(probe_filter::SKY_SPECULAR_LAYER, mip))
            .collect();
        let sky_diffuse = layer_view(probe_filter::SKY_DIFFUSE_LAYER, probe_filter::DIFFUSE_MIP);
        let faces = (0..CUBE_FACE_COUNT)
            .map(|face| {
                (0..MIPS)
                    .map(|mip| {
                        texture.create_view(&TextureViewDescriptor {
                            dimension: Some(TextureViewDimension::D2),
                            base_mip_level: mip,
                            mip_level_count: Some(1),
                            base_array_layer: face,
                            array_layer_count: Some(1),
                            ..default()
                        })
                    })
                    .collect()
            })
            .collect();
        let target_layer = |layer: u32, mip: u32| {
            target_texture.create_view(&TextureViewDescriptor {
                dimension: Some(TextureViewDimension::D2),
                base_mip_level: mip,
                mip_level_count: Some(1),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..default()
            })
        };
        let target_faces = (0..CUBE_FACE_COUNT)
            .map(|face| (0..MIPS).map(|mip| target_layer(face, mip)).collect())
            .collect();
        let target_diffuse_faces = (0..CUBE_FACE_COUNT)
            .map(|face| {
                diffuse_target_texture.create_view(&TextureViewDescriptor {
                    dimension: Some(TextureViewDimension::D2),
                    base_array_layer: face,
                    array_layer_count: Some(1),
                    ..default()
                })
            })
            .collect();
        Self {
            array,
            _texture: texture,
            faces,
            target_texture,
            diffuse_target_texture,
            target_faces,
            target_diffuse_faces,
            history: Mutex::default(),
            raw_cube,
            diffuse_faces,
            sky_view,
            sky_specular,
            sky_diffuse,
            epoch: AtomicU64::new(u64::MAX),
            pending_refilter: AtomicU64::new(0),
            draw_groups: default(),
        }
    }
}

pub(crate) fn layout() -> BindGroupLayoutDescriptor {
    let mut entries = [
        BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(std::mem::size_of::<EnhancedFrameGpu>() as u64),
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        BindingType::Sampler(SamplerBindingType::Filtering),
        BindingType::Texture {
            sample_type: TextureSampleType::Depth,
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(binding, ty)| BindGroupLayoutEntry {
        binding: binding as u32,
        visibility: ShaderStages::FRAGMENT,
        ty,
        count: None,
    })
    .collect::<Vec<_>>();
    entries.push(BindGroupLayoutEntry {
        binding: 13,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D3,
            multisampled: false,
        },
        count: None,
    });
    entries.push(BindGroupLayoutEntry {
        binding: 14,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Sampler(SamplerBindingType::Filtering),
        count: None,
    });
    entries.push(BindGroupLayoutEntry {
        binding: 22,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D3,
            multisampled: false,
        },
        count: None,
    });
    entries.push(BindGroupLayoutEntry {
        binding: 23,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Sampler(SamplerBindingType::Filtering),
        count: None,
    });
    BindGroupLayoutDescriptor::new("enhanced probe capture layout", &entries)
}

#[derive(Resource)]
pub(crate) struct ProbePipelines {
    capture: CachedRenderPipelineId,
    background: CachedRenderPipelineId,
    mip: CachedRenderPipelineId,
    cloud_sky: CachedRenderPipelineId,
    environment: probe_filter::EnvironmentFilter,
    history: probe_history::ReflectionResolve,
}

pub(crate) fn prepare_quality(
    origin: Res<ProbeOrigin>,
    mut pipelines: ResMut<ProbePipelines>,
    gpu: Res<ProbeGpu>,
    queue: Res<bevy::render::renderer::RenderQueue>,
) {
    if origin.source.is_none() {
        gpu.pending_refilter.store(0, Ordering::Relaxed);
    }
    pipelines.environment.set_quality(&queue, origin.quality);
}
impl FromWorld for ProbePipelines {
    fn from_world(world: &mut World) -> Self {
        let vertex = world.resource::<FullscreenShader>().to_vertex_state();
        let cache = world.resource::<PipelineCache>();
        let queue = |name: &'static str| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(name.into()),
                layout: vec![layout()],
                vertex: vertex.clone(),
                fragment: Some(FragmentState {
                    shader: PROBE_SHADER,
                    entry_point: Some(name.into()),
                    targets: vec![Some(ColorTargetState {
                        format: TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        };
        Self {
            capture: queue("capture_probe"),
            background: queue("capture_sky"),
            mip: queue("filter_mip"),
            cloud_sky: queue("cloud_environment"),
            environment: probe_filter::EnvironmentFilter::new(world),
            history: probe_history::ReflectionResolve::new(world),
        }
    }
}

/// Publishes incident atmosphere and cloud radiance without adding captured geometry bounce.
pub(crate) fn update_environment_sky(
    context: &mut RenderContext,
    world: &World,
    frame: &Buffer,
    source: &TextureView,
    depth: &TextureView,
) -> bool {
    let gpu = world.resource::<ProbeGpu>();
    let filter = &world.resource::<ProbePipelines>().environment;
    if !filter.ready(world) {
        return false;
    }
    if !draw(
        context,
        world,
        frame,
        source,
        depth,
        &gpu.sky_view,
        ProbePass::CloudSky,
    ) {
        return false;
    }
    if !draw(
        context,
        world,
        frame,
        &gpu.sky_view,
        depth,
        &gpu.sky_specular[0],
        ProbePass::Copy,
    ) {
        return false;
    }
    filter.sky(context, world)
}

#[cfg(test)]
pub(crate) fn environment_filter_layout() -> BindGroupLayoutDescriptor {
    probe_filter::layout()
}

#[cfg(test)]
pub(crate) fn reflection_resolve_layout() -> BindGroupLayoutDescriptor {
    probe_history::layout()
}

#[cfg(test)]
pub(crate) fn reflection_resolve_target() -> ColorTargetState {
    probe_history::colour_target()
}

/// Advances displayed captures once per rendered main view without rebuilding bind groups.
pub(crate) fn resolve_reflections(context: &mut RenderContext, world: &World, seconds: f32) {
    let gpu = world.resource::<ProbeGpu>();
    let origin = world.resource::<ProbeOrigin>();
    if origin.source.is_none() || gpu.epoch.load(Ordering::Relaxed) != origin.epoch {
        return;
    }
    let resolve = &world.resource::<ProbePipelines>().history;
    if !resolve.ready(world) {
        return;
    }
    let pending = gpu.pending_refilter.load(Ordering::Relaxed);
    if pending != 0 {
        let face = pending.trailing_zeros() as usize;
        let filter = &world.resource::<ProbePipelines>().environment;
        if filter.capture(context, world, face) {
            probe_history::publish_initial(context, gpu, face);
            gpu.pending_refilter
                .fetch_and(!(1 << face), Ordering::Relaxed);
        }
    }
    let weights = gpu.history.lock().unwrap().weights(seconds);
    for (face, weight) in weights.into_iter().enumerate() {
        if let Some(weight) = weight {
            resolve.resolve(context, world, face, weight);
        }
    }
}

pub(crate) fn filter_mips(
    context: &mut RenderContext,
    world: &World,
    frame: &Buffer,
    views: &[TextureView],
    depth: &TextureView,
) {
    for mip in 1..views.len() {
        draw(
            context,
            world,
            frame,
            &views[mip - 1],
            depth,
            &views[mip],
            ProbePass::Copy,
        );
    }
}

pub(crate) fn capture(
    context: &mut RenderContext,
    world: &World,
    face: u32,
    frame: &Buffer,
    source: &TextureView,
    depth: &TextureView,
) {
    let views = &world.resource::<super::gpu::EnhancedViews>().0;
    let Some(capture_view) = views.values().find(|view| view.frame.id() == frame.id()) else {
        return;
    };
    if !capture_view.capture_lighting_ready
        || !capture_view
            .capture_background_submitted
            .load(Ordering::Relaxed)
        || capture_view
            .capture_shadow_submission
            .as_ref()
            .is_some_and(|(submitted, frame)| submitted.load(Ordering::Relaxed) != *frame)
    {
        return;
    }
    let gpu = world.resource::<ProbeGpu>();
    if !world.resource::<ProbePipelines>().environment.ready(world) {
        return;
    }
    let epoch = world.resource::<ProbeOrigin>().epoch;
    if gpu.epoch.swap(epoch, Ordering::Relaxed) != epoch {
        gpu.pending_refilter.store(0, Ordering::Relaxed);
        probe_history::reset(context, gpu);
    }
    let views = &gpu.target_faces[face as usize];
    if draw(
        context,
        world,
        frame,
        source,
        depth,
        &views[0],
        ProbePass::Capture,
    ) {
        let complete =
            world
                .resource::<ProbePipelines>()
                .environment
                .capture(context, world, face as usize);
        if complete {
            probe_history::publish(context, gpu, face as usize);
            let origin = world.resource::<ProbeOrigin>();
            let previous = origin.face_mask();
            origin.complete_face(face);
            if previous != (1 << CUBE_FACE_COUNT) - 1
                && origin.face_mask() == (1 << CUBE_FACE_COUNT) - 1
            {
                // Refresh earlier lobes one at a time after the raw cube is complete.
                gpu.pending_refilter.store(
                    ((1 << CUBE_FACE_COUNT) - 1) & !(1 << face),
                    Ordering::Relaxed,
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "probe_lifecycle_tests.rs"]
mod tests;
