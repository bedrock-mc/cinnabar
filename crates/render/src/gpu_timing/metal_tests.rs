use super::*;
use render_scene::RuntimeStage;

/// Transfer workload for the frame completion regression.
const FRAME_WORK_BYTES: u64 = 4 * 1024 * 1024;

/// Uses the hardware backend because the NOOP backend never writes timestamp values.
fn metal_device() -> Option<(RenderDevice, RenderQueue)> {
    metal_device_with_features(wgpu::Features::TIMESTAMP_QUERY)
}

/// Requests only the features needed by the hardware regression being run.
pub(super) fn metal_device_with_features(
    features: wgpu::Features,
) -> Option<(RenderDevice, RenderQueue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Ok(adapter) =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("missing fixture: Metal adapter for GPU timestamp regression");
        return None;
    };
    if !adapter.features().contains(features) {
        eprintln!("missing fixture: Metal timestamp queries for GPU timestamp regression");
        return None;
    }
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    }))
    .expect("Metal timestamp device");
    Some((RenderDevice::from(device), RenderQueue::new(queue)))
}

#[derive(Resource)]
struct FrameWork(bevy::render::render_resource::Buffer);

/// Encodes real transfer work for the completion-latency regression.
fn clear_frame_work(work: Res<FrameWork>, mut context: RenderContext) {
    context.command_encoder().clear_buffer(&work.0, 0, None);
}

/// Uses the graph's existing flush parameter to submit every pending buffer together.
fn submit_frame_work(mut commands: bevy::render::renderer::FlushCommands) {
    commands.flush();
}

#[test]
fn metal_completion_latency_reports_without_timestamp_queries_or_extra_graph_submissions() {
    use bevy::render::renderer::{PendingCommandBuffers, RenderGraph, RenderGraphSystems};
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let Some((device, queue)) = metal_device_with_features(wgpu::Features::empty()) else {
        return;
    };
    let work = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("frame completion regression"),
        size: FRAME_WORK_BYTES,
        usage: wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let profiler = RuntimeStageProfiler::for_gameplay(true, None);
    // Callback latency must remain informational even with a tiny frame budget.
    profiler.set_frame_interval(std::time::Duration::from_nanos(1));
    let mut render_app = bevy::app::SubApp::new();
    render_app
        .add_schedule(bevy::ecs::schedule::Schedule::new(RenderStartup))
        .add_schedule(RenderGraph::base_schedule())
        .init_resource::<PendingCommandBuffers>()
        .insert_resource(device.clone())
        .insert_resource(queue.clone())
        .insert_resource(FrameWork(work))
        .add_systems(
            RenderGraph,
            (
                clear_frame_work
                    .in_set(RenderGraphSystems::Render)
                    .before(submit_frame_work),
                submit_frame_work.in_set(RenderGraphSystems::Submit),
            ),
        );
    let mut app = App::new();
    app.insert_resource(profiler.clone())
        .insert_sub_app(RenderApp, render_app);
    app.add_plugins(GpuTimingPlugin);
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.run_schedule(RenderStartup);
    assert!(!world.contains_resource::<GpuTimestamps>());
    assert_eq!(
        profiler
            .take_snapshot_if_due(std::time::Duration::ZERO)
            .unwrap()
            .samples[RuntimeStage::GpuFrame as usize]
            .count,
        0
    );
    let before = crate::device_poll::submission_marker(&queue);
    world.run_schedule(RenderGraph);
    let after = crate::device_poll::submission_marker(&queue);
    assert_eq!(
        after - before,
        2,
        "one graph submit plus the test's trailing marker"
    );
    world.run_schedule(Render);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    world.run_schedule(RenderGraph);
    let snapshot = profiler
        .take_snapshot_if_due(std::time::Duration::ZERO)
        .unwrap();
    let frame = &snapshot.samples[RuntimeStage::GpuFrameCompletionLatency as usize];
    assert_eq!(
        frame.count, 1,
        "one labelled latency is published after completion"
    );
    assert!(frame.total > std::time::Duration::ZERO);
    assert_eq!(snapshot.samples[RuntimeStage::GpuFrame as usize].count, 0);
    assert_eq!(profiler.slow_frame_counts().unwrap().gpu, 0);
    assert_eq!(profiler.slow_frame_counts().unwrap().slow, 0);
    eprintln!(
        "completion latency coverage: 0 -> {} samples; graph submissions: 1 -> 1; latency: {:?}",
        frame.count, frame.total
    );
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
}

