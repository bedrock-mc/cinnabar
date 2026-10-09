use super::*;

/// A transfer while a pack reload holds the resident set must not stall the new session.
#[test]
fn a_session_reset_releases_a_held_resource_geometry_snapshot() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(8));
    // Without a render sub-app the plugin leaves the reload bridge to the caller.
    let reload = ChunkTextureReload::default();
    app.insert_resource(reload.clone());
    let assets = ChunkTextureAssets::with_revision(Arc::new(RuntimeAssets::diagnostic()), 7);
    reload.request_geometry(assets, Arc::from([]));
    assert!(reload.geometry_pending());

    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .reset_session();
    app.update();
    assert!(!reload.geometry_pending(), "the reset releases the hold");
    assert!(reload.requested().is_some(), "the atlas request survives");

    let key = SubChunkKey::new(0, 0, 0, 0);
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(key, solid_test_mesh(), ChunkUploadPriority::new(0.0))
        .unwrap();
    app.update();
    let world = app.world_mut();
    assert_eq!(world.resource::<ChunkRenderQueue>().pending_len(), 0);
    let mut instances = world.query::<&ChunkRenderInstance>();
    assert_eq!(
        instances
            .iter(world)
            .map(|instance| instance.key)
            .collect::<Vec<_>>(),
        [key],
        "the new session's chunk is applied"
    );
}

#[test]
fn review_render_discard_rolls_back_new_and_replacement_manifest_entries() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut queue = ChunkRenderQueue::default();
    queue
        .try_insert(key, solid_test_mesh(), ChunkUploadPriority::new(0.0))
        .unwrap();
    queue.discard_resource_work();
    assert!(queue.render_manifest.is_empty());
    assert_eq!(queue.pending_len(), 0);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(8));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(key, solid_test_mesh(), ChunkUploadPriority::new(0.0))
        .unwrap();
    app.update();
    let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
    let resident = queue.render_manifest[&key];
    for _ in 0..2 {
        queue
            .try_update(key, solid_test_mesh(), ChunkUploadPriority::new(0.0))
            .unwrap();
    }
    queue.discard_resource_work();
    assert_eq!(queue.render_manifest[&key], resident);
    queue.try_remove(key).unwrap();
    queue
        .try_update(key, solid_test_mesh(), ChunkUploadPriority::new(0.0))
        .unwrap();
    queue.discard_resource_work();
    assert_eq!(queue.render_manifest.get(&key), Some(&resident));
    queue.try_remove(key).unwrap();
    queue.discard_resource_work();
    assert!(queue.render_manifest.is_empty());
    assert_eq!(queue.removals.len(), 1);
}
