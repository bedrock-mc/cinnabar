use super::*;
use bevy::render::renderer::RenderQueue;

const SLOTS: usize = FRAME.slots;
const MAX_COPIES: usize = FRAME.max_copies;

/// Supplies validation and synchronous buffer copies without a hardware requirement.
fn setup(slot_bytes: u64) -> (RenderDevice, RenderQueue, Pool, wgpu::Buffer) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let pool = Pool::new(
        &device,
        Shape {
            slot_bytes,
            ..FRAME
        },
    );
    let target = device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("pooled upload destination"),
        size: 1024,
        usage: wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    (device, queue, pool, target)
}

/// Identifies each retained slot buffer so tests can prove none was replaced.
fn buffers(pool: &Pool) -> Vec<wgpu::Buffer> {
    pool.slots.iter().map(|slot| slot.buffer.clone()).collect()
}

/// Reads one copy's staged source bytes while its slot is still mapped.
fn source(pool: &Pool, copy: &Copy) -> Vec<u8> {
    let mapped = copy.source - copy.source % wgpu::MAP_ALIGNMENT;
    pool.slots[copy.slot]
        .buffer
        .slice(mapped..copy.source + copy.bytes)
        .get_mapped_range()
        .expect("staging slot is mapped")[(copy.source - mapped) as usize..]
        .to_vec()
}

#[test]
fn completed_slots_reuse_buffers_and_empty_batches_create_no_work() {
    let (device, queue, mut pool, target) = setup(64);
    let retained = buffers(&pool);
    for value in 0..SLOTS * 2 {
        assert_eq!(pool.stage_prefix(&[(&target, 0, &[value as u8; 64])]), None);
        let mut encoder = device.create_command_encoder(&Default::default());
        pool.encode(&mut encoder);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::Poll).unwrap();
        assert_eq!(buffers(&pool), retained);
        assert!(
            pool.slots
                .iter()
                .all(|slot| slot.state.load(Ordering::Acquire) == READY)
        );
    }
    assert_eq!(pool.stats.staged_writes, (SLOTS * 2) as u64);
    assert_eq!(pool.stats.staged_bytes, (SLOTS * 2 * 64) as u64);
    assert_eq!(pool.diagnostic_counts(), ([SLOTS, 0, 0, 0], 0));
    let stats = pool.stats;
    assert_eq!(pool.stage_prefix(&[(&target, 0, &[])]), None);
    assert!(!pool.has_copies());
    assert_eq!(pool.stats, stats);
}

#[test]
fn incomplete_slots_refuse_work_without_growth_or_polling() {
    let (device, queue, mut pool, target) = setup(64);
    let retained = buffers(&pool);
    let bytes = [1; 64];
    let writes = vec![(&target, 0, bytes.as_slice()); SLOTS];
    assert_eq!(pool.stage_prefix(&writes), None);
    let mut encoder = device.create_command_encoder(&Default::default());
    pool.encode(&mut encoder);
    let stats = pool.stats;
    assert_eq!(pool.stage_prefix(&[(&target, 256, &[5; 4])]), Some((0, 0)));
    assert_eq!(buffers(&pool), retained);
    assert!(
        pool.slots
            .iter()
            .all(|slot| slot.state.load(Ordering::Acquire) == PENDING)
    );
    assert!(!pool.has_copies());
    assert_eq!(pool.stats, stats);
    assert_eq!(pool.copies.capacity(), MAX_COPIES);
    assert_eq!(pool.diagnostic_counts(), ([0, 0, SLOTS, 0], 0));
    queue.submit([encoder.finish()]);
    device.poll(wgpu::PollType::Poll).unwrap();
}

/// A batch larger than the pool stages every byte that fits and reports where staging stopped.
#[test]
fn oversized_batches_stage_what_fits() {
    let (_, _, mut pool, target) = setup(64);
    let large = vec![2; SLOTS * 64];
    let first = 16;
    assert_eq!(
        pool.stage_prefix(&[(&target, 0, &[1; 16]), (&target, 16, large.as_slice())]),
        Some((1, (SLOTS * 64 - first) as u64))
    );
    assert!(
        pool.slots
            .iter()
            .all(|slot| slot.offset == 64 && slot.state.load(Ordering::Acquire) == ACTIVE)
    );
    assert_eq!(pool.stats.staged_writes, 1);
    assert_eq!(pool.stats.staged_bytes, (SLOTS * 64) as u64);
    let mut staged = vec![0; SLOTS * 64];
    for copy in &pool.copies {
        let at = copy.offset as usize;
        staged[at..at + copy.bytes as usize].copy_from_slice(&source(&pool, copy));
    }
    assert_eq!(&staged[..16], &[1; 16]);
    assert!(staged[16..].iter().all(|byte| *byte == 2));
}

