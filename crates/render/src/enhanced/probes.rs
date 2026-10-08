//! Amortized geometry captures for reflections beyond the screen boundary.
use super::{EnhancedRendering, PROBE_SHADER, frame::EnhancedFrameGpu};
#[path = "probe_cache.rs"]
mod probe_cache;
#[path = "probe_filter.rs"]
mod probe_filter;
#[path = "probe_history.rs"]
mod probe_history;
use bevy::{
    camera::{Camera3dDepthTextureUsage, RenderTarget},
    core_pipeline::FullscreenShader,
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
        view::Hdr,
    },
};
use probe_cache::{ProbeLighting, ProbeSchedule};
use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
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
pub(crate) struct ProbeFace(pub u32);
#[derive(Resource, ExtractResource, Clone, Default)]
pub(crate) struct ProbeOrigin {
    pub position: Vec3,
    pub epoch: u64,
    pub quality: super::EnhancedQuality,
    pub(crate) source: Option<Entity>,
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
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
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
    schedule: ProbeSchedule,
    lighting: Option<ProbeLighting>,
    material: Option<crate::ChunkTextureAssetIdentity>,
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
    atmosphere: Option<Res<crate::AtmosphereFrame>>,
    materials: Option<Res<crate::ChunkTextureAssets>>,
    actors: Option<Res<crate::ActorRenderFrame>>,
    changed_chunks: Query<(), Changed<crate::ChunkRenderInstance>>,
    mut removed_chunks: RemovedComponents<crate::ChunkRenderInstance>,
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
        cameras.schedule = default();
        cameras.settings = None;
        if origin.source.take().is_some() {
            origin.epoch = origin.epoch.wrapping_add(1);
            origin.reset_faces();
        }
        return;
    };
    let source_changed = origin.source != Some(source_entity);
    origin.source = Some(source_entity);
    origin.quality = settings.quality;
    let budget = super::quality::budget(settings.quality);
    cameras
        .schedule
        .set_intervals(budget.reflection_refresh, budget.reflection_animation);
    let capture_order = source_camera.order.saturating_add(1);
    let now = time.as_ref().map_or(0.0, |time| time.elapsed_secs_f64());
    if changed_chunks.iter().next().is_some() || removed_chunks.read().count() != 0 {
        cameras.schedule.invalidate();
    }
    let material = materials.as_ref().map(|materials| materials.identity());
    let enhanced = capture_settings(*settings);
    let inputs_changed = cameras.material != material || cameras.settings != Some(enhanced);
    if inputs_changed {
        cameras.material = material;
        cameras.settings = Some(enhanced);
        cameras.schedule.invalidate();
    }
    if let Some(atmosphere) = atmosphere {
        let next = ProbeLighting {
            sun: Vec3::from_array(atmosphere.sun_direction()),
            phase: atmosphere.moon_phase(),
            rain: atmosphere.rain_level(),
            thunder: atmosphere.thunder_level(),
            zenith: Vec3::from_array(atmosphere.sky_zenith()),
            horizon: Vec3::from_array(atmosphere.sky_horizon()),
        };
        if cameras.lighting.is_none_or(|old| !old.reusable(next)) {
            if cameras.lighting.is_some_and(|old| old.discontinuity(next)) {
                origin.epoch = origin.epoch.wrapping_add(1);
                origin.reset_faces();
                cameras
                    .schedule
                    .relocate(preferred_face(pose.forward().as_vec3()));
            }
            cameras.lighting = Some(next);
            cameras.schedule.invalidate();
        }
    }
    if now >= cameras.schedule.next_inputs {
        let signature = actors.as_ref().map_or(0, |frame| actor_signature(frame));
        if signature != cameras.actor_signature {
            cameras.actor_signature = signature;
            cameras.schedule.invalidate();
        }
        let lighting_signature = origin.lighting_signature.load(Ordering::Relaxed);
        if cameras.lighting_signature != lighting_signature {
            cameras.lighting_signature = lighting_signature;
            cameras.schedule.invalidate();
        }
        cameras.schedule.next_inputs = now + budget.reflection_refresh;
    }
    cameras
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
            .schedule
            .relocate(preferred_face(pose.forward().as_vec3()));
    } else if inputs_changed {
        origin.epoch = origin.epoch.wrapping_add(1);
        origin.reset_faces();
        cameras
            .schedule
            .relocate(preferred_face(pose.forward().as_vec3()));
    }
    cameras.schedule.completed(origin.face_mask());
    let active_face = cameras.schedule.next_face(now);
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

