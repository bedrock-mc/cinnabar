use super::*;

/// Selects valid authored pages whose upload spans multiple ordinary frames.
fn begin_upload(app: &mut App) -> ChunkTextureBindingIdentity {
    let side = assets::texture::PBR_TILE_SIZE;
    let array = TextureArray {
        layers: 2,
        mips: (0..=side.ilog2())
            .map(|level| {
                let size = side >> level;
                assets::texture::TextureMip {
                    size,
                    rgba8: vec![128; (size * size * 2 * 4) as usize].into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };
    let maps = Arc::new(
        EnhancedTextureAssets::new(
            [array.clone(), array.clone()],
            [array.clone(), array.clone()],
            [array.clone(), array],
            vec![u32::MAX; EnhancedTextureAssets::REFERENCE_BASE_WORDS].into_boxed_slice(),
        )
        .unwrap(),
    );
    let candidate = app
        .world()
        .resource::<ChunkTextureAssets>()
        .with_updated_enhanced(Some(maps));
    let identity = candidate.binding_identity();
    app.insert_resource(candidate);
    app.world_mut()
        .run_system_once(prepare_chunk_texture_assets)
        .unwrap();
    let gpu = app.world().resource::<ChunkGpuTextureAssets>();
    assert!(gpu.authored_pending.is_some());
    assert_ne!(gpu.prepared.as_ref().unwrap().identity, identity);
    identity
}

#[test]
fn pending_authored_upload_keeps_chunk_streaming_active() {
    let (mut app, instances) = publication_app();
    let before = published(&app);
    let identity = begin_upload(&mut app);
    let mut next = instances[0].1.clone();
    next.key.x = 10;
    next.origin = render_scene::chunk::chunk_origin(next.key);
    let next = app.world_mut().spawn(next).id();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    assert!(app.world().get::<GpuChunkAllocation>(next).is_some());
    let during = published(&app);
    assert_eq!((during.0, during.1), (before.0, before.1));
    for _ in 0..3 {
        app.world_mut()
            .run_system_once(prepare_chunk_texture_assets)
            .unwrap();
    }
    assert_eq!(published(&app).0, identity);
}

#[test]
fn pending_authored_upload_keeps_biome_uploads_active() {
    let (mut app, _) = publication_app();
    app.init_resource::<ChunkGpuBiomeTints>();
    app.world_mut()
        .run_system_once(prepare_chunk_biome_tints)
        .unwrap();
    let before = app
        .world()
        .resource::<ChunkGpuBiomeTints>()
        .prepared
        .as_ref()
        .unwrap()
        .identity;
    begin_upload(&mut app);
    let source = ChunkBiomeTints::with_revision(vec![BiomeTint::default()].into(), 7);
    let identity = source.resource_identity();
    assert_ne!(before, identity);
    app.insert_resource(source);
    app.world_mut()
        .run_system_once(prepare_chunk_biome_tints)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<ChunkGpuBiomeTints>()
            .prepared
            .as_ref()
            .unwrap()
            .identity,
        identity
    );
}
