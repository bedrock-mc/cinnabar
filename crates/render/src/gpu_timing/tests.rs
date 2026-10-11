use super::*;
use crate::render_test_support::record;
use bevy::prelude::Component;
use bevy::{
    ecs::system::RunSystemOnce,
    render::renderer::{PendingCommandBuffers, WgpuWrapper},
};

/// Creates a validation device with the requested timestamp capabilities.
pub(super) fn noop_device(features: wgpu::Features) -> (RenderDevice, RenderQueue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    }))
    .unwrap();
    (
        RenderDevice::from(device),
        RenderQueue(Arc::new(WgpuWrapper::new(queue))),
    )
}

/// Resolves submitted frame spans and requests their asynchronous readback.
pub(super) fn finish_frame(
    timestamps: &mut GpuTimestamps,
    device: &RenderDevice,
    queue: &RenderQueue,
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    timestamps.encode_readback(&mut encoder);
    queue.submit([encoder.finish()]);
    timestamps.request_readback(queue);
}

/// Completes pending samples and their readback copies before inspecting decoded frames.
pub(super) fn complete_readback(
    timestamps: &mut GpuTimestamps,
    device: &RenderDevice,
    queue: &RenderQueue,
) {
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    if cfg!(target_os = "macos") {
        finish_frame(timestamps, device, queue);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn completed_metal_samples_drain_when_every_readback_slot_is_occupied() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
    for _ in 0..SLOTS {
        let index = timestamps.ring.acquire().unwrap();
        timestamps.slots[index].passes = 1;
        timestamps.slots[index].stages[0] = RuntimeStage::GpuOpaque;
        timestamps.slots[index]
            .state
            .store(WRITTEN, Ordering::Release);
        timestamps.ring.submit(index);
    }
    timestamps.begin(|_| panic!("completed samples still need a readback copy"));
    assert!(timestamps.open_pass(RuntimeStage::GpuUi).is_none());
    let mut world = World::new();
    world.insert_resource(timestamps);
    let submissions = crate::device_poll::submissions_so_far(&queue);
    let buffers = run_readback(&mut world, &device);
    assert_eq!(
        crate::device_poll::submissions_so_far(&queue),
        submissions + 1
    );
    queue.submit(buffers);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    timestamps.request_readback(&queue);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mut frames = Vec::new();
    timestamps.begin(|frame| frames.push(*frame));
    assert_eq!(frames.len(), SLOTS);
    assert!(timestamps.ring.oldest_in_flight().is_none());
    assert!(timestamps.open_pass(RuntimeStage::GpuUi).is_some());
}

/// Runs the production readback system after all drawing commands have been recorded.
pub(super) fn run_readback(world: &mut World, device: &RenderDevice) -> Vec<wgpu::CommandBuffer> {
    world.insert_resource(device.clone());
    world.init_resource::<PendingCommandBuffers>();
    world.run_system_once(encode_readback).unwrap();
    world.resource_mut::<PendingCommandBuffers>().take()
}

/// Deferred passes and draws belong to the same readback as synchronously recorded work.
#[test]
fn readback_includes_queries_allocated_by_parallel_recording() {
    let pool = bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let features = wgpu::Features::TIMESTAMP_QUERY
        | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
        | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let (device, queue) = noop_device(features);
    for synchronous in [false, true] {
        let mut timestamps = GpuTimestamps::new(&device, &queue, true).unwrap();
        timestamps.begin(|_| unreachable!("first frame has no readback"));
        let mut world = World::new();
        world.insert_resource(timestamps);
        let (_, mut buffers) = record(&mut world, &device, |world, context| {
            let timestamps = world.resource::<GpuTimestamps>();
            if synchronous {
                let span = timestamps.open_pass(RuntimeStage::GpuUi).unwrap();
                context
                    .command_encoder()
                    .write_timestamp(span.queries, span.begin);
                context
                    .command_encoder()
                    .write_timestamp(span.queries, span.begin + 1);
            }
            let generated = pool.scope(|scope| {
                scope.spawn(async {
                    let mut encoder = device.create_command_encoder(&Default::default());
                    for span in [
                        timestamps.open_pass(RuntimeStage::GpuOpaque).unwrap(),
                        timestamps.open_draw(RuntimeStage::GpuActors).unwrap(),
                    ] {
                        encoder.write_timestamp(span.queries, span.begin);
                        encoder.write_timestamp(span.queries, span.begin + 1);
                    }
                    encoder.finish()
                });
            });
            for buffer in generated {
                context.add_command_buffer(buffer);
            }
        });
        buffers.extend(run_readback(&mut world, &device));
        queue.submit(buffers);
        let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
        timestamps.request_readback(&queue);
        let slot = &timestamps.slots[timestamps.ring.oldest_in_flight().unwrap()];
        let expected = if synchronous {
            vec![RuntimeStage::GpuUi, RuntimeStage::GpuOpaque]
        } else {
            vec![RuntimeStage::GpuOpaque]
        };
        assert_eq!(slot.stages[..slot.passes as usize], expected);
        assert_eq!(slot.draws, 1);
        assert_eq!(slot.stages[PASS_SPANS as usize], RuntimeStage::GpuActors);
        complete_readback(&mut timestamps, &device, &queue);
        let mut readbacks = 0;
        timestamps.begin(|_| readbacks += 1);
        assert_eq!(readbacks, 1);
    }
}

/// A render schedule whose timed opaque system counts executions.
fn timed_world() -> (World, Arc<AtomicU32>) {
    let runs = Arc::new(AtomicU32::new(0));
    let counter = runs.clone();
    let mut world = World::new();
    let mut schedule = bevy::ecs::schedule::Schedule::new(Core3d);
    schedule.add_systems(profiled(
        move || {
            counter.fetch_add(1, Ordering::Relaxed);
        },
        Some(RuntimeStage::GpuOpaque),
        "fixture opaque",
    ));
    world.add_schedule(schedule);
    (world, runs)
}

/// Runs the timed opaque system and returns its deferred command buffers.
fn run_opaque(world: &mut World, device: &RenderDevice) -> Vec<wgpu::CommandBuffer> {
    world.insert_resource(device.clone());
    world.init_resource::<PendingCommandBuffers>();
    world.run_schedule(Core3d);
    world.resource_mut::<PendingCommandBuffers>().take()
}

#[test]
fn missing_timestamp_feature_runs_nodes_untimed() {
    let (device, queue) = noop_device(wgpu::Features::empty());
    assert!(GpuTimestamps::new(&device, &queue, true).is_none());
    let (mut world, runs) = timed_world();
    assert!(run_opaque(&mut world, &device).is_empty());
    assert_eq!(runs.load(Ordering::Relaxed), 1);
}

#[test]
fn timed_frame_is_read_back_on_a_later_frame_without_waiting() {
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let (mut world, runs) = timed_world();
    let mut timestamps = GpuTimestamps::new(&device, &queue, true).unwrap();
    assert!(!timestamps.draw_spans);
    let mut frames = Vec::new();
    timestamps.begin(|frame| frames.push(*frame));
    world.insert_resource(timestamps);

    let mut buffers = run_opaque(&mut world, &device);
    if cfg!(target_os = "macos") {
        let (_, encoded) = record(&mut world, &device, |world, context| {
            let writes = render_pass_timestamps(world, RuntimeStage::GpuOpaque).unwrap();
            context
                .command_encoder()
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("NOOP readback lifecycle"),
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                        query_set: writes.query_set,
                        beginning_of_pass_write_index: writes.beginning_of_pass_write_index,
                        end_of_pass_write_index: writes.end_of_pass_write_index,
                    }),
                });
        });
        buffers.extend(encoded);
    }
    assert!(!buffers.is_empty());
    queue.submit(buffers);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    assert_eq!(timestamps.frame.passes.load(Ordering::Relaxed), 1);
    finish_frame(&mut timestamps, &device, &queue);
    assert!(timestamps.ring.oldest_in_flight().is_some());
    assert!(frames.is_empty());

    complete_readback(&mut timestamps, &device, &queue);
    timestamps.begin(|frame| frames.push(*frame));
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    assert_eq!(frames.len(), 1);
    // The NOOP backend writes no ticks, so the frame decodes to no durations.
    assert_eq!(frames[0].iter().count(), 0);
    assert!(timestamps.ring.oldest_in_flight().is_none());
}