#[test]
fn copy_metadata_exhaustion_stages_nothing_more_without_growing() {
    let (_, _, mut pool, target) = setup(4096);
    let bytes = [1; 4];
    let writes = vec![(&target, 0, bytes.as_slice()); MAX_COPIES];
    assert_eq!(pool.stage_prefix(&writes), None);
    let stats = pool.stats;
    // Not abutting the newest copy, so it needs a record of its own.
    assert_eq!(pool.stage_prefix(&[(&target, 8, &[2; 4])]), Some((0, 0)));
    assert_eq!(pool.copies.len(), MAX_COPIES);
    assert_eq!(pool.copies.capacity(), MAX_COPIES);
    assert_eq!(pool.stats, stats);
}

#[test]
fn failed_mapping_slots_are_quarantined_without_replacement() {
    let (_, _, mut pool, target) = setup(64);
    let retained = buffers(&pool);
    pool.slots[0].state.store(FAILED, Ordering::Release);
    let bytes = [1; 64];
    let writes = vec![(&target, 0, bytes.as_slice()); SLOTS - 1];
    assert_eq!(pool.stage_prefix(&writes), None);
    assert_eq!(pool.stage_prefix(&[(&target, 192, &[4; 4])]), Some((0, 0)));
    assert!(pool.copies.iter().all(|copy| copy.slot != 0));
    assert_eq!(pool.slots[0].state.load(Ordering::Acquire), FAILED);
    assert_eq!(pool.slots[0].offset, 0);
    assert_eq!(pool.diagnostic_counts(), ([0, SLOTS - 1, 0, 1], SLOTS - 1));
    assert_eq!(buffers(&pool), retained);
}

#[test]
fn four_byte_payload_sizes_keep_the_next_mapped_offset_aligned() {
    let (_, _, mut pool, target) = setup(192);
    assert_eq!(
        pool.stage_prefix(&[(&target, 0, &[1; 132]), (&target, 136, &[2; 20])]),
        None
    );
    assert_eq!(pool.copies[0].source, 0);
    assert_eq!(pool.copies[1].source, 136);
    for copy in &pool.copies {
        assert_eq!(copy.source % wgpu::MAP_ALIGNMENT, 0);
        let expected = if copy.offset == 0 { 1 } else { 2 };
        assert!(source(&pool, copy).iter().all(|byte| *byte == expected));
    }
}

/// Abutting records become one copy even when a record ends mid mapping granule.
#[test]
fn abutting_writes_share_one_copy() {
    let (_, _, mut pool, target) = setup(256);
    assert_eq!(
        pool.stage_prefix(&[
            (&target, 0, &[1; 12]),
            (&target, 12, &[2; 20]),
            (&target, 32, &[3; 8]),
        ]),
        None
    );
    assert_eq!(pool.copies.len(), 1);
    assert_eq!((pool.copies[0].offset, pool.copies[0].bytes), (0, 40));
    let expected: Vec<u8> = [vec![1; 12], vec![2; 20], vec![3; 8]].concat();
    assert_eq!(source(&pool, &pool.copies[0]), expected);
    assert_eq!(pool.stats.staged_writes, 3);
}

/// A write larger than one slot spans free slots instead of falling back.
#[test]
fn large_writes_split_across_slots_in_destination_order() {
    let (_, _, mut pool, target) = setup(64);
    let bytes: Vec<u8> = (0..160_u8).collect();
    assert_eq!(pool.stage_prefix(&[(&target, 32, &bytes)]), None);
    assert_eq!(pool.copies.len(), 3);
    let mut joined = Vec::new();
    let mut offset = 32;
    for copy in &pool.copies {
        assert_eq!(copy.offset, offset);
        offset += copy.bytes;
        joined.extend(source(&pool, copy));
    }
    assert_eq!(joined, bytes);
    assert_eq!(pool.stats.staged_writes, 1);
    assert_eq!(pool.stats.staged_bytes, 160);
}

/// Growth adds slots only after misses, at most the per-frame step and never past the ceiling.
#[test]
fn growth_follows_misses_up_to_the_ceiling() {
    let (device, _, _, _) = setup(64);
    let mut pool = Pool::new(
        &device,
        Shape {
            slots: 2,
            max_slots: 5,
            growth_slots: 2,
            slot_bytes: 64,
            ..FRAME
        },
    );
    pool.grow();
    assert_eq!(pool.slots.len(), 2);
    pool.record_miss(1, 4 * 64);
    pool.grow();
    assert_eq!(pool.slots.len(), 4);
    pool.grow();
    assert_eq!(pool.slots.len(), 4);
    pool.record_miss(1, 4 * 64);
    pool.grow();
    assert_eq!(pool.slots.len(), 5);
    assert_eq!(pool.stats.grown_slots, 3);
    assert!(
        pool.slots
            .iter()
            .all(|slot| slot.state.load(Ordering::Acquire) == READY)
    );
}
