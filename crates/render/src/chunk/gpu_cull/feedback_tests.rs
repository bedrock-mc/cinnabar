use super::super::model::DECISION_DRAWS;
use super::*;

/// A decision buffer whose identity also identifies the cull's visibility-history allocation.
fn source(device: &RenderDevice, slots: u32) -> wgpu::Buffer {
    device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("rejection feedback test decisions"),
        size: u64::from(slots) * 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

/// Records a completed late dispatch; only reserved samples add a copy to the encoder.
fn encoded(feedback: &GpuCullFeedback, device: &RenderDevice, decisions: &wgpu::Buffer) {
    let mut encoder = device
        .wgpu_device()
        .create_command_encoder(&Default::default());
    feedback.encode(&mut encoder, decisions);
    drop(encoder.finish());
}

/// The selected view of a validating NOOP feedback fixture.
fn fixture() -> (RenderDevice, wgpu::Queue, Entity, GpuCullFeedback) {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    (
        RenderDevice::from(device),
        queue,
        world.spawn_empty().id(),
        GpuCullFeedback::default(),
    )
}

#[test]
fn cold_gpu_frames_do_not_record_rejection_feedback() {
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let mut world = World::new();
    let view = world.spawn_empty().id();
    let mut feedback = GpuCullFeedback::default();
    assert!(feedback.select(Some(view)));
    feedback.prepare(&device, view, 2, &source(&device, 2));
    assert!(
        feedback.sample.is_none(),
        "a first GPU frame has no terrain depth history"
    );
}

#[test]
fn isolated_probes_warm_history_before_recording_their_second_frame() {
    let (device, _queue, view, mut feedback) = fixture();
    let decisions = source(&device, 2);
    assert!(feedback.select(Some(view)));
    feedback
        .admission
        .observe(view, feedback.admission.frame, 128, 2);
    feedback.admission.frame += PROBE_INTERVAL;
    assert!(feedback.select(Some(view)));
    feedback.prepare(&device, view, 2, &decisions);
    assert!(feedback.sample.is_none());
    encoded(&feedback, &device, &decisions);
    assert!(
        feedback.select(Some(view)),
        "a probe needs a consecutive GPU follow-up"
    );
    feedback.prepare(&device, view, 2, &decisions);
    assert_eq!(
        feedback
            .sample
            .as_ref()
            .unwrap()
            .state
            .load(Ordering::Acquire),
        RECORDING
    );
}

#[test]
fn replaced_storage_and_changed_views_cannot_sample_old_history() {
    let (device, _queue, view, mut feedback) = fixture();
    let initial = source(&device, 2);
    assert!(feedback.select(Some(view)));
    feedback.prepare(&device, view, 2, &initial);
    encoded(&feedback, &device, &initial);

    let grown = source(&device, 64);
    assert!(feedback.select(Some(view)));
    feedback.prepare(&device, view, 64, &grown);
    assert!(
        feedback.sample.is_none(),
        "new GPU storage starts with empty history"
    );
    encoded(&feedback, &device, &grown);
    assert!(feedback.select(Some(view)));
    feedback.prepare(&device, view, 64, &grown);
    assert_eq!(
        feedback
            .sample
            .as_ref()
            .unwrap()
            .state
            .load(Ordering::Acquire),
        RECORDING
    );
    feedback.finish_frame();

    let mut world = World::new();
    world.spawn_empty();
    let other = world.spawn_empty().id();
    assert_ne!(other, view);
    assert!(feedback.select(Some(other)));
    feedback.prepare(&device, other, 64, &grown);
    assert_eq!(
        feedback
            .sample
            .as_ref()
            .unwrap()
            .state
            .load(Ordering::Acquire),
        IDLE
    );
}

#[test]
fn a_reserved_sample_without_a_late_pass_retries_without_advancing_the_cadence() {
    let (device, _queue, view, mut feedback) = fixture();
    let decisions = source(&device, 2);
    feedback.select(Some(view));
    feedback.prepare(&device, view, 2, &decisions);
    encoded(&feedback, &device, &decisions);
    feedback.select(Some(view));
    feedback.prepare(&device, view, 2, &decisions);
    assert_eq!(
        feedback
            .sample
            .as_ref()
            .unwrap()
            .state
            .load(Ordering::Acquire),
        RECORDING
    );
    feedback.finish_frame();
    assert_eq!(feedback.next_sample, 0);
    assert_eq!(
        feedback
            .sample
            .as_ref()
            .unwrap()
            .state
            .load(Ordering::Acquire),
        IDLE
    );
    assert!(feedback.select(Some(view)));
    feedback.prepare(&device, view, 2, &decisions);
    assert_eq!(
        feedback
            .sample
            .as_ref()
            .unwrap()
            .state
            .load(Ordering::Acquire),
        IDLE,
        "the missing late pass must warm history again"
    );
    encoded(&feedback, &device, &decisions);
    assert!(feedback.select(Some(view)));
    feedback.prepare(&device, view, 2, &decisions);
    assert_eq!(
        feedback
            .sample
            .as_ref()
            .unwrap()
            .state
            .load(Ordering::Acquire),
        RECORDING
    );
}

#[test]
fn pending_and_mapped_samples_keep_their_buffers_until_released() {
    for state in [PENDING, MAPPED] {
        let (device, _queue, view, mut feedback) = fixture();
        let initial = source(&device, 2);
        feedback.select(Some(view));
        feedback.prepare(&device, view, 2, &initial);
        encoded(&feedback, &device, &initial);
        feedback.select(Some(view));
        feedback.prepare(&device, view, 2, &initial);
        let sample = feedback.sample.as_ref().unwrap();
        let buffer = sample.buffer.clone();
        sample.state.store(state, Ordering::Release);
        let grown = source(&device, 64);
        for _ in 0..2 {
            feedback.admission.frame += 1;
            feedback.prepare(&device, view, 64, &grown);
            encoded(&feedback, &device, &grown);
        }
        let sample = feedback.sample.as_ref().unwrap();
        assert_eq!(sample.buffer, buffer);
        assert_eq!(sample.slots, 2);
        sample.state.store(IDLE, Ordering::Release);
        feedback.admission.frame += 1;
        feedback.prepare(&device, view, 64, &grown);
        let replacement = feedback.sample.as_ref().unwrap();
        assert_ne!(replacement.buffer, buffer);
        assert_eq!(replacement.slots, 64);
    }
}

#[test]
fn feedback_counts_candidates_independently_of_early_and_late_draws() {
    assert_eq!(
        rejection_counts(&[
            DECISION_CANDIDATE | DECISION_HIDDEN,
            DECISION_CANDIDATE | DECISION_DRAWS,
            DECISION_CANDIDATE,
            0,
        ]),
        (3, 1)
    );
}

#[test]
fn low_rejection_skips_work_until_a_fresh_probe_and_view_changes_reset_it() {
    let mut world = World::new();
    let a = world.spawn_empty().id();
    let b = world.spawn_empty().id();
    let mut admission = Admission::default();
    assert!(admission.select(Some(a)));
    admission.observe(a, 1, 128, 2);
    assert!(admission.select(Some(a)));
    assert!(!admission.select(Some(a)));
    admission.frame += PROBE_INTERVAL;
    assert!(admission.select(Some(a)));
    admission.observe(a, admission.frame, 128, 100);
    assert!(admission.select(Some(a)));
    assert!(!admission.select(None));
    assert!(admission.select(Some(b)));
    admission.observe(a, admission.frame, 128, 0);
    assert!(admission.select(Some(b)));
    assert!(admission.select(Some(a)));
    admission.observe(a, 1, 128, 0);
    assert!(
        admission.select(Some(a)),
        "feedback from a previous view selection must be ignored"
    );
}

/// Periodic occlusion probes retain both views' CPU commands and their shared buffer layout.
#[test]
fn adaptive_gpu_probes_keep_cpu_batches_without_rebuilding_or_sorting() {
    use super::super::GpuCullFrame;
    use super::super::app_tests::{
        KEYS, camera_transform, chunk_app, frame, insert_meshes, noop_render_plugin, spawn_camera,
    };
    use crate::chunk::pipeline::batches::RetainedTerrainBatches;
    use crate::chunk::*;
    let stats = |app: &App| {
        app.sub_app(RenderApp)
            .world()
            .resource::<RetainedTerrainBatches>()
            .stats
    };

    #[derive(Resource, Default)]
    struct Paths {
        gpu: usize,
        cpu: usize,
    }
    let (mut app, _) = chunk_app(
        noop_render_plugin(
            WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT | WgpuFeatures::INDIRECT_FIRST_INSTANCE,
        ),
        Msaa::Off,
        camera_transform(),
    );
    spawn_camera(&mut app, camera_transform(), 1);
    app.sub_app_mut(RenderApp)
        .init_resource::<Paths>()
        .add_systems(
            Render,
            (|frame: Res<GpuCullFrame>, mut paths: ResMut<Paths>| {
                if frame.view.is_some() {
                    paths.gpu += 1;
                } else {
                    paths.cpu += 1;
                }
            })
            .in_set(RenderSystems::Queue)
            .after(crate::chunk::draw::queue_chunks),
        );
    insert_meshes(&mut app, &KEYS);
    for _ in 0..8 {
        frame(&mut app);
    }
    {
        let mut feedback = app
            .sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<GpuCullFeedback>();
        let view = feedback.admission.view.unwrap();
        let frame = feedback.admission.frame;
        feedback.admission.observe(view, frame, 128, 2);
    }
    for _ in 0..8 {
        frame(&mut app);
    }
    let settled = stats(&app);
    let world = app.sub_app(RenderApp).world();
    let gpu_frames = world.resource::<Paths>().gpu;
    let cpu_frames = world.resource::<Paths>().cpu;
    let buffer = world.resource::<ChunkGpuArena>().indirect_buffer.id();
    let commands = world
        .resource::<ChunkGpuArena>()
        .uploaded_indirect_bytes
        .clone();
    assert!(!commands.is_empty());
    let mut samples = Vec::new();
    let initial_sample = world
        .resource::<GpuCullFeedback>()
        .sample
        .as_ref()
        .unwrap()
        .frame;
    for _ in 0..2 * PROBE_INTERVAL {
        frame(&mut app);
        let world = app.sub_app(RenderApp).world();
        assert_eq!(
            world
                .resource::<pipeline::solid::ChunkSolidIndirectBatches>()
                .0
                .len(),
            2
        );
        assert_eq!(world.resource::<ChunkIndirectBatches>().0.len(), 2);
        assert_eq!(world.resource::<ChunkModelIndirectBatches>().0.len(), 2);
        assert_eq!(
            world.resource::<ChunkDepthLiquidIndirectBatches>().0.len(),
            2
        );
        let sample_frame = world
            .resource::<GpuCullFeedback>()
            .sample
            .as_ref()
            .unwrap()
            .frame;
        if sample_frame != initial_sample && samples.last() != Some(&sample_frame) {
            samples.push(sample_frame);
        }
        let current = stats(&app);
        assert_eq!(
            current.batches_rebuilt, settled.batches_rebuilt,
            "{current:?}"
        );
        assert_eq!(current.cube_sorts, settled.cube_sorts, "{current:?}");
        let arena = world.resource::<ChunkGpuArena>();
        assert_eq!(arena.indirect_buffer.id(), buffer);
        assert_eq!(arena.uploaded_indirect_bytes, commands);
    }
    assert_eq!(
        samples.len(),
        2,
        "each periodic probe must produce a warm sample: {samples:?}"
    );
    assert_eq!(samples[1] - samples[0], PROBE_INTERVAL);
    let paths = app.sub_app(RenderApp).world().resource::<Paths>();
    assert_eq!(
        paths.gpu - gpu_frames,
        4,
        "two probes need two GPU frames each"
    );
    assert!(
        paths.cpu > cpu_frames,
        "the view must return to CPU-planned draws"
    );
}