/// Draws one triangle while recording an owned render-pass timestamp pair.
fn draw_timestamp_triangle(
    encoder: &mut wgpu::CommandEncoder,
    world: &World,
    view: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    stage: RuntimeStage,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("owned timestamp regression pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: render_pass_timestamps(world, stage),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.draw(0..3, 0..1);
}

#[test]
fn metal_deferred_pass_markers_emit_readable_timestamps() {
    let Some((device, queue)) = metal_device() else {
        return;
    };
    let raw = device.wgpu_device();
    let shader = raw.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("timestamp regression draw"),
        source: wgpu::ShaderSource::Wgsl(
            "@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
                 let xy = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
                 return vec4f(xy * 2.0 - vec2f(1.0), 0.0, 1.0);
             }
             @fragment fn fragment() -> @location(0) vec4f { return vec4f(1.0); }"
                .into(),
        ),
    });
    let pipeline = raw.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("timestamp regression draw"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let texture = raw.create_texture(&wgpu::TextureDescriptor {
        label: Some("timestamp regression target"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
    timestamps.begin(|_| unreachable!("first frame has no readback"));
    let mut world = World::new();
    world.insert_resource(timestamps);
    let pool = bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (_, mut buffers) =
        crate::render_test_support::record(&mut world, &device, |render_world, context| {
            let generated = pool.scope(|scope| {
                scope.spawn(async {
                    let mut encoder = device.create_command_encoder(&Default::default());
                    draw_timestamp_triangle(
                        &mut encoder,
                        render_world,
                        &view,
                        &pipeline,
                        RuntimeStage::GpuOpaque,
                    );
                    encoder.finish()
                });
            });
            for buffer in generated {
                context.add_command_buffer(buffer);
            }
        });
    buffers.extend(super::tests::run_readback(&mut world, &device));
    queue.submit(buffers);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    timestamps.request_readback(&queue);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    timestamps.begin(|_| panic!("completed samples still need a readback copy"));
    world.insert_resource(timestamps);
    let (_, mut buffers) =
        crate::render_test_support::record(&mut world, &device, |world, context| {
            draw_timestamp_triangle(
                context.command_encoder(),
                world,
                &view,
                &pipeline,
                RuntimeStage::GpuUi,
            );
        });
    buffers.extend(super::tests::run_readback(&mut world, &device));
    queue.submit(buffers);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    timestamps.request_readback(&queue);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let index = timestamps.ring.oldest_in_flight().unwrap();
    assert_eq!(timestamps.slots[index].passes, 1);
    assert_eq!(
        timestamps.slots[index].state.load(Ordering::Acquire),
        MAPPED
    );
    let mapped = timestamps.slots[index]
        .buffer
        .slice(..)
        .get_mapped_range()
        .expect("readback buffer is mapped");
    let values: Vec<_> = mapped[..16]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|bytes| u64::from_le_bytes(*bytes))
        .collect();
    eprintln!("Metal owned pass timestamp values: {values:?}");
    assert!(
        values[0] != 0 && values[1] >= values[0] && values[1] != u64::MAX,
        "owned pass timestamps must be valid"
    );
    drop(mapped);
    let mut frames = Vec::new();
    timestamps.begin(|frame| frames.push(*frame));
    assert_eq!(frames.len(), 1);
    assert_eq!(
        frames[0].get(RuntimeStage::GpuOpaque),
        Some(std::time::Duration::from_secs_f64(
            (values[1] - values[0]) as f64 * f64::from(queue.get_timestamp_period()) * 1e-9
        ))
    );
    assert_eq!(frames[0].get(RuntimeStage::GpuUi), None);
    assert_eq!(frames[0].get(RuntimeStage::GpuFrame), None);
}

/// Each depth reduction claims one existing pass span and keeps its target across owner changes.
#[test]
fn metal_depth_resolve_uses_owned_pass_queries_without_extra_passes() {
    use crate::scene_sampling::ResolvedDepth;
    use bevy::render::{texture::CachedTexture, view::ViewDepthStencilTexture};

    let Some((device, queue)) = metal_device() else {
        return;
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("timed multisample depth fixture"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 4,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth = ViewDepthStencilTexture::new(
        CachedTexture {
            default_view: texture.create_view(&Default::default()),
            texture,
        },
        Some(0.0),
        None,
    );
    let mut resolved = ResolvedDepth::new(&device, &depth, RuntimeStage::GpuShadows);
    let retained = resolved.view.id();
    let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
    timestamps.begin(|_| unreachable!("first frame has no readback"));
    let mut world = World::new();
    world.insert_resource(timestamps);
    let (_, buffers) = crate::render_test_support::record(&mut world, &device, |world, context| {
        resolved.draw(context, world, None);
        let next = RuntimeStage::GPU_MOD_PASSES[0];
        resolved.set_stage(next);
        resolved.draw(context, world, None);
        let timestamps = world.resource::<GpuTimestamps>();
        assert_eq!(timestamps.frame.passes.load(Ordering::Relaxed), 2);
        assert_eq!(timestamps.frame.draws.load(Ordering::Relaxed), 0);
        assert_eq!(
            timestamps.frame.stages[0].load(Ordering::Relaxed),
            RuntimeStage::GpuShadows as u8
        );
        assert_eq!(
            timestamps.frame.stages[1].load(Ordering::Relaxed),
            next as u8
        );
        assert_eq!(resolved.view.id(), retained);
        bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    });
    queue.submit(buffers);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
}
