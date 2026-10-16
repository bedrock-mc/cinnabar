use super::*;
use crate::chunk::gpu_cull::app_tests::{
    KEYS, camera_transform, chunk_app, frame, insert_meshes, noop_render_plugin,
};

/// Records encoded work before the submission system consumes the plan.
#[derive(Resource, Default)]
struct EncodedWork {
    planned: bool,
    verdict: bool,
    encoded: bool,
}

/// Observes whether the terrain pass dispatched occlusion for the prepared plan.
fn capture_work(occlusion: Res<DirectOcclusion>, mut work: ResMut<EncodedWork>) {
    *work = occlusion
        .plan
        .as_ref()
        .map_or_else(EncodedWork::default, |plan| EncodedWork {
            planned: true,
            verdict: plan.verdict.is_some(),
            encoded: plan.encoded.load(Ordering::Acquire),
        });
}

#[test]
fn visibility_waits_for_new_pipeline_variants_when_msaa_changes() {
    let (mut app, camera) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Off,
        camera_transform(),
    );
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<DirectOcclusion>()
        .same_frame = true;
    insert_meshes(&mut app, &KEYS);
    for _ in 0..8 {
        frame(&mut app);
    }
    let before = app
        .sub_app(RenderApp)
        .world()
        .resource::<DirectOcclusion>()
        .depth_pipelines
        .len();
    *app.world_mut().get_mut::<Msaa>(camera).unwrap() = Msaa::Sample4;
    for _ in 0..8 {
        frame(&mut app);
    }
    let world = app.sub_app(RenderApp).world();
    let occlusion = world.resource::<DirectOcclusion>();
    assert!(occlusion.depth_pipelines.len() > before);
    assert!(occlusion.depth_pipelines.values().all(|&id| {
        world
            .resource::<PipelineCache>()
            .get_render_pipeline(id)
            .is_some()
    }));
}

#[test]
fn legacy_occlusion_does_no_compute_when_readbacks_are_full() {
    let (mut app, _) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Off,
        camera_transform(),
    );
    insert_meshes(&mut app, &KEYS);
    for _ in 0..16 {
        frame(&mut app);
    }
    {
        let world = app.sub_app_mut(RenderApp).world_mut();
        let mut occlusion = world.resource_mut::<DirectOcclusion>();
        occlusion.history = OcclusionHistory::default();
        occlusion.verdicts.reset();
        let tag = VerdictTag {
            frame: occlusion.frame,
            basis: occlusion.last_view.unwrap().1,
            slots: occlusion.table.slot_count(),
        };
        for _ in 0..SLOTS {
            let slot = occlusion.verdicts.acquire(tag).unwrap();
            drop(occlusion.verdicts.submit(slot));
        }
    }
    app.sub_app_mut(RenderApp)
        .init_resource::<EncodedWork>()
        .add_systems(
            Render,
            capture_work
                .after(bevy::render::renderer::render_system)
                .before(crate::device_poll::FrameSubmissions),
        );
    frame(&mut app);
    let world = app.sub_app(RenderApp).world();
    let work = world.resource::<EncodedWork>();
    assert!(work.planned && !work.verdict);
    assert!(world.resource::<DirectOcclusion>().stats.solid_drawn > 0);
    assert!(!work.encoded, "no consumer needs an occlusion dispatch");
}

#[cfg(target_os = "macos")]
#[test]
fn metal_visibility_bindings_allocate_nothing_on_unchanged_frames() {
    let Some((device, _)) = crate::upload_allocation_tests::metal_device() else {
        return;
    };
    let device = device.wgpu_device();
    let masks = crate::chunk::gpu_cull::unit_mask::UnitMasks::new(device);
    let buffer = || {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        })
    };
    let page = buffer();
    let bits = buffer();
    let mut bindings = None;
    masks.prepare(device, &page, Some(&bits), &mut bindings);
    let before = test_alloc::thread_allocations();
    masks.prepare(device, &page, Some(&bits), &mut bindings);
    assert_eq!(test_alloc::thread_allocations() - before, 0);
}