#[test]
fn metal_wrappers_encode_no_synthetic_passes() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let (mut world, runs) = timed_world();
    let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
    timestamps.begin(|_| unreachable!("first frame has no readback"));
    world.insert_resource(timestamps);
    assert!(run_opaque(&mut world, &device).is_empty());
    let (_, buffers) = record(&mut world, &device, |world, context| {
        timed(world, context, RuntimeStage::GpuUi, |_| {})
    });
    assert!(buffers.is_empty());
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    let timestamps = world.resource::<GpuTimestamps>();
    assert_eq!(timestamps.frame.passes.load(Ordering::Relaxed), 0);
    assert_eq!(timestamps.frame.draws.load(Ordering::Relaxed), 0);
}

#[test]
fn metal_owned_pass_queries_reuse_bounded_storage() {
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
    timestamps.begin(|_| unreachable!("first frame has no readback"));
    let mut world = World::new();
    world.insert_resource(timestamps);
    if !cfg!(target_os = "macos") {
        assert!(render_pass_timestamps(&world, RuntimeStage::GpuUi).is_none());
        assert_eq!(
            world
                .resource::<GpuTimestamps>()
                .frame
                .passes
                .load(Ordering::Relaxed),
            0
        );
        return;
    }
    for index in 0..PASS_SPANS {
        let writes = render_pass_timestamps(&world, RuntimeStage::GpuUi).unwrap();
        assert!(std::ptr::eq(
            writes.query_set,
            &world.resource::<GpuTimestamps>().queries
        ));
        assert_eq!(writes.beginning_of_pass_write_index, Some(index * 2));
        assert_eq!(writes.end_of_pass_write_index, Some(index * 2 + 1));
    }
    assert!(render_pass_timestamps(&world, RuntimeStage::GpuUi).is_none());
    assert_eq!(world.resource::<GpuTimestamps>().slots.len(), SLOTS);
}

