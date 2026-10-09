//! Strafing over an ocean drives the production transparent sort and queue systems.
use super::*;
use bevy::{
    core_pipeline::core_3d::{Transparent3d, graph::Core3d},
    ecs::system::RunSystemOnce,
    render::{
        render_graph::RenderSubGraph,
        render_phase::{AddRenderCommand, DrawFunctions, ViewSortedRenderPhases},
        render_resource::PipelineCache,
        renderer::{RenderAdapter, WgpuWrapper},
        sync_world::MainEntity,
        view::RetainedViewEntity,
    },
};

const OCEAN_RADIUS: i32 = 8;
pub(super) const SURFACE_SUBCHUNK_Y: i32 = 3;
const STRAFE_FRAMES: usize = 480;
const STRAFE_STEP: f32 = 0.1;
/// Flat water draws without a sort, so the strafe needs shores to have anything to sort.
const STRAFE_SHORE_PERIOD: i32 = 3;

/// A flat 16x16 water surface, or with `shore` also one wall of side faces, which
/// makes its faces overlap on screen and their order matter.
pub(super) fn ocean_surface(key: SubChunkKey, shore: bool) -> ChunkRenderInstance {
    let mut quads = Vec::with_capacity(272);
    let mut push = |origin: [u8; 3], face: Face, heights: [u8; 4]| {
        let index = quads.len() as u32;
        quads.push(
            PackedLiquidQuad::try_pack(origin, face, heights, 0, index, [0; 2], false).unwrap(),
        );
    };
    for z in 0..16 {
        for x in 0..16 {
            push([x, 15, z], Face::PositiveY, [224; 4]);
        }
    }
    if shore {
        for z in 0..16 {
            push([0, 15, z], Face::NegativeX, [0, 224, 224, 0]);
        }
    }
    ChunkRenderInstance {
        cube_layout: CubeQuadLayout::default(),
        key,
        origin: chunk_origin(key),
        generation: 1,
        cube_quads: Arc::from([]),
        cube_lighting: Arc::from([]),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from([]),
        liquid_lighting: vec![PackedQuadLighting::new([0; 4]); quads.len()].into(),
        liquid_quads: quads.into(),
        has_depth_liquid: false,
        has_transparent_liquid: true,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::default(),
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
    }
}

/// Coarse 90-degree frustum around a horizontal `forward`, enough to churn membership.
pub(super) fn in_frustum(camera: Vec3, forward: Vec3, key: SubChunkKey) -> bool {
    let min = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
    let max = min + Vec3::splat(16.0);
    let right = Vec3::new(forward.z, 0.0, -forward.x);
    let planes = [forward + right, forward - right, forward];
    planes.iter().all(|normal| {
        let corner = Vec3::new(
            if normal.x >= 0.0 { max.x } else { min.x },
            if normal.y >= 0.0 { max.y } else { min.y },
            if normal.z >= 0.0 { max.z } else { min.z },
        );
        (corner - camera).dot(*normal) >= 0.0
    })
}

pub(super) struct Fixture {
    pub(super) app: App,
    pub(super) view: Entity,
    pub(super) retained: RetainedViewEntity,
    pub(super) surfaces: Vec<(Entity, SubChunkKey)>,
    /// Worker sort jobs that have finished, and the refs they sorted rather than reused.
    pub(super) jobs: usize,
    pub(super) sorted_refs: usize,
}

/// Every `shore_period`th sub-chunk (by `x + z`) also has side faces; zero means none do.
pub(super) fn is_shore(key: SubChunkKey, shore_period: i32) -> bool {
    shore_period != 0 && (key.x + key.z).rem_euclid(shore_period) == 0
}

