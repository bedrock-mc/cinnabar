use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Every byte the 64-byte test slots hold.
const CAPACITY: usize = FRAME.slots * 64;
/// Larger than the whole test pool, so its tail always takes the queue path.
const OVERSIZED: usize = 600;

/// Uses NOOP's real buffer copies to check final bytes without driver scheduling assumptions.
fn setup() -> (RenderDevice, RenderQueue, Staging, wgpu::Buffer) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let staging = Staging::new(
        &device,
        Shape {
            slot_bytes: 64,
            ..FRAME
        },
    );
    let target = device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("upload ordering destination"),
        size: 1024,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    (device, queue, staging, target)
}

/// Submits the upload prefix, then reads actual destination bytes after a nonblocking NOOP poll.
fn finish(
    device: &RenderDevice,
    queue: &RenderQueue,
    staging: &Staging,
    target: &wgpu::Buffer,
) -> Vec<u8> {
    let mut encoder = device.create_command_encoder(&Default::default());
    staging.encode_pending(&mut encoder);
    let mapped = Arc::new(AtomicBool::new(false));
    let complete = mapped.clone();
    encoder.map_buffer_on_submit(target, wgpu::MapMode::Read, .., move |result| {
        result.unwrap();
        complete.store(true, Ordering::Release);
    });
    queue.submit([encoder.finish()]);
    device.poll(wgpu::PollType::Poll).unwrap();
    assert!(mapped.load(Ordering::Acquire));
    let bytes = target
        .slice(..)
        .get_mapped_range()
        .expect("readback buffer is mapped")
        .to_vec();
    target.unmap();
    bytes
}

/// A batch larger than the pool stages what fits; only the overflow takes the queue path.
#[test]
fn batches_larger_than_the_pool_queue_only_the_overflow() {
    let (device, queue, staging, target) = setup();
    let bytes: Vec<u8> = (0..OVERSIZED).map(|byte| byte as u8).collect();
    let mut queued = Vec::new();
    staging.write_batch_or(&device, &queue, &[(&target, 0, bytes.as_slice())], |rest| {
        queued.extend(rest.iter().map(|&(_, offset, bytes)| (offset, bytes.len())));
        for &(buffer, offset, bytes) in rest {
            queue.write_buffer(buffer, offset, bytes);
        }
    });
    assert_eq!(queued, [(CAPACITY as u64, OVERSIZED - CAPACITY)]);
    let stats = staging.lock().stats;
    assert_eq!(stats.staged_bytes, CAPACITY as u64);
    assert_eq!(stats.fallback_writes, 1);
    assert_eq!(stats.fallback_bytes, (OVERSIZED - CAPACITY) as u64);
    assert_eq!(stats.overflow_submissions, 0);
    let written = finish(&device, &queue, &staging, &target);
    assert_eq!(&written[..OVERSIZED], &bytes[..]);
}

/// Unstaged bytes that overlap older staged copies land after them; later staged bytes land last.
#[test]
fn overlapping_fallback_preserves_pooled_fallback_and_later_pooled_bytes() {
    let (device, queue, staging, target) = setup();
    staging.write_batch(&device, &queue, &[(&target, 0, &[1; CAPACITY])]);
    staging.write_batch(&device, &queue, &[(&target, 8, &[2; 16])]);
    let stats = staging.lock().stats;
    assert_eq!(stats.fallback_bytes, 16);
    assert_eq!(stats.overflow_submissions, 1);
    staging.write_batch(&device, &queue, &[(&target, 16, &[3; 4])]);
    let bytes = finish(&device, &queue, &staging, &target);
    assert_eq!(&bytes[..8], &[1; 8]);
    assert_eq!(&bytes[8..16], &[2; 8]);
    assert_eq!(&bytes[16..20], &[3; 4]);
    assert_eq!(&bytes[20..24], &[2; 4]);
    assert!(bytes[24..CAPACITY].iter().all(|byte| *byte == 1));
}

#[test]
fn empty_overlaps_do_not_flush_disjoint_fallback_work() {
    let (device, queue, staging, target) = setup();
    let oversized = [2; OVERSIZED];
    staging.write_batch(&device, &queue, &[(&target, 0, &[1; 16])]);
    staging.write_batch(
        &device,
        &queue,
        &[(&target, 8, &[]), (&target, 32, &oversized)],
    );
    let stats = staging.lock().stats;
    assert_eq!(stats.overflow_submissions, 0);
    assert_eq!(stats.fallback_writes, 1);
    assert_eq!(stats.fallback_bytes, (OVERSIZED - (CAPACITY - 16)) as u64);
    let bytes = finish(&device, &queue, &staging, &target);
    assert_eq!(&bytes[..16], &[1; 16]);
    assert!(bytes[32..32 + OVERSIZED].iter().all(|byte| *byte == 2));
}

/// The ordering flush submits after the pool lock is released, so it never blocks stagers.
#[test]
fn overlap_flush_submits_without_holding_the_pool_lock() {
    let (device, queue, staging, target) = setup();
    let staging = Arc::new(staging);
    staging.write_batch(&device, &queue, &[(&target, 0, &[1; CAPACITY])]);
    let callback_staging = Arc::clone(&staging);
    let callback_ran = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&callback_ran);
    queue.on_submitted_work_done(move || {
        assert!(callback_staging.0.try_lock().is_ok());
        observed.store(true, Ordering::Release);
    });
    staging.write_batch(&device, &queue, &[(&target, 8, &[2; 8])]);
    assert!(callback_ran.load(Ordering::Acquire));
    let bytes = finish(&device, &queue, &staging, &target);
    assert_eq!(&bytes[..8], &[1; 8]);
    assert_eq!(&bytes[8..16], &[2; 8]);
    assert!(bytes[16..CAPACITY].iter().all(|byte| *byte == 1));
}

/// A full pool hands the owner's own fallback only the unstaged writes, after older copies.
#[test]
fn custom_fallback_runs_after_older_overlapping_copies() {
    let (device, queue, staging, target) = setup();
    staging.write_batch(&device, &queue, &[(&target, 0, &[1; CAPACITY])]);
    let mut fallbacks = 0;
    staging.write_batch_or(&device, &queue, &[(&target, 8, &[2; 8])], |rest| {
        fallbacks += 1;
        assert_eq!(rest.len(), 1);
        queue.write_buffer(&target, rest[0].1, rest[0].2);
    });
    assert_eq!(fallbacks, 1);
    let bytes = finish(&device, &queue, &staging, &target);
    assert_eq!(&bytes[..8], &[1; 8]);
    assert_eq!(&bytes[8..16], &[2; 8]);
    assert_eq!(staging.lock().stats.overflow_submissions, 1);
}