#[test]
fn frame_without_spans_releases_its_slot() {
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
    for _ in 0..SLOTS * 2 {
        timestamps.begin(|_| unreachable!("no frame was submitted"));
        finish_frame(&mut timestamps, &device, &queue);
        assert!(timestamps.ring.oldest_in_flight().is_none());
    }
}

#[test]
fn ui_pass_categories_are_opt_in_and_do_not_duplicate_parent_spans() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    for enabled in [false, true] {
        let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
        timestamps.ui_categories = enabled;
        timestamps.begin(|_| unreachable!("first frame has no readback"));
        let mut world = World::new();
        world.insert_resource(timestamps);
        let (_, buffers) = record(&mut world, &device, |world, context| {
            timed(world, context, RuntimeStage::GpuUi, |_| {})
        });
        let parent_span = !cfg!(target_os = "macos") && !enabled;
        assert_eq!(buffers.is_empty(), !parent_span);
        let timestamps = world.resource::<GpuTimestamps>();
        assert_eq!(
            timestamps.frame.passes.load(Ordering::Relaxed),
            u32::from(parent_span)
        );
        for category in RuntimeStage::GPU_UI {
            let writes = ui_pass_timestamps(&world, category);
            if cfg!(target_os = "macos") || enabled {
                let writes = writes.expect("owned UI passes use timestamp-capable adapters");
                let span = writes.beginning_of_pass_write_index.unwrap() / 2;
                let stage = timestamps.frame.stages[span as usize].load(Ordering::Relaxed);
                assert_eq!(
                    RuntimeStage::ALL[stage as usize],
                    if enabled {
                        category
                    } else {
                        RuntimeStage::GpuUi
                    }
                );
                assert_eq!(writes.end_of_pass_write_index, Some(span * 2 + 1));
            } else {
                assert!(writes.is_none(), "ordinary rendering retains graph markers");
            }
        }
        assert_eq!(
            timestamps.frame.passes.load(Ordering::Relaxed),
            if cfg!(target_os = "macos") || enabled {
                RuntimeStage::GPU_UI.len() as u32
            } else {
                1
            }
        );
    }
}

