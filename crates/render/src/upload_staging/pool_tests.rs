use super::*;
use bevy::render::renderer::{RenderQueue, WgpuWrapper};

/// Supplies validation and synchronous buffer copies without a hardware requirement.
fn setup(slot_bytes: u64) -> (RenderDevice, RenderQueue, Pool, wgpu::Buffer) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let pool = Pool::new(&device, slot_bytes);
    let target = device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("pooled upload destination"),
        size: 1024,
        usage: wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    (device, queue, pool, target)
}

#[test]
fn completed_slots_reuse_buffers_and_empty_batches_create_no_work() {
    let (device, queue, mut pool, target) = setup(64);
    let buffers = pool.slots.each_ref().map(|slot| slot.buffer.clone());
    for value in 0..SLOT_COUNT * 2 {
        assert!(pool.try_stage(&[(&target, 0, &[value as u8; 64])]));
        let mut encoder = device.create_command_encoder(&Default::default());
        pool.encode(&mut encoder);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::Poll).unwrap();
        assert_eq!(
            pool.slots.each_ref().map(|slot| &slot.buffer),
            buffers.each_ref()
        );
        assert!(
            pool.slots
                .iter()
                .all(|slot| slot.state.load(Ordering::Acquire) == READY)
        );
    }
    assert_eq!(pool.stats.staged_writes, (SLOT_COUNT * 2) as u64);
    assert_eq!(pool.stats.staged_bytes, (SLOT_COUNT * 2 * 64) as u64);
    assert_eq!(pool.diagnostic_counts(), ([SLOT_COUNT, 0, 0, 0], 0));
    let stats = pool.stats;
    assert!(pool.try_stage(&[(&target, 0, &[])]));
    assert!(!pool.has_copies());
    assert_eq!(pool.stats, stats);
}

#[test]
fn incomplete_slots_refuse_work_without_growth_or_polling() {
    let (device, queue, mut pool, target) = setup(64);
    let buffers = pool.slots.each_ref().map(|slot| slot.buffer.clone());
    let bytes = [1; 64];
    let writes = vec![(&target, 0, bytes.as_slice()); SLOT_COUNT];
    assert!(pool.try_stage(&writes));
    let mut encoder = device.create_command_encoder(&Default::default());
    pool.encode(&mut encoder);
    let stats = pool.stats;
    assert!(!pool.try_stage(&[(&target, 256, &[5; 4])]));
    assert_eq!(
        pool.slots.each_ref().map(|slot| &slot.buffer),
        buffers.each_ref()
    );
    assert!(
        pool.slots
            .iter()
            .all(|slot| slot.state.load(Ordering::Acquire) == PENDING)
    );
    assert!(!pool.has_copies());
    assert_eq!(pool.stats, stats);
    assert_eq!(pool.copies.capacity(), MAX_COPIES);
    assert_eq!(pool.diagnostic_counts(), ([0, 0, SLOT_COUNT, 0], 0));
    queue.submit([encoder.finish()]);
    device.poll(wgpu::PollType::Poll).unwrap();
}

#[test]
fn oversized_batches_preserve_all_slots_without_partial_writes() {
    let (_, _, mut pool, target) = setup(64);
    assert!(!pool.try_stage(&[(&target, 0, &[1; 16]), (&target, 16, &[2; 80])]));
    assert!(!pool.has_copies());
    assert!(
        pool.slots
            .iter()
            .all(|slot| { slot.offset == 0 && slot.state.load(Ordering::Acquire) == READY })
    );
    assert_eq!(pool.stats, Stats::default());
}

#[test]
fn copy_metadata_exhaustion_refuses_the_batch_without_growing() {
    let (_, _, mut pool, target) = setup(4096);
    let bytes = [1; 4];
    let writes = vec![(&target, 0, bytes.as_slice()); MAX_COPIES];
    assert!(pool.try_stage(&writes));
    let stats = pool.stats;
    assert!(!pool.try_stage(&[(&target, 4, &[2; 4])]));
    assert_eq!(pool.copies.len(), MAX_COPIES);
    assert_eq!(pool.copies.capacity(), MAX_COPIES);
    assert_eq!(pool.stats, stats);
}

#[test]
fn failed_mapping_slots_are_quarantined_without_replacement() {
    let (_, _, mut pool, target) = setup(64);
    let buffers = pool.slots.each_ref().map(|slot| slot.buffer.clone());
    pool.slots[0].state.store(FAILED, Ordering::Release);
    let bytes = [1; 64];
    let writes = vec![(&target, 0, bytes.as_slice()); SLOT_COUNT - 1];
    assert!(pool.try_stage(&writes));
    assert!(!pool.try_stage(&[(&target, 192, &[4; 4])]));
    assert!(pool.copies.iter().all(|copy| copy.slot != 0));
    assert_eq!(pool.slots[0].state.load(Ordering::Acquire), FAILED);
    assert_eq!(pool.slots[0].offset, 0);
    assert_eq!(
        pool.diagnostic_counts(),
        ([0, SLOT_COUNT - 1, 0, 1], SLOT_COUNT - 1)
    );
    assert_eq!(
        pool.slots.each_ref().map(|slot| &slot.buffer),
        buffers.each_ref()
    );
}

#[test]
fn four_byte_payload_sizes_keep_the_next_mapped_offset_aligned() {
    let (_, _, mut pool, target) = setup(192);
    assert!(pool.try_stage(&[(&target, 0, &[1; 132]), (&target, 132, &[2; 20])]));
    assert_eq!(pool.copies[0].source, 0);
    assert_eq!(pool.copies[1].source, 136);
    for copy in &pool.copies {
        assert_eq!(copy.source % wgpu::MAP_ALIGNMENT, 0);
        let bytes = pool.slots[copy.slot]
            .buffer
            .slice(copy.source..copy.source + copy.bytes)
            .get_mapped_range();
        assert!(
            bytes
                .iter()
                .all(|byte| *byte == if copy.offset == 0 { 1 } else { 2 })
        );
    }
}
