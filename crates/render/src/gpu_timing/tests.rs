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
    timestamps.submit(&device, &queue);
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
        timestamps.submit(&device, &queue);
        assert!(timestamps.ring.oldest_in_flight().is_none());
    }
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
    timestamps.submit(&device, &queue);
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
