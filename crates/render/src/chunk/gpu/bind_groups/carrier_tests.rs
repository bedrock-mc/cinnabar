use super::*;
use bevy::render::renderer::WgpuWrapper;

/// Creates complete authored arrays for carrier replacement tests.
fn authored(side: u32, layers: u32) -> Arc<EnhancedTextureAssets> {
    let array = TextureArray {
        layers,
        mips: (0..=side.ilog2())
            .map(|level| {
                let size = side >> level;
                TextureMip {
                    size,
                    rgba8: vec![128; (size * size * layers * 4) as usize].into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };
    Arc::new(
        EnhancedTextureAssets::new(
            [array.clone(), array.clone()],
            [array.clone(), array.clone()],
            [array.clone(), array],
            vec![u32::MAX; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS]
                .into_boxed_slice(),
        )
        .unwrap(),
    )
}

/// Starts a real preparation system with a ready carrier and authored GPU maps.
fn ready(source: Arc<EnhancedTextureAssets>) -> App {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let assets =
        ChunkTextureAssets::with_enhanced(Arc::new(RuntimeAssets::diagnostic()), source, 0);
    let (prepared, stats) = build_chunk_texture_assets(&assets, &device, &queue).unwrap();
    let mut app = App::new();
    app.insert_resource(ChunkGpuArena::new(&device))
        .insert_resource(device)
        .insert_resource(queue)
        .insert_resource(ChunkGpuTextureAssets {
            attempted_identity: Some(assets.identity()),
            _attempted_assets: Some(assets.assets().clone()),
            prepared: Some(prepared),
            ..Default::default()
        })
        .insert_resource(stats)
        .insert_resource(assets)
        .init_resource::<ChunkTextureReload>()
        .add_systems(Update, prepare_chunk_texture_assets);
    app
}

#[test]
fn carrier_replacement_reuses_unchanged_authored_gpu_maps() {
    let source = authored(4, 1);
    let mut app = ready(source.clone());
    let previous = app
        .world()
        .resource::<ChunkGpuTextureAssets>()
        .prepared
        .as_ref()
        .unwrap();
    let views = previous.enhanced_views.each_ref().map(|view| view.id());
    let references = previous.enhanced_texture_refs.id();
    let carrier = previous.views[0].id();
    let replacement =
        ChunkTextureAssets::with_enhanced(Arc::new(RuntimeAssets::diagnostic()), source, 1);
    let identity = replacement.identity();
    app.insert_resource(replacement);
    app.update();
    let gpu = app.world().resource::<ChunkGpuTextureAssets>();
    let prepared = gpu.prepared.as_ref().unwrap();
    assert_ne!(prepared.views[0].id(), carrier);
    assert_eq!(
        prepared.enhanced_views.each_ref().map(|view| view.id()),
        views
    );
    assert_eq!(prepared.enhanced_texture_refs.id(), references);
    assert_eq!(prepared.identity, identity);
    assert!(gpu.authored_pending.is_none());
}

#[test]
fn carrier_replacement_stages_changed_authored_maps_within_frame_budget() {
    let mut app = ready(authored(4, 1));
    let replacement = ChunkTextureAssets::with_enhanced(
        Arc::new(RuntimeAssets::diagnostic()),
        authored(assets::PBR_TILE_SIZE, 2),
        1,
    );
    let identity = replacement.identity();
    let source = replacement.enhanced().unwrap().clone();
    app.insert_resource(replacement);
    app.update();
    assert!(
        app.world()
            .resource::<ChunkGpuTextureAssets>()
            .authored_pending
            .is_some(),
        "changed maps larger than one frame budget must remain staged"
    );
    let fallback_refs = app
        .world()
        .resource::<ChunkGpuTextureAssets>()
        .prepared
        .as_ref()
        .unwrap()
        .enhanced_texture_refs
        .id();
    while app
        .world()
        .resource::<ChunkGpuTextureAssets>()
        .authored_pending
        .is_some()
    {
        app.update();
    }
    let prepared = app
        .world()
        .resource::<ChunkGpuTextureAssets>()
        .prepared
        .as_ref()
        .unwrap();
    assert_eq!(prepared.identity, identity);
    assert!(Arc::ptr_eq(
        prepared.authored_source.as_ref().unwrap(),
        &source
    ));
    assert_ne!(prepared.enhanced_texture_refs.id(), fallback_refs);
}
