use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Uses NOOP's real buffer copies to check final bytes without driver scheduling assumptions.
fn setup() -> (RenderDevice, RenderQueue, BufferUploadStaging, wgpu::Buffer) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let staging = BufferUploadStaging(Mutex::new(Pool::new(&device, 64)));
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
    staging: &BufferUploadStaging,
    target: &wgpu::Buffer,
) -> Vec<u8> {
    let mut encoder = device.create_command_encoder(&Default::default());
    staging.0.lock().unwrap().encode(&mut encoder);
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

#[test]
fn overlapping_fallback_preserves_pooled_fallback_and_later_pooled_bytes() {
    let (device, queue, staging, target) = setup();
    staging.write_batch(&device, &queue, &[(&target, 0, &[1; 16])]);
    staging.write_batch(&device, &queue, &[(&target, 8, &[2; 80])]);
    staging.write_batch(&device, &queue, &[(&target, 16, &[3; 4])]);
    let bytes = finish(&device, &queue, &staging, &target);
    assert_eq!(&bytes[..8], &[1; 8]);
    assert_eq!(&bytes[8..16], &[2; 8]);
    assert_eq!(&bytes[16..20], &[3; 4]);
    assert_eq!(&bytes[20..88], &[2; 68]);
    assert!(bytes[88..].iter().all(|byte| *byte == 0));
    let stats = staging.0.lock().unwrap().stats;
    assert_eq!(stats.staged_writes, 2);
    assert_eq!(stats.fallback_writes, 1);
    assert_eq!(stats.fallback_bytes, 80);
    assert_eq!(stats.overflow_submissions, 1);
}

#[test]
fn empty_overlaps_do_not_flush_disjoint_fallback_work() {
    let (device, queue, staging, target) = setup();
    staging.write_batch(&device, &queue, &[(&target, 0, &[1; 16])]);
    staging.write_batch(
        &device,
        &queue,
        &[(&target, 8, &[]), (&target, 32, &[2; 80])],
    );
    assert_eq!(staging.0.lock().unwrap().stats.overflow_submissions, 0);
    let bytes = finish(&device, &queue, &staging, &target);
    assert_eq!(&bytes[..16], &[1; 16]);
    assert_eq!(&bytes[32..112], &[2; 80]);
    assert_eq!(staging.0.lock().unwrap().stats.fallback_writes, 1);
}

#[test]
fn concurrent_fallback_keeps_older_flushes_under_the_same_lock() {
    let (device, queue, staging, target) = setup();
    let staging = Arc::new(staging);
    staging.write_batch(&device, &queue, &[(&target, 0, &[1; 16])]);
    let callback_staging = Arc::clone(&staging);
    let callback_ran = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&callback_ran);
    queue.on_submitted_work_done(move || {
        assert!(matches!(
            callback_staging.0.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        observed.store(true, Ordering::Release);
    });
    staging.write_batch(&device, &queue, &[(&target, 8, &[2; 80])]);
    assert!(callback_ran.load(Ordering::Acquire));
    let bytes = finish(&device, &queue, &staging, &target);
    assert_eq!(&bytes[..8], &[1; 8]);
    assert_eq!(&bytes[8..88], &[2; 80]);
}
