use super::*;

#[test]
fn session_reset_releases_old_publication_capacity_without_acknowledging_old_removals() {
    let config = client_world::PublicationServiceConfig::PHASE2_GATE;
    let allowance = client_world::PublicationAllowance::new(config);
    allowance.begin_frame(1, 2, 0, 2, config.maximum_frame_items);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(1));
    let token = ChunkUploadToken {
        generation: 1,
        dirty_since: Instant::now(),
    };
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_remove_tracked_permitted(
            SubChunkKey::new(0, 1, 0, 0),
            ChunkUploadPriority::new(0.0),
            token,
            allowance.try_admit_zero_byte().unwrap(),
        )
        .unwrap();
    app.update();
    assert_eq!(
        app.world().resource::<ChunkGpuRemovalQueue>().pending_len(),
        1
    );
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_remove_tracked_permitted(
            SubChunkKey::new(0, 2, 0, 0),
            ChunkUploadPriority::new(0.0),
            token,
            allowance.try_admit_zero_byte().unwrap(),
        )
        .unwrap();
    assert_eq!(allowance.live_permits(), 2);
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .reset_session();
    assert_eq!(
        allowance.live_permits(),
        1,
        "queued old work is dropped immediately"
    );
    app.update();
    assert_eq!(allowance.live_permits(), 0);
    assert_eq!(
        app.world().resource::<ChunkGpuRemovalQueue>().pending_len(),
        0
    );
    assert!(
        app.world()
            .resource::<ChunkUploadAcknowledgements>()
            .is_empty()
    );
}

#[test]
fn session_reset_retires_old_entities_and_discards_queued_work_before_same_key_reuse() {
    let reused = SubChunkKey::new(0, 1, 0, 0);
    let old_only = SubChunkKey::new(0, 2, 0, 0);
    let pending_only = SubChunkKey::new(0, 3, 0, 0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(8));
    for key in [reused, old_only] {
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_insert(key, solid_test_mesh(), ChunkUploadPriority::new(0.0))
            .unwrap();
    }
    app.update();
    let old_entity = app.world().resource::<ChunkEntities>().0[&reused];
    let old_generation = app
        .world()
        .get::<ChunkRenderInstance>(old_entity)
        .unwrap()
        .generation();
    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_insert(
                pending_only,
                solid_test_mesh(),
                ChunkUploadPriority::new(0.0),
            )
            .unwrap();
        queue.try_remove(old_only).unwrap();
        queue.reset_session();
        assert_eq!(queue.retained_len(), 0);
        assert_eq!(queue.pending_bytes(), 0);
        assert!(queue.render_manifest.is_empty());
        queue
            .try_insert(reused, solid_test_mesh(), ChunkUploadPriority::new(0.0))
            .unwrap();
    }
    app.update();
    let entities = &app.world().resource::<ChunkEntities>().0;
    assert_eq!(entities.len(), 1);
    let replacement = entities[&reused];
    assert_ne!(replacement, old_entity);
    assert!(app.world().get_entity(old_entity).is_err());
    assert!(
        app.world()
            .get::<ChunkRenderInstance>(replacement)
            .unwrap()
            .generation()
            > old_generation
    );
    assert_eq!(app.world().resource::<ChunkRenderQueue>().retained_len(), 0);

    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .reset_session();
    app.update();
    assert!(app.world().resource::<ChunkEntities>().0.is_empty());
    assert!(app.world().get_entity(replacement).is_err());
}