/// Frames nothing inspects pass by pass open one whole-frame span, which the readback closes.
#[test]
fn unprofiled_frames_time_only_the_whole_frame() {
    if cfg!(target_os = "macos") {
        return;
    }
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let stages = [
        RuntimeStage::GpuOpaque,
        RuntimeStage::GpuTransparent,
        RuntimeStage::GpuBlit,
    ];
    for detail in [false, true] {
        let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
        timestamps.pass_detail = detail;
        timestamps.begin(|_| unreachable!("first frame has no readback"));
        let mut world = World::new();
        world.insert_resource(timestamps);
        let (_, buffers) = record(&mut world, &device, |world, context| {
            for stage in stages {
                timed(world, context, stage, |_| {});
            }
        });
        queue.submit(buffers);
        let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
        finish_frame(&mut timestamps, &device, &queue);
        let slot = &timestamps.slots[timestamps.ring.oldest_in_flight().unwrap()];
        let recorded = &slot.stages[..slot.passes as usize];
        if detail {
            assert_eq!(recorded, stages);
        } else {
            assert_eq!(recorded, [RuntimeStage::GpuFrame]);
        }
    }
}

/// The stage profiler or an explicit request, such as the F3 overlay's, times every pass.
#[test]
fn profiling_or_a_detail_request_times_every_pass() {
    use bevy::ecs::system::RunSystemOnce;
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    for (profiling, requested, detail) in [
        (false, false, false),
        (true, false, true),
        (false, true, true),
    ] {
        let mut world = World::new();
        world.insert_resource(GpuTimestamps::new(&device, &queue, false).unwrap());
        world.insert_resource(RuntimeStageProfiler::new(profiling));
        world.insert_resource(DetailedGpuTiming(requested));
        world.run_system_once(begin_gpu_frame).unwrap();
        assert_eq!(world.resource::<GpuTimestamps>().pass_detail, detail);
    }
}

/// The frame's own graph closes and copies its spans; a frame whose graph never resolved them
/// gives its slot back instead of mapping stale storage.
#[test]
fn spans_are_read_back_only_after_the_frame_graph_resolves_them() {
    if cfg!(target_os = "macos") {
        return;
    }
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
    timestamps.begin(|_| unreachable!("first frame has no readback"));
    let mut world = World::new();
    world.insert_resource(timestamps);
    let (_, buffers) = record(&mut world, &device, |world, context| {
        timed(world, context, RuntimeStage::GpuOpaque, |_| {})
    });
    queue.submit(buffers);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    timestamps.request_readback(&queue);
    assert!(
        timestamps.ring.oldest_in_flight().is_none(),
        "an unresolved frame maps nothing"
    );

    timestamps.begin(|_| unreachable!("nothing was read back"));
    world.insert_resource(timestamps);
    let (_, mut buffers) = record(&mut world, &device, |world, context| {
        timed(world, context, RuntimeStage::GpuOpaque, |_| {})
    });
    assert_ne!(
        world
            .resource::<GpuTimestamps>()
            .frame
            .open_frame
            .load(Ordering::Relaxed),
        NO_SPAN
    );
    buffers.extend(run_readback(&mut world, &device));
    assert_eq!(
        world
            .resource::<GpuTimestamps>()
            .frame
            .open_frame
            .load(Ordering::Relaxed),
        NO_SPAN
    );
    queue.submit(buffers);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    timestamps.request_readback(&queue);
    let slot = &timestamps.slots[timestamps.ring.oldest_in_flight().unwrap()];
    assert_eq!(
        slot.stages[..slot.passes as usize],
        [RuntimeStage::GpuFrame]
    );
}

