use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_scene::{ChunkRenderInstance, ChunkUploadAcknowledgements, ChunkUploadBudget};

/// Builds an untracked cube upload with valid paired lighting and no external assets.
fn instance() -> ChunkRenderInstance {
    let mesh = solid_test_mesh();
    ChunkRenderInstance {
        #[cfg(feature = "enhanced")]
        light_emitters: Default::default(),
        key: SubChunkKey::new(0, 0, 0, 0),
        cube_quads: mesh.quads().into(),
        cube_lighting: mesh.cube_lighting().into(),
        cube_layout: mesh.cube_layout(),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from([]),
        liquid_quads: Arc::from([]),
        liquid_lighting: Arc::from([]),
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::default(),
        generation: 1,
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
        origin: [0; 3],
    }
}

/// Runs a frame and returns work counters without relying on elapsed time.
fn prepare(app: &mut App) -> ChunkGpuUploadStats {
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    *app.world().resource::<ChunkGpuUploadStats>()
}

#[test]
fn byte_blocked_backlog_does_no_payload_preparation() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    let template = instance();
    let backlog = 4096;
    app.insert_resource(ChunkUploadBudget::new(
        1,
        chunk_instance_upload_byte_len(&template) - 1,
    ));
    for index in 0..backlog {
        let mut instance = template.clone();
        instance.key = SubChunkKey::new(0, index, 0, 0);
        app.world_mut().spawn(instance);
    }
    let stats = prepare(&mut app);
    eprintln!(
        "blocked backlog={backlog} examined={} validated={} range_plans={} admitted={} deferred={}",
        stats.candidates_examined,
        stats.model_streams_validated,
        stats.range_plans,
        stats.chunk_updates,
        stats.candidates_deferred,
    );
    assert_eq!(stats.chunk_updates, 0);
    assert_eq!(stats.candidates_deferred, backlog as usize);
    assert_eq!(stats.model_streams_validated, 0);
    assert_eq!(stats.range_plans, 0);
    assert!(stats.candidates_examined <= 2 * PublicationServiceConfig::DEFAULT.maximum_frame_items);
}

#[test]
fn allocation_retries_reuse_model_validation() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(ChunkUploadBudget::new(1, u64::MAX));
    app.world_mut()
        .resource_mut::<ChunkGpuArena>()
        .limits
        .max_quad_items = 0;
    app.world_mut().spawn(instance());
    let first = prepare(&mut app);
    let retry = prepare(&mut app);
    eprintln!(
        "retry first_validated={} retry_validated={}",
        first.model_streams_validated, retry.model_streams_validated
    );
    assert_eq!(first.model_streams_validated, 1);
    assert_eq!(first.chunk_updates, 0);
    assert_eq!(retry.chunk_updates, 0);
    assert_eq!(retry.model_streams_validated, 0);
}

#[test]
fn blocked_replacement_releases_obsolete_validation_allocations() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(ChunkUploadBudget::new(1, u64::MAX));
    let mut instance = instance();
    instance.model_refs = Arc::from([PackedModelRef::new(0, 0, 0, 1)]);
    instance.model_lighting = Arc::from([PackedQuadLighting::new([0; 4])]);
    let refs = Arc::clone(&instance.model_refs);
    let entity = app.world_mut().spawn(instance).id();
    assert_eq!(prepare(&mut app).model_streams_validated, 1);
    assert_eq!(Arc::weak_count(&refs), 1);

    // Replacing one stream drops its last owner; the other streams can stay shared.
    app.world_mut()
        .get_mut::<ChunkRenderInstance>(entity)
        .unwrap()
        .model_lighting = Arc::from([PackedQuadLighting::new([0; 4])]);
    app.insert_resource(ChunkUploadBudget::new(0, 0));
    let stats = prepare(&mut app);
    assert_eq!(stats.model_streams_validated, 0);
    assert_eq!(stats.chunk_updates, 0);
    assert_eq!(Arc::weak_count(&refs), 0);
}

#[test]
fn oversized_prefix_cannot_hide_a_smaller_upload() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    let small = instance();
    app.insert_resource(ChunkUploadBudget::new(
        1,
        chunk_instance_upload_byte_len(&small),
    ));
    let mut large = small.clone();
    large.cube_quads = small.cube_quads.repeat(2).into();
    large.cube_lighting = small.cube_lighting.repeat(2).into();
    let limit = PublicationServiceConfig::DEFAULT.maximum_frame_items;
    let backlog = 8 * limit;
    for index in 0..backlog {
        large.key = SubChunkKey::new(0, index as i32, 0, 0);
        app.world_mut().spawn(large.clone());
    }
    let mut small = small;
    small.key = SubChunkKey::new(0, backlog as i32, 0, 0);
    let entity = app.world_mut().spawn(small).id();
    for _ in 0..backlog.div_ceil(limit) {
        let stats = prepare(&mut app);
        assert!(stats.candidates_examined <= 2 * limit);
        assert!(stats.model_streams_validated <= 1);
        if stats.chunk_updates == 1 {
            assert!(
                app.world()
                    .resource::<ChunkGpuArena>()
                    .allocations
                    .contains_key(&entity)
            );
            return;
        }
    }
    panic!("a byte-blocked prefix hid the fitting upload for a complete scan");
}

#[test]
fn zero_item_budget_skips_model_validation() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(ChunkUploadBudget::new(0, u64::MAX));
    app.world_mut().spawn(instance());
    let stats = prepare(&mut app);
    assert_eq!(stats.model_streams_validated, 0);
    assert_eq!(stats.range_plans, 0);
    assert_eq!(stats.chunk_updates, 0);
}

#[test]
fn validation_cache_tracks_every_model_stream_generation_and_asset_identity() {
    use crate::chunk::gpu::upload::model_validation::ModelValidationCache;

    let mut cache = ModelValidationCache::default();
    let mut instance = instance();
    let mut assets = ChunkTextureAssets::default();
    let entity = Entity::from_bits(1);
    let limit = 1;
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (true, true)
    );
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (true, false)
    );
    // A same-generation stream replacement must not reuse a successful verdict.
    instance.model_refs = Arc::from([PackedModelRef::new(0, 0, 0, 1)]);
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, false)
    );
    instance.model_lighting = Arc::from([PackedQuadLighting::new([0; 4])]);
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    instance.model_draw_refs = Arc::from([PackedModelDrawRef::new(0, 0)]);
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    instance.transparent_model_draw_refs = Arc::from([PackedModelDrawRef::new(0, 0)]);
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    instance.generation += 1;
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    assets.revision += 1;
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    assets.assets = Arc::new(RuntimeAssets::diagnostic());
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    cache.retain_waiters(&HashMap::new());
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
}
