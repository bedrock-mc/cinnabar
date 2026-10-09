use super::*;
use bevy::render::{render_graph::RenderGraph, renderer::WgpuWrapper};

pub(super) fn noop_device(features: wgpu::Features) -> (RenderDevice, RenderQueue) {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..Default::default()
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

/// Ends a frame as the graph's readback node and the frame submissions do.
pub(super) fn finish_frame(
    timestamps: &mut GpuTimestamps,
    device: &RenderDevice,
    queue: &RenderQueue,
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    timestamps.encode_readback(&mut encoder);
    queue.submit([encoder.finish()]);
    timestamps.request_readback();
}

/// Runs the production readback node while deferred recording is still queued.
pub(super) fn run_readback_node<'w>(world: &'w World, context: &mut RenderContext<'w>) {
    let mut graph = RenderGraph::default();
    graph.add_node(ReadbackLabel, ReadbackNode);
    let state = graph.get_node_state(ReadbackLabel).unwrap();
    let mut outputs = [];
    let mut graph_context = RenderGraphContext::new(&graph, state, &[], &mut outputs);
    state.node.run(&mut graph_context, context, world).unwrap();
}

/// Deferred passes and draws belong to the same readback as synchronously recorded work.
#[test]
fn readback_includes_queries_allocated_by_deferred_recording() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let features = wgpu::Features::TIMESTAMP_QUERY
        | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
        | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let (device, queue) = noop_device(features);
    for synchronous in [false, true] {
        let mut timestamps = GpuTimestamps::new(&device, &queue, true).unwrap();
        timestamps.begin(|_| unreachable!("first frame has no readback"));
        let mut world = World::new();
        world.insert_resource(timestamps);
        let timestamps = world.resource::<GpuTimestamps>();
        let mut context = RenderContext::new(device.clone(), None);
        if synchronous {
            let span = timestamps.open_pass(RuntimeStage::GpuUi).unwrap();
            context
                .command_encoder()
                .write_timestamp(span.queries, span.begin);
            context
                .command_encoder()
                .write_timestamp(span.queries, span.begin + 1);
        }
        context.add_command_buffer_generation_task(move |device| {
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
        run_readback_node(&world, &mut context);
        queue.submit(context.finish().0);
        let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
        timestamps.request_readback();
        let slot = &timestamps.slots[timestamps.ring.oldest_in_flight().unwrap()];
        let expected = if synchronous {
            vec![RuntimeStage::GpuUi, RuntimeStage::GpuOpaque]
        } else {
            vec![RuntimeStage::GpuOpaque]
        };
        assert_eq!(slot.stages[..slot.passes as usize], expected);
        assert_eq!(slot.draws, 1);
        assert_eq!(slot.stages[PASS_SPANS as usize], RuntimeStage::GpuActors);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mut readbacks = 0;
        timestamps.begin(|_| readbacks += 1);
        assert_eq!(readbacks, 1);
    }
}

struct CountingNode(Arc<AtomicU32>);

impl Node for CountingNode {
    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        _: &mut RenderContext<'w>,
        _: &'w World,
    ) -> Result<(), NodeRunError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// A Core3d graph whose opaque node counts runs, wrapped as the plugin does.
fn timed_world() -> (World, Arc<AtomicU32>) {
    let runs = Arc::new(AtomicU32::new(0));
    let mut core = RenderGraph::default();
    core.add_node(Node3d::MainOpaquePass, CountingNode(runs.clone()));
    let mut graph = RenderGraph::default();
    graph.add_sub_graph(Core3d, core);
    let mut world = World::new();
    world.insert_resource(graph);
    wrap_timed_nodes(&mut world);
    (world, runs)
}

/// Runs the wrapped opaque node once and returns the command buffers it recorded.
fn run_opaque(world: &World, device: &RenderDevice) -> Vec<wgpu::CommandBuffer> {
    let graph = world
        .resource::<RenderGraph>()
        .get_sub_graph(Core3d)
        .unwrap();
    let state = graph.get_node_state(Node3d::MainOpaquePass).unwrap();
    assert!(state.node.downcast_ref::<TimedNode>().is_some());
    let mut outputs = [];
    let mut graph_context = RenderGraphContext::new(graph, state, &[], &mut outputs);
    let mut render_context = RenderContext::new(device.clone(), None);
    state
        .node
        .run(&mut graph_context, &mut render_context, world)
        .unwrap();
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    render_context.finish().0
}

#[test]
fn missing_timestamp_feature_runs_nodes_untimed() {
    let (device, queue) = noop_device(wgpu::Features::empty());
    assert!(GpuTimestamps::new(&device, &queue, true).is_none());
    let (world, runs) = timed_world();
    assert!(run_opaque(&world, &device).is_empty());
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

    let mut buffers = run_opaque(&world, &device);
    if cfg!(target_os = "macos") {
        let writes = render_pass_timestamps(&world, RuntimeStage::GpuOpaque).unwrap();
        let mut context = RenderContext::new(device.clone(), None);
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
        buffers.extend(context.finish().0);
    }
    assert_eq!(buffers.len(), 1, "the sampled work shares one encoder");
    queue.submit(buffers);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    assert_eq!(timestamps.frame.passes.load(Ordering::Relaxed), 1);
    finish_frame(&mut timestamps, &device, &queue);
    assert!(timestamps.ring.oldest_in_flight().is_some());
    assert!(frames.is_empty());

    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
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
    assert!(run_opaque(&world, &device).is_empty());
    let mut context = RenderContext::new(device.clone(), None);
    timed(&world, &mut context, RuntimeStage::GpuUi, |_| {});
    assert!(context.finish().0.is_empty());
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
        let mut context = RenderContext::new(device.clone(), None);
        timed(&world, &mut context, RuntimeStage::GpuUi, |_| {});
        let parent_span = !cfg!(target_os = "macos") && !enabled;
        let buffers = context.finish().0;
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
        let mut context = RenderContext::new(device.clone(), None);
        for stage in stages {
            timed(&world, &mut context, stage, |_| {});
        }
        queue.submit(context.finish().0);
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
    let mut context = RenderContext::new(device.clone(), None);
    timed(&world, &mut context, RuntimeStage::GpuOpaque, |_| {});
    queue.submit(context.finish().0);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    timestamps.request_readback();
    assert!(
        timestamps.ring.oldest_in_flight().is_none(),
        "an unresolved frame maps nothing"
    );

    timestamps.begin(|_| unreachable!("nothing was read back"));
    world.insert_resource(timestamps);
    let mut context = RenderContext::new(device.clone(), None);
    timed(&world, &mut context, RuntimeStage::GpuOpaque, |_| {});
    let readback = world.resource::<GpuTimestamps>();
    assert_ne!(readback.frame.open_frame.load(Ordering::Relaxed), NO_SPAN);
    run_readback_node(&world, &mut context);
    assert_eq!(readback.frame.open_frame.load(Ordering::Relaxed), NO_SPAN);
    let buffers = context.finish().0;
    assert_eq!(buffers.len(), 1, "ordinary frames retain one encoder");
    queue.submit(buffers);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    timestamps.request_readback();
    let slot = &timestamps.slots[timestamps.ring.oldest_in_flight().unwrap()];
    assert_eq!(
        slot.stages[..slot.passes as usize],
        [RuntimeStage::GpuFrame]
    );
}

#[test]
fn the_readback_node_runs_after_every_camera() {
    let mut graph = RenderGraph::default();
    graph.add_node(CameraDriverLabel, EmptyNode);
    let mut world = World::new();
    world.insert_resource(graph);
    add_readback_node(&mut world);
    let graph = world.resource::<RenderGraph>();
    let inputs: Vec<_> = graph
        .iter_node_inputs(ReadbackLabel)
        .unwrap()
        .map(|(_, node)| node.label)
        .collect();
    assert_eq!(inputs, [CameraDriverLabel.intern()]);
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
fn nodes_are_wrapped_inside_the_render_app_under_pipelined_rendering() {
    use bevy::render::pipelined_rendering::{PipelinedRenderingPlugin, RenderAppChannels};
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = noop_device(wgpu::Features::empty());
    let mut core = RenderGraph::default();
    core.add_node(Node3d::MainOpaquePass, CountingNode(Arc::default()));
    let mut graph = RenderGraph::default();
    graph.add_sub_graph(Core3d, core);
    let mut render_app = bevy::app::SubApp::new();
    render_app
        .add_schedule(bevy::ecs::schedule::Schedule::new(RenderStartup))
        .insert_resource(graph)
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
    let graph = render_app.world().resource::<RenderGraph>();
    let state = graph
        .get_sub_graph(Core3d)
        .unwrap()
        .get_node_state(Node3d::MainOpaquePass)
        .unwrap();
    assert!(state.node.downcast_ref::<TimedNode>().is_some());
}
