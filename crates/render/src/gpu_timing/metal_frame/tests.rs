use super::*;
use render_scene::RuntimeStage;

#[test]
fn full_latency_ring_skips_without_publishing_pending_callbacks() {
    let now = Instant::now();
    let mut frames = MetalFrames::default();
    for _ in 0..SLOTS {
        frames.begin(now, |_| panic!("pending frame was published"));
        let slot = frames.recording.take().unwrap();
        frames.ring.submit(slot);
    }
    frames.begin(now, |_| panic!("pending frame was published"));
    assert!(frames.recording.is_none());
    assert_eq!(frames.ring.oldest_in_flight(), Some(0));
    let mut samples = Vec::new();
    // A later completion must not overtake the oldest pending callback.
    complete(
        &frames.slots[1].elapsed,
        now,
        now + Duration::from_millis(7),
    );
    frames.begin(now, |frame| samples.push(*frame));
    assert!(samples.is_empty());
    complete(
        &frames.slots[0].elapsed,
        now,
        now + Duration::from_millis(9),
    );
    frames.begin(now, |frame| samples.push(*frame));
    assert_eq!(samples.len(), 2);
    assert_eq!(
        samples[0].get(RuntimeStage::GpuFrameCompletionLatency),
        Some(Duration::from_millis(9))
    );
    assert_eq!(
        samples[1].get(RuntimeStage::GpuFrameCompletionLatency),
        Some(Duration::from_millis(7))
    );
    assert_eq!(samples[0].get(RuntimeStage::GpuFrame), None);
    assert!(frames.recording.is_some());
    let slot = frames.recording.take().unwrap();
    frames.ring.release(slot);
    frames.begin(now, |_| panic!("completed sample was published twice"));
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
