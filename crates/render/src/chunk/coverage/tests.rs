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
