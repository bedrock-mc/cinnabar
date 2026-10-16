use super::*;
use render_scene::RuntimeStage;

#[test]
fn full_latency_ring_skips_without_publishing_pending_callbacks() {
    let now = Instant::now();
    let mut frames = MetalFrames::default();
    for _ in 0..SLOTS {
        frames.begin(|_| panic!("pending frame was published"));
        let slot = frames.recording.take().unwrap();
        frames.ring.submit(slot);
    }
    frames.begin(|_| panic!("pending frame was published"));
    assert!(frames.recording.is_none());
    assert_eq!(frames.ring.oldest_in_flight(), Some(0));
    let mut samples = Vec::new();
    // A later completion must not overtake the oldest pending callback.
    complete(
        &frames.slots[1].latency,
        now,
        now + Duration::from_millis(7),
    );
    frames.begin(|frame| samples.push(*frame));
    assert!(samples.is_empty());
    complete(
        &frames.slots[0].latency,
        now,
        now + Duration::from_millis(9),
    );
    frames.begin(|frame| samples.push(*frame));
    assert_eq!(samples.len(), 2);
    assert_eq!(
        samples[0].get(RuntimeStage::GpuFrameCompletionLatency),
        Some(Duration::from_millis(9))
    );
    assert_eq!(
        samples[1].get(RuntimeStage::GpuFrameCompletionLatency),
        Some(Duration::from_millis(7))
    );
    // Without markers there is no execution span to publish.
    assert_eq!(samples[0].get(RuntimeStage::GpuFrame), None);
    assert!(frames.recording.is_some());
    let slot = frames.recording.take().unwrap();
    frames.ring.release(slot);
    frames.begin(|_| panic!("completed sample was published twice"));
}

#[test]
fn completion_receipt_uses_the_callers_clock_and_preserves_zero() {
    let now = Instant::now();
    let elapsed = AtomicU64::new(PENDING_LATENCY);
    complete(&elapsed, now, now);
    assert_eq!(elapsed.load(Ordering::Acquire), 0);
    complete(&elapsed, now, now + Duration::from_millis(7));
    assert_eq!(elapsed.load(Ordering::Acquire), 7_000_000);
}

/// A frame waits for both markers, and a failed marker publishes latency without a span.
#[test]
fn marker_spans_settle_only_when_both_markers_finish() {
    use MarkerState::{Done, Failed, Pending};
    assert_eq!(settled(Pending, Done(9)), None);
    assert_eq!(settled(Done(4), Pending), None);
    assert_eq!(settled(Failed, Pending), None);
    assert_eq!(settled(Done(4), Done(9)), Some(Some((4, 9))));
    assert_eq!(settled(Failed, Done(9)), Some(None));
    assert_eq!(settled(Done(4), Failed), Some(None));
    assert_eq!(host_nanos(0.0), 0);
    assert_eq!(host_nanos(86_400.000_250), 86_400_000_250_000);
}

#[test]
fn gpu_completion_maps_host_time_without_accepting_unfinished_timestamps() {
    let now = Instant::now();
    assert_eq!(completed_at(20_000_000, now, 0), None);
    assert_eq!(completed_at(20_000_000, now, 21_000_000), None);
    assert_eq!(
        completed_at(20_000_000, now, 15_000_000),
        now.checked_sub(Duration::from_millis(5))
    );
}

/// Independent native work must finish before the closing marker's GPU timestamp.
#[test]
fn closing_marker_settles_earlier_independent_command_buffers() {
    use super::super::metal_tests::metal_device_with_features;
    use objc2_metal::{MTLBlitCommandEncoder, MTLCommandEncoder, MTLDevice, MTLResourceOptions};
    let Some((device, queue)) = metal_device_with_features(wgpu::Features::empty()) else {
        return;
    };
    // SAFETY: the test creates its own buffers; wgpu retains queue and device ownership.
    let metal = unsafe { device.wgpu_device().as_hal::<wgpu::hal::api::Metal>() }.unwrap();
    let work: Vec<_> = (0..16)
        .map(|_| {
            (
                metal
                    .raw_device()
                    .newBufferWithLength_options(
                        super::super::metal_tests::FRAME_WORK_BYTES as usize,
                        MTLResourceOptions::StorageModeShared,
                    )
                    .unwrap(),
                metal
                    .raw_device()
                    .newBufferWithLength_options(
                        super::super::metal_tests::FRAME_WORK_BYTES as usize,
                        MTLResourceOptions::StorageModePrivate,
                    )
                    .unwrap(),
            )
        })
        .collect();
    drop(metal);
    for _ in 0..4 {
        let mut commands = Vec::new();
        let mut native = Vec::new();
        for (source, destination) in &work {
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            // SAFETY: only raw encoding is used, and the test retains every resource until completion.
            native.push(
                unsafe {
                    encoder.as_hal_mut::<wgpu::hal::api::Metal, _, _>(|encoder| {
                        encoder
                            .and_then(|encoder| encoder.raw_command_buffer())
                            .map(|buffer| {
                                let blit = buffer.blitCommandEncoder().unwrap();
                                blit.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                                    source,
                                    0,
                                    destination,
                                    0,
                                    super::super::metal_tests::FRAME_WORK_BYTES as usize,
                                );
                                blit.endEncoding();
                                Message::retain(buffer)
                            })
                    })
                }
                .unwrap(),
            );
            commands.push(encoder.finish());
        }
        let (command, marker) = Marker::new(&device, "independent work completion").unwrap();
        commands.push(command);
        queue.submit(commands);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let MarkerState::Done(end) = marker.read(|buffer| buffer.GPUEndTime()) else {
            panic!("completion marker did not settle");
        };
        for buffer in &native {
            let work_end = host_nanos(buffer.GPUEndTime());
            assert!(
                work_end > 0 && end >= work_end,
                "closing marker completed before prior work"
            );
        }
    }
}