// Clock stamps and interpolation fractions do not invalidate an otherwise unchanged pose.
fn actor_signature(frame: &crate::ActorRenderFrame) -> u64 {
    let mut hash = DefaultHasher::new();
    frame.skin_revision.hash(&mut hash);
    frame.artwork.identity().hash(&mut hash);
    frame.instance_pages.hash(&mut hash);
    frame.rig.geometry_revision.hash(&mut hash);
    bytemuck::cast_slice::<_, u8>(&frame.rig.current_bones).hash(&mut hash);
    bytemuck::cast_slice::<_, u8>(&frame.rig.previous_bones).hash(&mut hash);
    for instance in frame.rig.instances.iter() {
        bytemuck::bytes_of(&instance.world_from_actor).hash(&mut hash);
        instance.geometry_id.hash(&mut hash);
        instance.texture_layer.hash(&mut hash);
        instance.tint.hash(&mut hash);
        instance.overlay_rgba8.hash(&mut hash);
        instance.light.hash(&mut hash);
        instance.multitexture_layers.hash(&mut hash);
        bytemuck::bytes_of(&instance.uv_anim).hash(&mut hash);
    }
    hash.finish()
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

enum ProbePass {
    Capture,
    Copy,
    CloudSky,
}

fn draw(
    context: &mut RenderContext,
    world: &World,
    frame: &Buffer,
    source: &TextureView,
    depth: &TextureView,
    target: &TextureView,
    pass: ProbePass,
) -> bool {
    let noise = world.resource::<super::cloud_noise::CloudNoiseVolume>();
    let scattering = world.resource::<super::multiple_scattering::MultipleScattering>();
    if !noise.ready() || !scattering.ready() {
        return false;
    }
    let cache = world.resource::<PipelineCache>();
    let pipelines = world.resource::<ProbePipelines>();
    let Some(pipeline) = cache.get_render_pipeline(match pass {
        ProbePass::Capture => pipelines.capture,
        ProbePass::Copy => pipelines.mip,
        ProbePass::CloudSky => pipelines.cloud_sky,
    }) else {
        return false;
    };
    let gpu = world.resource::<super::gpu::EnhancedGpu>();
    let views = &world.resource::<super::gpu::EnhancedViews>().0;
    let bound_view = views.values().find(|view| view.frame.id() == frame.id());
    let sky_view = bound_view.and_then(|view| {
        view.capture_parent
            .and_then(|parent| views.get(&parent))
            .or(Some(view))
    });
    let sky = sky_view
        .and_then(|view| view.post.as_ref())
        .map_or(&gpu.fallback_colour, |post| &post.sky);
    let probe = world.resource::<ProbeGpu>();
    let mut groups = probe.draw_groups.lock().unwrap();
    let key = DrawBindingKey {
        frame: frame.id(),
        source: source.id(),
        depth: depth.id(),
        sky: sky.id(),
    };
    if groups.len() >= 64 && !groups.contains_key(&key) {
        groups.clear();
    }
    let group = groups.entry(key).or_insert_with(|| {
        context.render_device().create_bind_group(
            "enhanced probe filter",
            &cache.get_bind_group_layout(&layout()),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(source),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(&gpu.linear_sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(depth),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(sky),
                },
                BindGroupEntry {
                    binding: 13,
                    resource: BindingResource::TextureView(&noise.view),
                },
                BindGroupEntry {
                    binding: 14,
                    resource: BindingResource::Sampler(&noise.sampler),
                },
                BindGroupEntry {
                    binding: 22,
                    resource: BindingResource::TextureView(&scattering.view),
                },
                BindGroupEntry {
                    binding: 23,
                    resource: BindingResource::Sampler(&scattering.sampler),
                },
            ],
        )
    });
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("enhanced reflection capture/filter"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })],
        ..default()
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
    true
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
mod tests {
    use super::*;
    #[test]
    fn actor_clock_stamps_do_not_refresh_a_stationary_reflection() {
        let mut frame = crate::ActorRenderFrame::default();
        frame.rig.instances = Arc::from([crate::ActorGpuInstance::default()]);
        let unchanged = actor_signature(&frame);
        frame.rig.frame_generation += 1;
        Arc::make_mut(&mut frame.rig.instances)[0].partial_tick = 0.5;
        assert_eq!(actor_signature(&frame), unchanged);
        Arc::make_mut(&mut frame.rig.instances)[0].world_from_actor[0][3] = 5.0;
        assert_ne!(actor_signature(&frame), unchanged);
    }

    #[test]
    fn stale_render_feedback_cannot_complete_a_relocated_probe() {
        let mut origin = ProbeOrigin::default();
        origin.epoch = 1;
        origin.reset_faces();
        let previous_frame = origin.clone();
        previous_frame.complete_face(3);
        assert_eq!(origin.face_mask(), 1 << 3);
        origin.epoch = 2;
        origin.reset_faces();
        previous_frame.complete_face(1);
        assert_eq!(origin.face_mask(), 0);
        origin.complete_face(4);
        assert_eq!(origin.face_mask(), 1 << 4);
    }

    #[test]
    fn cube_capture_axes_are_orthogonal_and_cover_the_sphere() {
        let mut sum = Vec3::ZERO;
        for face in 0..6 {
            let (direction, up) = face_basis(face);
            assert_eq!(direction.dot(up), 0.0);
            sum += direction;
        }
        assert_eq!(sum, Vec3::ZERO);
    }

    #[test]
    fn probe_origin_hysteresis_avoids_eight_block_reflection_drops() {
        assert!(!should_relocate(Vec3::ZERO, Vec3::new(15.99, 0.0, 0.0)));
        assert!(!should_relocate(Vec3::ZERO, Vec3::new(0.0, 0.0, -16.0)));
        assert!(should_relocate(Vec3::ZERO, Vec3::new(16.01, 0.0, 0.0)));
    }
}