/// An ocean of flat surfaces, with shores placed as [`is_shore`] describes.
pub(super) fn fixture_with(shore_period: i32) -> Fixture {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..Default::default()
    });
    let adapter =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
            ..Default::default()
        },
        ..Default::default()
    }))
    .unwrap();
    let device = RenderDevice::from(device);
    let adapter = RenderAdapter(Arc::new(WgpuWrapper::new(adapter)));
    let mut app = App::new();
    app.insert_resource(PipelineCache::new(device.clone(), adapter.clone(), false))
        .insert_resource(ChunkGpuArena::new(&device))
        .insert_resource(device)
        .insert_resource(adapter)
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
        .insert_resource(ChunkUploadBudget::new(usize::MAX, u64::MAX))
        .insert_resource(RuntimeStageProfiler::new(true))
        .insert_resource(ChunkPipeline::from_world(&mut World::new()))
        .init_resource::<ChunkTextureAssets>()
        .init_resource::<ChunkGpuUploadStats>()
        .init_resource::<ChunkBiomeTints>()
        .init_resource::<ChunkUploadAcknowledgements>()
        .init_resource::<ChunkGpuRemovalQueue>()
        .init_resource::<TransparentRetirementFence>()
        .init_resource::<GpuUpdateFairness>()
        .init_resource::<TransparentSortRuntime>()
        .init_resource::<TransparentModelSortRuntime>()
        .init_resource::<TransparentUploadBudget>()
        .init_resource::<TransparentSortMetrics>()
        .init_resource::<TransparentWitnessRequest>()
        .init_resource::<TransparentWitnessEvidence>()
        .init_resource::<crate::chunk::transparent::mixed::MixedTerrainRuntime>()
        .init_resource::<DrawFunctions<Transparent3d>>()
        .init_resource::<ViewSortedRenderPhases<Transparent3d>>()
        .add_render_command::<Transparent3d, DrawTransparentLiquidCommands>()
        .add_render_command::<Transparent3d, DrawTransparentLiquidDirectCommands>()
        .add_render_command::<Transparent3d, DrawTransparentModelCommands>()
        .add_render_command::<Transparent3d, crate::chunk::transparent::mixed::DrawMixedTerrainCommands>();
    let mut surfaces = Vec::new();
    for z in -OCEAN_RADIUS..=OCEAN_RADIUS {
        for x in -OCEAN_RADIUS..=OCEAN_RADIUS {
            let key = SubChunkKey::new(0, x, SURFACE_SUBCHUNK_Y, z);
            let surface = ocean_surface(key, is_shore(key, shore_period));
            surfaces.push((app.world_mut().spawn(surface).id(), key));
        }
    }
    while app.world().resource::<ChunkGpuArena>().allocations.len() < surfaces.len() {
        app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    }
    let view = app.world_mut().spawn_empty().id();
    let retained = RetainedViewEntity::new(view.into(), None, 0);
    app.world_mut().entity_mut(view).insert((
        MainEntity::from(view),
        Msaa::Off,
        ExtractedView {
            retained_view_entity: retained,
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            hdr: false,
            viewport: UVec4::new(0, 0, 1, 1),
            color_grading: default(),
            invert_culling: false,
        },
        ExtractedCamera {
            target: None,
            physical_viewport_size: None,
            physical_target_size: None,
            viewport: None,
            render_graph: Core3d.intern(),
            order: 0,
            output_mode: default(),
            msaa_writeback: default(),
            clear_color: default(),
            sorted_camera_index_for_target: 0,
            exposure: 1.0,
            hdr: false,
        },
        RenderVisibleEntities::default(),
    ));
    Fixture {
        app,
        view,
        retained,
        surfaces,
        jobs: 0,
        sorted_refs: 0,
    }
}

impl Fixture {
    fn frame(&mut self, camera: Vec3) {
        self.frame_looking(camera, Vec3::Z);
    }

    /// Streams one more water sub-chunk in and uploads it.
    pub(super) fn spawn_surface(&mut self, key: SubChunkKey, shore: bool) {
        let entity = self.app.world_mut().spawn(ocean_surface(key, shore)).id();
        self.surfaces.push((entity, key));
        while !self
            .app
            .world()
            .resource::<ChunkGpuArena>()
            .allocations
            .contains_key(&entity)
        {
            self.app
                .world_mut()
                .run_system_once(prepare_gpu_chunks)
                .unwrap();
        }
    }

    /// Water sub-chunks the coarse frustum admits from `camera` looking along `forward`.
    pub(super) fn visible_water(&self, camera: Vec3, forward: Vec3) -> BTreeSet<SubChunkKey> {
        self.surfaces
            .iter()
            .filter(|(_, key)| in_frustum(camera, forward, *key))
            .map(|&(_, key)| key)
            .collect()
    }

