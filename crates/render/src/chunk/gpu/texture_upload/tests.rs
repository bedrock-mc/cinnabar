use super::*;

/// Both atlas views must share one uploaded allocation per physical page.
#[test]
fn atlas_views_share_each_uploaded_texture_page() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions::enabled(),
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let (prepared, stats) =
        build_chunk_texture_assets(&ChunkTextureAssets::default(), &device, &queue).unwrap();

    let report = instance.generate_report().unwrap();
    assert_eq!(report.hub.textures.num_allocated, assets::MAX_TEXTURE_PAGES);
    assert_eq!(
        report.hub.texture_views.num_allocated,
        crate::material_shader::CHUNK_SAMPLED_TEXTURE_BINDINGS as usize
    );
    assert_eq!(
        report.hub.samplers.num_allocated,
        crate::material_shader::CHUNK_SAMPLER_COUNT as usize
    );

    let mut uploaded_bytes = 0;
    for ((texture, view), native_view) in prepared
        ._textures
        .iter()
        .zip(&prepared.views)
        .zip(&prepared.native_leaf_views)
    {
        assert_eq!(texture.format(), TextureFormat::Rgba8UnormSrgb);
        assert_eq!(view.texture(), &**texture);
        assert_eq!(native_view.texture(), &**texture);
        assert_ne!(view.id(), native_view.id());
        // Creating the alternate view exercises format compatibility validation.
        let _linear_view = texture.create_view(&TextureViewDescriptor {
            format: Some(TextureFormat::Rgba8Unorm),
            dimension: Some(TextureViewDimension::D2Array),
            ..Default::default()
        });
        for mip in 0..texture.mip_level_count() {
            let size = texture.size().mip_level_size(mip, texture.dimension());
            uploaded_bytes += u64::from(size.width)
                * u64::from(size.height)
                * u64::from(size.depth_or_array_layers)
                * u64::from(texture.format().block_copy_size(None).unwrap());
        }
    }
    assert_eq!(stats.queue_upload_bytes, uploaded_bytes);
}
