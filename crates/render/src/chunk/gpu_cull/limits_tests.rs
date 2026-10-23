use super::kernels::CullKernels;

/// Both compute ceilings must allow pipeline creation on a 128-thread device.
#[test]
fn kernels_fit_128_thread_devices() {
    for (invocations, x) in [(128, 256), (256, 128), (128, 128)] {
        let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits {
                max_compute_invocations_per_workgroup: invocations,
                max_compute_workgroup_size_x: x,
                ..Default::default()
            },
            ..Default::default()
        });
        let limits = device.limits();
        assert!(
            super::gpu_cull_submission(
                super::ChunkDrawMode::MultiDrawIndirect,
                super::WgpuFeatures::INDIRECT_FIRST_INSTANCE,
                super::DownlevelFlags::COMPUTE_SHADERS,
                false,
                &limits
            )
            .is_some()
        );
        assert!(super::direct::direct_occlusion_supported(
            super::ChunkDrawMode::Direct,
            super::DownlevelFlags::COMPUTE_SHADERS,
            false,
            &limits
        ));
        assert_eq!(CullKernels::new(&device).workgroup, 128);
    }
}

/// Missing any required kernel resource keeps both drawing modes on CPU visibility.
#[test]
fn admission_rejects_incompatible_kernel_limits() {
    use super::*;
    for limits in [
        wgpu::Limits {
            max_compute_invocations_per_workgroup: 32,
            ..Default::default()
        },
        wgpu::Limits {
            max_compute_workgroup_size_x: 32,
            ..Default::default()
        },
        wgpu::Limits {
            max_compute_workgroup_size_y: 4,
            ..Default::default()
        },
        wgpu::Limits {
            max_compute_workgroup_storage_size: 512,
            ..Default::default()
        },
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: model::CULL_STORAGE_BINDINGS.len() as u32 - 1,
            ..Default::default()
        },
    ] {
        assert_eq!(
            gpu_cull_submission(
                ChunkDrawMode::MultiDrawIndirect,
                WgpuFeatures::INDIRECT_FIRST_INSTANCE,
                DownlevelFlags::COMPUTE_SHADERS,
                false,
                &limits
            ),
            None
        );
        assert!(!direct::direct_occlusion_supported(
            ChunkDrawMode::Direct,
            DownlevelFlags::COMPUTE_SHADERS,
            false,
            &limits
        ));
    }
}

/// Admission and dispatch use the workgroup dimensions declared by the depth kernels.
#[test]
fn depth_kernel_workgroups_match_admission() {
    let module = naga::front::wgsl::parse_str(super::kernels::pyramid_shader_source()).unwrap();
    assert!(!module.entry_points.is_empty());
    for entry in &module.entry_points {
        assert_eq!(
            entry.workgroup_size,
            [
                super::model::PYRAMID_WORKGROUP,
                super::model::PYRAMID_WORKGROUP,
                1
            ]
        );
    }
}

/// A recoverable allocation failure keeps ordinary terrain queued and retries later.
#[test]
fn cull_allocation_failures_keep_ordinary_terrain_queued_and_retry() {
    use super::{app_tests::*, *};
    for (features, label) in [
        (
            WgpuFeatures::INDIRECT_FIRST_INSTANCE,
            "terrain cull storage",
        ),
        (WgpuFeatures::empty(), "terrain occlusion storage"),
    ] {
        let (mut app, _) = chunk_app(noop_render_plugin(features), Msaa::Off, camera_transform());
        app.sub_app_mut(RenderApp)
            .edit_schedule(Render, |schedule| {
                schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
            });
        #[derive(Resource, Default)]
        struct Queued {
            culled: bool,
            ordinary: bool,
        }
        app.sub_app_mut(RenderApp)
            .init_resource::<Queued>()
            .add_systems(
                Render,
                (|frame: Res<GpuCullFrame>,
                  phases: Res<ViewBinnedRenderPhases<Opaque3d>>,
                  mut queued: ResMut<Queued>| {
                    queued.culled = frame.view.is_some();
                    queued.ordinary = phases
                        .0
                        .values()
                        .any(|phase| !phase.non_mesh_items.is_empty());
                })
                .in_set(RenderSystems::PhaseSort),
            );
        insert_meshes(&mut app, &KEYS);
        let failure = crate::gpu_allocation::inject(move |owner, _| owner == label);
        for _ in 0..8 {
            frame(&mut app);
        }
        let world = app.sub_app(RenderApp).world();
        assert!(!world.resource::<Queued>().culled);
        assert!(
            world.resource::<Queued>().ordinary,
            "ordinary terrain remains queued after allocation failure"
        );
        if features.is_empty() {
            assert!(!world.resource::<direct::DirectOcclusion>().ready);
        } else {
            assert!(!world.resource::<GpuCull>().ready);
        }
        drop(failure);
        for _ in 0..8 {
            frame(&mut app);
        }
        let world = app.sub_app(RenderApp).world();
        if features.is_empty() {
            assert!(world.resource::<direct::DirectOcclusion>().ready);
        } else {
            assert!(world.resource::<GpuCull>().ready);
        }
    }
}