    /// Runs one prepare and queue for the camera at `camera` looking along `forward`.
    pub(super) fn frame_looking(&mut self, camera: Vec3, forward: Vec3) {
        let visible = self
            .surfaces
            .iter()
            .filter(|(_, key)| in_frustum(camera, forward, *key))
            .map(|&(entity, _)| (entity, MainEntity::from(entity)))
            .collect();
        let world = self.app.world_mut();
        let mut entity = world.entity_mut(self.view);
        entity.get_mut::<ExtractedView>().unwrap().world_from_view =
            GlobalTransform::from(Transform::from_translation(camera).looking_to(forward, Vec3::Y));
        entity
            .get_mut::<RenderVisibleEntities>()
            .unwrap()
            .entities
            .insert(std::any::TypeId::of::<ChunkRenderInstance>(), visible);
        world
            .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
            .insert_or_clear(self.retained);
        world.run_system_once(prepare_transparent_sorts).unwrap();
        world.run_system_once(queue_transparent_chunks).unwrap();
        // A frame is long enough for the worker; keep its result for the next prepare.
        let runtime = world.resource::<TransparentSortRuntime>();
        if runtime.gate.in_flight_generation().is_some() {
            let result = runtime.result_receiver.lock().unwrap().recv().unwrap();
            self.jobs += 1;
            self.sorted_refs += result
                .fresh
                .iter()
                .map(|order| order.refs.len())
                .sum::<usize>();
            runtime.result_sender.send(result).unwrap();
        }
    }
}

const STAGES: [RuntimeStage; 3] = [
    RuntimeStage::TransparentPreparation,
    RuntimeStage::TransparentWorker,
    RuntimeStage::TransparentQueue,
];

/// Returns per-stage samples and transparent upload bytes for one strafe pass.
fn strafe_pass(shore_period: i32) -> ([crate::runtime_profile::RuntimeStageSample; 3], u64, usize) {
    let mut fixture = fixture_with(shore_period);
    let start = Vec3::new(3.3, 64.62, -40.7);
    for _ in 0..8 {
        fixture.frame(start);
    }
    let profiler = fixture
        .app
        .world()
        .resource::<RuntimeStageProfiler>()
        .clone();
    let metrics = || {
        fixture
            .app
            .world()
            .resource::<TransparentSortMetrics>()
            .snapshot()
    };
    let warm_bytes = metrics().upload_bytes;
    let _ = profiler.take_snapshot_if_due(std::time::Duration::ZERO);
    for frame in 0..STRAFE_FRAMES {
        fixture.frame(start + Vec3::X * frame as f32 * STRAFE_STEP);
    }
    let snapshot = profiler
        .take_snapshot_if_due(std::time::Duration::ZERO)
        .unwrap();
    let metrics = fixture
        .app
        .world()
        .resource::<TransparentSortMetrics>()
        .snapshot();
    (
        STAGES.map(|stage| snapshot.samples[stage as usize]),
        metrics.upload_bytes - warm_bytes,
        metrics.ref_count,
    )
}

/// Runs three strafe passes over the ocean and prints each stage's fastest total.
fn report_strafe(label: &str, shore_period: i32) -> (u64, usize) {
    let passes = (0..3)
        .map(|_| strafe_pass(shore_period))
        .collect::<Vec<_>>();
    for (index, stage) in STAGES.iter().enumerate() {
        let best = passes
            .iter()
            .map(|(samples, _, _)| samples[index])
            .min_by_key(|sample| sample.total)
            .unwrap();
        println!(
            "{label} {}: count={} total={:.3}ms mean={:.1}us max={:.1}us",
            stage.name(),
            best.count,
            best.total.as_secs_f64() * 1e3,
            best.total.as_secs_f64() * 1e6 / best.count.max(1) as f64,
            best.maximum.as_secs_f64() * 1e6,
        );
    }
    let (_, uploaded, refs) = passes[0];
    println!("{label} upload_bytes={uploaded} over {STRAFE_FRAMES} frames, committed_refs={refs}");
    (uploaded, refs)
}

/// Strafing re-sorts and re-uploads only what moved, not the whole visible water set.
#[test]
fn strafing_over_water_uploads_only_changed_order() {
    // Flat water needs no sort at all, so strafing over it sorts and uploads nothing.
    assert_eq!(report_strafe("flat ocean", 0), (0, 0));
    let (uploaded, refs) = report_strafe("ocean with shores", STRAFE_SHORE_PERIOD);
    let snapshot_bytes = (refs * size_of::<PackedTransparentDrawRef>()) as u64;
    assert!(refs > 0);
    assert!(
        uploaded < 20 * snapshot_bytes,
        "{uploaded} bytes is a full re-upload on most frames"
    );
}
