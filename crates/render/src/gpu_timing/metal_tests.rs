use super::*;
use render_scene::RuntimeStage;

/// Transfer workload for the frame completion regression.
pub(super) const FRAME_WORK_BYTES: u64 = 4 * 1024 * 1024;

/// Uses the hardware backend because the NOOP backend never writes timestamp values.
fn metal_device() -> Option<(RenderDevice, RenderQueue)> {
    metal_device_with_features(wgpu::Features::TIMESTAMP_QUERY)
}

/// Requests only the features needed by the hardware regression being run.
#[allow(
    clippy::print_stderr,
    reason = "explains on stderr why the regression skips when the Metal timestamp fixture is absent"
)]
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
fn metal_frame_markers_report_inside_the_graph_submission_without_timestamp_queries() {
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
    // Only execution may count against even a tiny frame budget, never callback latency.
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
    let latency = &snapshot.samples[RuntimeStage::GpuFrameCompletionLatency as usize];
    let execution = &snapshot.samples[RuntimeStage::GpuFrame as usize];
    assert_eq!(
        latency.count, 1,
        "one latency is published after completion"
    );
    assert_eq!(execution.count, 1, "one execution span is published");
    assert!(execution.total > std::time::Duration::ZERO);
    assert!(execution.total <= latency.total);
    assert_eq!(profiler.slow_frame_counts().unwrap().gpu, 1);
    assert_eq!(profiler.slow_frame_counts().unwrap().slow, 1);
    eprintln!(
        "Metal gpu_frame samples: 0 -> {}; graph submissions: 1 -> 1; execution {:?}, latency {:?}",
        execution.count, execution.total, latency.total
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
    timestamps.begin(0, |_, _, _| unreachable!("first frame has no readback"));
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
    timestamps.begin(0, |_, _, _| {
        panic!("completed samples still need a readback copy")
    });
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
    timestamps.begin(0, |_, frame, _| frames.push(*frame));
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
    timestamps.begin(0, |_, _, _| unreachable!("first frame has no readback"));
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

/// Fragment work whose own pass timestamps bound the frame span from below.
#[derive(Resource)]
struct SpinWork {
    pipeline: wgpu::RenderPipeline,
    view: wgpu::TextureView,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
}

impl SpinWork {
    const SIZE: u32 = 1024;

    fn new(device: &RenderDevice) -> Self {
        let raw = device.wgpu_device();
        let shader = raw.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frame span regression"),
            source: wgpu::ShaderSource::Wgsl(
                "@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
                     let xy = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
                     return vec4f(xy * 2.0 - vec2f(1.0), 0.0, 1.0);
                 }
                 @fragment fn fragment(@builtin(position) p: vec4f) -> @location(0) vec4f {
                     var x = p.x * p.y;
                     for (var i = 0u; i < 4000u; i++) { x = fma(x, 1.0001, 0.5); }
                     return vec4f(fract(x));
                 }"
                .into(),
            ),
        });
        let pipeline = raw.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("frame span regression"),
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
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let texture = raw.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame span regression target"),
            size: wgpu::Extent3d {
                width: Self::SIZE,
                height: Self::SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let buffer = |usage| {
            raw.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame span regression timestamps"),
                size: 2 * TIMESTAMP_BYTES,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            pipeline,
            view: texture.create_view(&Default::default()),
            queries: raw.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("frame span regression"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            readback: buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
        }
    }

    /// Resolves completed pass timestamps separately so resolution cannot overtake sampling.
    fn pass_duration(&self, device: &RenderDevice, queue: &RenderQueue) -> std::time::Duration {
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("completed frame span regression timestamps"),
        });
        encoder.resolve_query_set(&self.queries, 0..2, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, 2 * TIMESTAMP_BYTES);
        queue.submit([encoder.finish()]);
        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let ticks: Vec<u64> = slice.get_mapped_range().unwrap()[..16]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|bytes| u64::from_le_bytes(*bytes))
            .collect();
        self.readback.unmap();
        assert!(ticks[0] != 0 && ticks[1] > ticks[0], "pass ticks {ticks:?}");
        render_scene::gpu_times::ticks_to_duration(
            ticks[1] - ticks[0],
            queue.get_timestamp_period(),
        )
    }
}

/// Draws the timed fragment work, leaving its timestamps for completed-frame readback.
fn draw_spin_work(work: Res<SpinWork>, mut context: RenderContext) {
    let encoder = context.command_encoder();
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frame span regression"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &work.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
                query_set: &work.queries,
                beginning_of_pass_write_index: Some(0),
                end_of_pass_write_index: Some(1),
            }),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&work.pipeline);
        pass.draw(0..3, 0..1);
    }
}

/// The marker span covers the frame's measured pass and stays below the CPU-observed latency.
#[test]
fn metal_frame_span_brackets_the_graph_work_it_submits() {
    use bevy::render::renderer::{PendingCommandBuffers, RenderGraph, RenderGraphSystems};
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let Some((device, queue)) = metal_device() else {
        return;
    };
    let profiler = RuntimeStageProfiler::for_gameplay(true, None);
    let mut render_app = bevy::app::SubApp::new();
    render_app
        .add_schedule(bevy::ecs::schedule::Schedule::new(RenderStartup))
        .add_schedule(RenderGraph::base_schedule())
        .init_resource::<PendingCommandBuffers>()
        .insert_resource(device.clone())
        .insert_resource(queue.clone())
        .insert_resource(profiler.clone())
        .insert_resource(SpinWork::new(&device))
        .add_systems(
            RenderGraph,
            (
                draw_spin_work.in_set(RenderGraphSystems::Render),
                submit_frame_work.in_set(RenderGraphSystems::Submit),
            ),
        );
    crate::device_poll::install(&mut render_app);
    super::metal_frame::install(&mut render_app);
    let world = render_app.world_mut();
    world.run_schedule(RenderStartup);
    let mut previous = None;
    for frame in 0..4 {
        // Each frame publishes the previous one, whose markers settled during the wait below.
        world.run_schedule(RenderGraph);
        let snapshot = profiler
            .take_snapshot_if_due(std::time::Duration::ZERO)
            .unwrap();
        let execution = &snapshot.samples[RuntimeStage::GpuFrame as usize];
        let latency = &snapshot.samples[RuntimeStage::GpuFrameCompletionLatency as usize];
        if let Some(pass) = previous {
            eprintln!(
                "frame {frame}: pass {pass:?}, gpu_frame {:?}, completion latency {:?}",
                execution.total, latency.total
            );
            assert_eq!((execution.count, latency.count), (1, 1));
            assert!(pass <= execution.total, "the span covers the pass");
            assert!(execution.total <= latency.total, "GPU time fits in latency");
        } else {
            assert_eq!((execution.count, latency.count), (0, 0));
        }
        world.run_schedule(Render);
        previous = Some(world.resource::<SpinWork>().pass_duration(&device, &queue));
    }
}
