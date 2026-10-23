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

/// Allocates the production frame shape and a destination large enough for one full frame.
fn frame_pool() -> (RenderDevice, RenderQueue, Pool, wgpu::Buffer) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let target = device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("maximum particle frame destination"),
        size: PARTICLE_FRAME_BYTES + FRAME_OTHER_BYTES,
        usage: wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let pool = Pool::new(&device, FRAME);
    (device, queue, pool, target)
}

/// Splits the particle cap across all material lists, with disjoint hand/UI data and alignment gaps.
fn frame_payloads() -> Vec<(u64, Vec<u8>)> {
    let instance_bytes = std::mem::size_of::<particles::ParticleInstance>();
    let opaque = particles::MAX_LIVE_PARTICLES / 3;
    let blend = particles::MAX_LIVE_PARTICLES / 5;
    let add = particles::MAX_LIVE_PARTICLES - opaque - blend;
    let mut offset = FRAME_OTHER_BYTES;
    let mut writes = vec![
        (0, vec![7; 4092]),
        (4096, vec![8; FRAME_OTHER_BYTES as usize - 4112]),
    ];
    for (index, count) in [opaque, blend, add].into_iter().enumerate() {
        let bytes = vec![index as u8 + 1; count * instance_bytes];
        writes.push((offset, bytes));
        offset += (count * instance_bytes) as u64;
    }
    assert_eq!(offset - FRAME_OTHER_BYTES, PARTICLE_FRAME_BYTES);
    writes
}

#[test]
fn maximum_particle_frames_stage_while_four_frames_await_completion_then_reuse() {
    let (device, queue, mut pool, target) = frame_pool();
    let retained = buffers(&pool);
    let payloads = frame_payloads();
    let writes: Vec<_> = payloads
        .iter()
        .map(|(offset, bytes)| (&target, *offset, bytes.as_slice()))
        .collect();
    let mut commands = Vec::new();
    let frames = FRAME.slots / FRAME_SLOTS_PER_FRAME as usize;
    for _ in 0..frames {
        assert_eq!(pool.stage_prefix(&writes), None);
        let mut expected = vec![0; (PARTICLE_FRAME_BYTES + FRAME_OTHER_BYTES) as usize];
        for (offset, bytes) in &payloads {
            expected[*offset as usize..*offset as usize + bytes.len()].copy_from_slice(bytes);
        }
        let mut staged = vec![0; expected.len()];
        for copy in &pool.copies {
            let offset = copy.offset as usize;
            staged[offset..offset + copy.bytes as usize].copy_from_slice(&source(&pool, copy));
            assert_eq!(copy.source % wgpu::COPY_BUFFER_ALIGNMENT, 0);
        }
        assert_eq!(staged, expected);
        assert!(pool.copies.len() <= FRAME.max_copies);
        let mut encoder = device.create_command_encoder(&Default::default());
        pool.encode(&mut encoder);
        commands.push(encoder.finish());
    }
    assert_eq!(pool.diagnostic_counts(), ([0, 0, FRAME.slots, 0], 0));
    assert_eq!(pool.stage_prefix(&writes), Some((0, 0)));
    queue.submit(commands);
    device.poll(wgpu::PollType::Poll).unwrap();
    assert_eq!(pool.stage_prefix(&writes), None);
    assert_eq!(buffers(&pool), retained);
    assert_eq!(pool.stats.grown_slots, 0);
    assert_eq!(
        pool.stats.staged_writes,
        ((frames + 1) * writes.len()) as u64
    );
}

#[test]
fn maximum_frame_uses_ready_slots_and_keeps_failed_and_pending_storage_quarantined() {
    let (_, _, mut pool, target) = frame_pool();
    let unavailable = FRAME.slots - FRAME_SLOTS_PER_FRAME as usize;
    for (index, slot) in pool.slots.iter().take(unavailable).enumerate() {
        slot.state.store(
            if index % 2 == 0 { FAILED } else { PENDING },
            Ordering::Release,
        );
    }
    let payloads = frame_payloads();
    let writes: Vec<_> = payloads
        .iter()
        .map(|(offset, bytes)| (&target, *offset, bytes.as_slice()))
        .collect();
    assert_eq!(pool.stage_prefix(&writes), None);
    assert!(pool.copies.iter().all(|copy| copy.slot >= unavailable));
    let Some((index, staged)) = pool.stage_prefix(&writes) else {
        panic!("a second maximum frame must exceed the remaining capacity");
    };
    assert_eq!(index, 2, "hand/UI writes fit before particle overflow");
    assert!(staged > 0 && staged < payloads[index].1.len() as u64);
    assert_eq!(staged % wgpu::COPY_BUFFER_ALIGNMENT, 0);
    assert!(pool.copies.iter().all(|copy| copy.slot >= unavailable));
    assert!(pool.stats.staged_bytes <= FRAME_SLOT_BYTES * FRAME_SLOTS_PER_FRAME);
}

/// Slot growth that runs out of memory keeps the existing slots and retries after later misses.
#[test]
fn growth_without_memory_keeps_slots_and_retries() {
    let (device, _, _, _) = setup(64);
    let terrain = crate::gpu_allocation::FailureFlag::new(&device);
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
    {
        let _failure = crate::gpu_allocation::inject(|_, _| true);
        pool.record_miss(1, 4 * 64);
        pool.grow();
    }
    assert_eq!(pool.slots.len(), 2);
    assert_eq!(pool.stats.grown_slots, 0);
    assert!(
        !terrain.failed(),
        "optional growth must keep terrain drawable"
    );
    pool.record_miss(1, 4 * 64);
    pool.grow();
    assert_eq!(pool.slots.len(), 4);
}
