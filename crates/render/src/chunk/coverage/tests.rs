use super::*;
use bevy::prelude::MinimalPlugins;
use render_scene::{ChunkRenderInstance, ChunkRenderQueue, ChunkUploadPriority, ChunkUploadToken};

#[test]
fn cancelled_queued_withdrawal_keeps_the_existing_near_instance() {
    let key = SubChunkKey::new(0, 5, 0, 0);
    let assets = assets::RuntimeAssets::diagnostic();
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets,
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &source,
    );
    assert!(!mesh.is_empty());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::default())
        .insert_resource(FarTerrainScene {
            target: Some(Arc::new(far_terrain::TerrainTarget {
                centre: key.chunk(),
                horizon: 32,
                near: std::sync::Arc::new(HashMap::from([(
                    key.chunk(),
                    HashMap::from([(key, 3)]),
                )])),
                retained: std::sync::Arc::new(HashSet::from([key])),
                empty_far: std::sync::Arc::new(HashSet::new()),
            })),
            ..Default::default()
        });
    let since = Instant::now();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked(
            key,
            mesh,
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 1,
                dirty_since: since,
            },
        )
        .unwrap();
    app.update();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_remove_tracked(
            key,
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 2,
                dirty_since: since,
            },
        )
        .unwrap();
    app.update();
    let world = app.world_mut();
    let mut query = world.query::<&ChunkRenderInstance>();
    let remaining = query
        .single(world)
        .expect("cancelled withdrawal retains the old cover");
    assert_eq!((remaining.key, remaining.generation), (key, 1));
    assert_eq!(world.resource::<ChunkRenderQueue>().retained_len(), 0);
}

/// Without a far target nothing reads empty proofs, so receipts are not copied every frame;
/// a target that arrives later still replays the source's current proofs.
#[test]
fn targetless_frames_leave_empty_proofs_for_the_next_target_to_replay() {
    use bevy::ecs::system::RunSystemOnce;
    let key = SubChunkKey::new(0, 3, 0, 4);
    let source = render_scene::terrain_publication::TerrainPublications::default();
    source.retain(std::iter::once(key));
    let mut app = crate::chunk::tests::gpu_publication::noop_gpu_publication_app(
        render_scene::ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.init_resource::<TerrainCoverage>()
        .insert_resource(FarTerrainScene {
            publications: Some(source.clone()),
            ..Default::default()
        });
    let frame = |app: &mut App| {
        let world = app.world_mut();
        world.run_system_once(begin_terrain_coverage).unwrap();
        world.run_system_once(publish_terrain_coverage).unwrap();
    };
    frame(&mut app);
    source.record(key, 2, true);
    frame(&mut app);
    assert_eq!(
        app.world().resource::<TerrainCoverage>().source_revision,
        None
    );

    app.world_mut().resource_mut::<FarTerrainScene>().target =
        Some(Arc::new(far_terrain::TerrainTarget {
            centre: key.chunk(),
            horizon: 32,
            near: Arc::new(HashMap::from([(key.chunk(), HashMap::from([(key, 2)]))])),
            retained: Arc::new(HashSet::from([key])),
            empty_far: Arc::new(HashSet::new()),
        }));
    frame(&mut app);
    let (revision, empty) = source.changed_empty(None).unwrap();
    assert_eq!(empty, HashMap::from([(key, 2)]));
    assert_eq!(
        app.world().resource::<TerrainCoverage>().source_revision,
        Some(revision)
    );
}
