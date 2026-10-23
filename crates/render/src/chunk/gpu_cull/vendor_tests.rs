//! Vendor admission must prevent shader compilation in both terrain culling paths.

use bevy::render::renderer::RenderAdapterInfo;

use super::*;

/// Installs terrain culling on a validating device reporting the requested vendor.
fn install_vendor(vendor: u32, features: WgpuFeatures) -> (App, wgpu::Instance) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions::enabled(),
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, _) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    }))
    .unwrap();
    let mut info = adapter.get_info();
    info.vendor = vendor;
    let mut render = SubApp::new();
    render
        .insert_resource(RenderDevice::from(device))
        .insert_resource(RenderAdapter::new(adapter))
        .insert_resource(RenderAdapterInfo::new(info));
    let mut app = App::new();
    app.insert_sub_app(RenderApp, render);
    install(&mut app);
    (app, instance)
}

#[test]
fn mali_and_powervr_never_create_terrain_cull_compute_pipelines() {
    for vendor in [POWERVR_VENDOR_ID, ARM_MALI_VENDOR_ID] {
        for features in [
            WgpuFeatures::empty(),
            WgpuFeatures::INDIRECT_FIRST_INSTANCE,
            WgpuFeatures::INDIRECT_FIRST_INSTANCE | WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT,
        ] {
            let (app, instance) = install_vendor(vendor, features);
            assert_eq!(
                instance
                    .generate_report()
                    .unwrap()
                    .hub
                    .compute_pipelines
                    .num_allocated,
                0,
                "vendor {vendor:#x}, features {features:?} must never compile terrain cull shaders"
            );
            let world = app.sub_app(RenderApp).world();
            assert!(!world.resource::<GpuCullSupport>().0);
            assert!(!world.resource::<DirectOcclusionSupport>().0);
            assert!(!world.contains_resource::<GpuCull>());
            assert!(!world.contains_resource::<DirectOcclusion>());
        }
    }
}

#[test]
fn desktop_vendors_still_create_terrain_cull_compute_pipelines() {
    for vendor in [0x10de, 0x1002, 0x8086] {
        for features in [WgpuFeatures::empty(), WgpuFeatures::INDIRECT_FIRST_INSTANCE] {
            let (app, instance) = install_vendor(vendor, features);
            assert!(
                instance
                    .generate_report()
                    .unwrap()
                    .hub
                    .compute_pipelines
                    .num_allocated
                    > 0
            );
            let world = app.sub_app(RenderApp).world();
            assert_eq!(world.resource::<GpuCullSupport>().0, !features.is_empty());
            assert_eq!(
                world.resource::<DirectOcclusionSupport>().0,
                features.is_empty()
            );
        }
    }
}