#[test]
fn readback_includes_every_camera_in_submission_order() {
    use bevy::{
        core_pipeline::schedule::camera_driver,
        render::{
            camera::{SortedCamera, SortedCameras},
            renderer::{RenderGraph, ViewQuery},
        },
    };
    #[derive(Component)]
    struct CameraStage(RuntimeStage);
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let mut timestamps = GpuTimestamps::new(&device, &queue, true).unwrap();
    timestamps.begin(|_| unreachable!("first frame"));
    let mut world = World::new();
    world.insert_resource(device.clone());
    world.insert_resource(queue.clone());
    world.insert_resource(timestamps);
    world.init_resource::<PendingCommandBuffers>();
    let mut cameras = Vec::new();
    for (order, stage) in [RuntimeStage::GpuOpaque, RuntimeStage::GpuTransparent]
        .into_iter()
        .enumerate()
    {
        let camera = crate::render_test_support::camera(false);
        let output_mode = camera.output_mode;
        let entity = world.spawn((camera, CameraStage(stage))).id();
        cameras.push(SortedCamera {
            entity,
            order: order as isize,
            target: None,
            hdr: false,
            output_mode,
        });
    }
    world.insert_resource(SortedCameras(cameras));
    let mut views = bevy::ecs::schedule::Schedule::new(Core3d);
    views.add_systems(
        |world: &World, stage: ViewQuery<&CameraStage>, mut context: RenderContext| {
            let span = world
                .resource::<GpuTimestamps>()
                .open_pass(stage.into_inner().0)
                .unwrap();
            mark(context.command_encoder(), span.queries, span.begin);
            mark(context.command_encoder(), span.queries, span.begin + 1);
        },
    );
    world.add_schedule(views);
    let mut root = bevy::ecs::schedule::Schedule::new(RenderGraph);
    root.add_systems(camera_driver);
    world.add_schedule(root);
    install_readback(&mut world);
    world.run_schedule(RenderGraph);
    queue.submit(world.resource_mut::<PendingCommandBuffers>().take());
    let mut timestamps = world.resource_mut::<GpuTimestamps>();
    timestamps.request_readback(&queue);
    let slot = &timestamps.slots[timestamps.ring.oldest_in_flight().unwrap()];
    assert_eq!(slot.passes, 2);
    assert_eq!(
        slot.stages[..2],
        [RuntimeStage::GpuOpaque, RuntimeStage::GpuTransparent]
    );
}

#[test]
fn draw_overflow_drops_categories_but_keeps_passes() {
    let features = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    let (device, queue) = noop_device(features);
    assert!(
        !GpuTimestamps::new(&device, &queue, false)
            .unwrap()
            .draw_spans,
        "per-draw spans stay off without aggregate profiling"
    );
    let mut timestamps = GpuTimestamps::new(&device, &queue, true).unwrap();
    timestamps.begin(|_| {});
    assert!(timestamps.open_pass(RuntimeStage::GpuOpaque).is_some());
    let draws = (0..=DRAW_SPANS)
        .filter(|_| timestamps.open_draw(RuntimeStage::GpuActors).is_some())
        .count();
    assert_eq!(draws, DRAW_SPANS as usize);
    finish_frame(&mut timestamps, &device, &queue);
    let slot = timestamps.ring.oldest_in_flight().unwrap();
    assert_eq!(
        (timestamps.slots[slot].passes, timestamps.slots[slot].draws),
        (1, 0)
    );
    assert_eq!(timestamps.slots[slot].stages[0], RuntimeStage::GpuOpaque);
}

/// Pipelined rendering removes the render app during cleanup, before later plugins' hooks.
#[test]
fn timing_starts_inside_the_render_app_under_pipelined_rendering() {
    use bevy::render::pipelined_rendering::{PipelinedRenderingPlugin, RenderAppChannels};
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let mut render_app = bevy::app::SubApp::new();
    render_app
        .add_schedule(bevy::ecs::schedule::Schedule::new(RenderStartup))
        .insert_resource(device)
        .insert_resource(queue);
    let mut app = App::new();
    app.insert_sub_app(RenderApp, render_app);
    app.insert_resource(RuntimeStageProfiler::new(false));
    app.add_plugins((PipelinedRenderingPlugin, GpuTimingPlugin));
    app.finish();
    app.cleanup();
    assert!(app.get_sub_app(RenderApp).is_none());
    let mut channels = app
        .world_mut()
        .remove_resource::<RenderAppChannels>()
        .unwrap();
    let mut render_app = bevy::tasks::block_on(channels.recv()).unwrap();
    render_app.world_mut().run_schedule(RenderStartup);
    assert!(render_app.world().contains_resource::<GpuTimestamps>());
}
