use std::num::NonZeroU64;

use crate::chunk::*;
use crate::upload_staging::BufferWrite;

/// Arena writes collected while GPU preparation admits uploads.
#[derive(Default)]
pub(in crate::chunk) struct ArenaWrites {
    pub(in crate::chunk) quads: Vec<(u32, Vec<[u32; 2]>)>,
    pub(in crate::chunk) model: Vec<(u32, Vec<[u32; 4]>)>,
    pub(in crate::chunk) model_lighting: Vec<(u32, Vec<[u16; 4]>)>,
    pub(in crate::chunk) model_draw: Vec<(u32, Vec<[u32; 2]>)>,
    pub(in crate::chunk) transparent_model_draw: Vec<(u32, Vec<[u32; 2]>)>,
    pub(in crate::chunk) liquid: Vec<(u32, Vec<[u32; 4]>)>,
    pub(in crate::chunk) liquid_lighting: Vec<(u32, Vec<[u16; 4]>)>,
    pub(in crate::chunk) cube_lighting: Vec<(u32, Vec<[u16; 4]>)>,
    pub(in crate::chunk) biome: Vec<(u32, Vec<u32>)>,
    pub(in crate::chunk) origins: Vec<(u32, GpuChunkOrigin)>,
}

impl ArenaWrites {
    /// Every admitted upload writes an origin record.
    pub(in crate::chunk) fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }

    /// Stages the collected writes into the arena's current buffers. A migration slice
    /// encodes staged copies before it copies, and the next queue submit runs queued
    /// fallback writes first, so they must be issued before a slice copies their regions.
    pub(in crate::chunk) fn issue(&mut self, arena: &ChunkGpuArena, uploads: TerrainUploads<'_>) {
        #[cfg(feature = "tracy")]
        let _span =
            bevy::log::info_span!("terrain.arena_upload", chunks = self.origins.len()).entered();
        let quads = std::mem::take(&mut self.quads);
        let origins = std::mem::take(&mut self.origins);
        let model = std::mem::take(&mut self.model);
        let model_lighting = std::mem::take(&mut self.model_lighting);
        let model_draw = std::mem::take(&mut self.model_draw);
        let transparent = std::mem::take(&mut self.transparent_model_draw);
        let liquid = std::mem::take(&mut self.liquid);
        let liquid_lighting = std::mem::take(&mut self.liquid_lighting);
        let cube_lighting = std::mem::take(&mut self.cube_lighting);
        let biome = std::mem::take(&mut self.biome);

        let mut staged = Vec::new();
        stage(&mut staged, PACKED_QUAD_BYTES, &quads);
        write_merged(uploads, &arena.quad_buffer, &mut staged);
        staged.extend(origins.iter().map(|(index, origin)| {
            (
                u64::from(*index) * CHUNK_ORIGIN_BYTES,
                bytemuck::bytes_of(origin),
            )
        }));
        write_merged(uploads, &arena.origin_buffer, &mut staged);
        let word = GEOMETRY_STREAM_WORD_BYTES;
        stage(&mut staged, word, &model);
        stage(&mut staged, word, &model_lighting);
        stage(&mut staged, word, &model_draw);
        stage(&mut staged, word, &transparent);
        stage(&mut staged, word, &liquid);
        stage(&mut staged, word, &liquid_lighting);
        stage(&mut staged, word, &cube_lighting);
        write_merged(uploads, &arena.geometry_stream_buffer, &mut staged);
        stage(&mut staged, BIOME_WORD_BYTES, &biome);
        write_merged(uploads, &arena.biome_buffer, &mut staged);
    }
}

/// Appends each non-empty record run as its byte offset and bytes, in issue order.
fn stage<'a, T: bytemuck::Pod>(
    staged: &mut Vec<(u64, &'a [u8])>,
    item_bytes: u64,
    writes: &'a [(u32, Vec<T>)],
) {
    staged.extend(
        writes
            .iter()
            .filter(|(_, records)| !records.is_empty())
            .map(|(offset, records)| {
                (
                    u64::from(*offset) * item_bytes,
                    bytemuck::cast_slice(records),
                )
            }),
    );
}

/// Stages `staged` into `buffer` in offset order, so abutting records share one retained
/// copy; overlapping ranges keep their issue order, so the buffer ends identical either way.
/// Whatever the pool cannot hold takes the queue path. Drains `staged`.
fn write_merged(uploads: TerrainUploads<'_>, buffer: &Buffer, staged: &mut Vec<(u64, &[u8])>) {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "terrain.merge_upload_ranges",
        ranges = staged.len(),
        bytes = staged.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
    )
    .entered();
    let mut sorted = staged.clone();
    sorted.sort_by_key(|(offset, _)| *offset);
    let overlapping = sorted
        .windows(2)
        .any(|pair| pair[0].0 + pair[0].1.len() as u64 > pair[1].0);
    let order = if overlapping {
        &staged[..]
    } else {
        &sorted[..]
    };
    let writes: Vec<BufferWrite<'_>> = order
        .iter()
        .map(|&(offset, bytes)| (&**buffer, offset, bytes))
        .collect();
    uploads.write(&writes, |rest| {
        // The unstaged rest is a suffix of `order`, so it keeps its order and overlap.
        let rest: Vec<(u64, &[u8])> = rest
            .iter()
            .map(|&(_, offset, bytes)| (offset, bytes))
            .collect();
        queue_merged(uploads.queue, buffer, &rest, overlapping);
    });
    staged.clear();
}

/// The queue path: one write per abutting run, since each `write_buffer` call allocates its
/// own staging memory, or each record in issue order when ranges overlap.
fn queue_merged(
    render_queue: &RenderQueue,
    buffer: &Buffer,
    order: &[(u64, &[u8])],
    overlapping: bool,
) {
    if overlapping {
        for &(offset, bytes) in order {
            #[cfg(feature = "tracy")]
            let _span =
                bevy::log::info_span!("terrain.arena_write", offset, bytes = bytes.len()).entered();
            render_queue.write_buffer(buffer, offset, bytes);
        }
        return;
    }
    for run in abutting_runs(order) {
        let run = &order[run];
        let offset = run[0].0;
        if let [(_, bytes)] = run {
            #[cfg(feature = "tracy")]
            let _span =
                bevy::log::info_span!("terrain.arena_write", offset, bytes = bytes.len()).entered();
            render_queue.write_buffer(buffer, offset, bytes);
            continue;
        }
        let size = run.iter().map(|(_, bytes)| bytes.len() as u64).sum();
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "terrain.arena_write_merged",
            offset,
            bytes = size,
            ranges = run.len()
        )
        .entered();
        if let Some(size) = NonZeroU64::new(size)
            && let Some(mut view) = render_queue.write_buffer_with(buffer, offset, size)
        {
            let mut at = 0;
            for (_, bytes) in run {
                view.slice(at..at + bytes.len()).copy_from_slice(bytes);
                at += bytes.len();
            }
        }
    }
}

/// Index ranges of `sorted` whose byte ranges each start where the previous one ends.
fn abutting_runs(sorted: &[(u64, &[u8])]) -> Vec<std::ops::Range<usize>> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < sorted.len() {
        let mut end = start + 1;
        let mut next = sorted[start].0 + sorted[start].1.len() as u64;
        while end < sorted.len() && sorted[end].0 == next {
            next += sorted[end].1.len() as u64;
            end += 1;
        }
        runs.push(start..end);
        start = end;
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upload_staging::TerrainUploadStaging;

    /// Merged runs write exactly the bytes the individual writes would, in any issue order.
    #[test]
    fn abutting_runs_reproduce_individual_writes() {
        let records: Vec<Vec<u8>> = (0..12_u8)
            .map(|seed| vec![seed; 4 + 4 * (seed as usize % 3)])
            .collect();
        let mut offset = 0;
        let mut writes = Vec::new();
        for (index, bytes) in records.iter().enumerate() {
            writes.push((offset, bytes.as_slice()));
            // Leave a gap after every third record so runs split.
            offset += bytes.len() as u64 + if index % 3 == 2 { 8 } else { 0 };
        }
        writes.reverse();
        let apply = |buffer: &mut Vec<u8>, at: u64, bytes: &[u8]| {
            buffer[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
        };
        let mut expected = vec![0; offset as usize];
        for (at, bytes) in &writes {
            apply(&mut expected, *at, bytes);
        }
        let mut sorted = writes.clone();
        sorted.sort_by_key(|(at, _)| *at);
        let runs = abutting_runs(&sorted);
        assert_eq!(runs.len(), 4);
        let mut merged = vec![0; offset as usize];
        for run in runs {
            let joined: Vec<u8> = sorted[run.clone()]
                .iter()
                .flat_map(|(_, bytes)| bytes.iter().copied())
                .collect();
            apply(&mut merged, sorted[run.start].0, &joined);
        }
        assert_eq!(merged, expected);
    }

    /// Retained terrain staging and an arena whose quad stream holds 512 records.
    fn staged_arena(device: &RenderDevice) -> (TerrainUploadStaging, ChunkGpuArena) {
        let mut arena = ChunkGpuArena::new(device);
        arena.quad_capacity = 512;
        arena.quad_buffer = create_storage_buffer(device, "test quads", 512 * PACKED_QUAD_BYTES);
        (TerrainUploadStaging::new(device), arena)
    }

    /// One frame's quad records: two abutting allocations and one after a gap.
    fn quad_writes(frame: u32) -> ArenaWrites {
        ArenaWrites {
            quads: vec![
                (16, vec![[frame, 2]; 16]),
                (0, vec![[frame, 1]; 16]),
                (40, vec![[frame, 3]; 8]),
            ],
            ..ArenaWrites::default()
        }
    }

    /// Ends a frame as the graph prefix and device poll do, so completed slots return.
    fn end_frame(device: &RenderDevice, queue: &RenderQueue, staging: &TerrainUploadStaging) {
        let mut encoder = device.create_command_encoder(&Default::default());
        staging.encode_pending(&mut encoder);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::Poll).unwrap();
    }

    /// Copies the first `bytes` of `buffer` out through a mappable buffer.
    fn read(device: &RenderDevice, queue: &RenderQueue, buffer: &Buffer, bytes: u64) -> Vec<u8> {
        let readback = device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain staging readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(buffer, 0, &readback, 0, bytes);
        encoder.map_buffer_on_submit(&readback, wgpu::MapMode::Read, .., |result| {
            result.expect("readback maps");
        });
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::Poll).unwrap();
        readback
            .slice(..)
            .get_mapped_range()
            .expect("readback is mapped")
            .to_vec()
    }

    /// Warm terrain uploads never miss the pool, and abutting records share one copy.
    #[test]
    fn warm_terrain_uploads_stage_without_pool_misses() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
        let (staging, arena) = staged_arena(&device);
        let uploads = TerrainUploads {
            device: &device,
            queue: &queue,
            staging: Some(&staging),
        };
        for frame in 0..4 {
            quad_writes(frame).issue(&arena, uploads);
            end_frame(&device, &queue, &staging);
        }
        let stats = staging.stats();
        assert_eq!(stats.fallback_writes, 0);
        assert_eq!(stats.staged_writes, 12);
        assert_eq!(stats.encoded_copies, 8);
        let words: Vec<[u32; 2]> = [[3, 1]; 16]
            .into_iter()
            .chain([[3, 2]; 16])
            .chain([[0, 0]; 8])
            .chain([[3, 3]; 8])
            .collect();
        let bytes = read(&device, &queue, &arena.quad_buffer, 48 * PACKED_QUAD_BYTES);
        assert_eq!(bytes, bytemuck::cast_slice::<[u32; 2], u8>(&words));
    }

    /// A frame larger than the terrain pool stages what fits and queues only the overflow.
    #[test]
    fn oversized_terrain_frames_queue_only_the_overflow() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
        let (staging, mut arena) = staged_arena(&device);
        let capacity = staging.capacity_bytes();
        let overflow = 64 * 1024;
        let quads = ((capacity + overflow) / PACKED_QUAD_BYTES) as u32;
        arena.quad_capacity = quads as usize;
        arena.quad_buffer =
            create_storage_buffer(&device, "test quads", u64::from(quads) * PACKED_QUAD_BYTES);
        let uploads = TerrainUploads {
            device: &device,
            queue: &queue,
            staging: Some(&staging),
        };
        let half = quads / 2;
        let words: Vec<[u32; 2]> = (0..quads).map(|quad| [quad, !quad]).collect();
        ArenaWrites {
            quads: vec![
                (half, words[half as usize..].to_vec()),
                (0, words[..half as usize].to_vec()),
            ],
            ..ArenaWrites::default()
        }
        .issue(&arena, uploads);
        let stats = staging.stats();
        assert_eq!(stats.staged_bytes, capacity);
        assert_eq!(stats.fallback_bytes, overflow);
        assert_eq!(stats.fallback_writes, 1);
        end_frame(&device, &queue, &staging);
        let bytes = read(
            &device,
            &queue,
            &arena.quad_buffer,
            u64::from(quads) * PACKED_QUAD_BYTES,
        );
        assert_eq!(bytes, bytemuck::cast_slice::<[u32; 2], u8>(&words));
    }

    /// A growth slice copies after this frame's staged writes, so the new buffer keeps them.
    #[test]
    fn migration_slices_carry_staged_writes_into_the_grown_buffer() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
        let (staging, mut arena) = staged_arena(&device);
        let uploads = TerrainUploads {
            device: &device,
            queue: &queue,
            staging: Some(&staging),
        };
        quad_writes(7).issue(&arena, uploads);
        let old = arena.quad_buffer.id();
        let copy_bytes = 512 * PACKED_QUAD_BYTES;
        begin_arena_migration(
            &mut arena,
            &device,
            ArenaStream::Quads,
            ArenaGrowthPlan {
                new_capacity: 1024,
                gpu_copy_bytes: copy_bytes,
            },
        );
        assert_eq!(
            advance_arena_migration(&mut arena, uploads, u64::MAX),
            copy_bytes
        );
        assert!(arena.migration.is_none());
        assert_ne!(arena.quad_buffer.id(), old);
        end_frame(&device, &queue, &staging);
        let bytes = read(&device, &queue, &arena.quad_buffer, 32 * PACKED_QUAD_BYTES);
        let words: Vec<[u32; 2]> = [[7, 1]; 16].into_iter().chain([[7, 2]; 16]).collect();
        assert_eq!(bytes, bytemuck::cast_slice::<[u32; 2], u8>(&words));
    }

    /// Recurring terrain uploads create no Metal buffers; the queue path made one per run.
    #[cfg(target_os = "macos")]
    #[test]
    fn recurring_terrain_uploads_allocate_no_metal_staging_buffers() {
        use crate::upload_allocation_tests::{live_buffers, metal_device};
        let Some((device, queue)) = metal_device() else {
            return;
        };
        let queue = RenderQueue::new(queue);
        let (staging, arena) = staged_arena(&device);
        let uploads = TerrainUploads {
            device: &device,
            queue: &queue,
            staging: Some(&staging),
        };
        // Control: the queue path's staging is visible to the counter until submission.
        let before = live_buffers(&device);
        quad_writes(0).issue(
            &arena,
            TerrainUploads {
                staging: None,
                ..uploads
            },
        );
        assert!(live_buffers(&device) > before);
        end_frame(&device, &queue, &staging);
        for frame in 1..4 {
            let before = live_buffers(&device);
            quad_writes(frame).issue(&arena, uploads);
            assert_eq!(
                live_buffers(&device),
                before,
                "a terrain upload must not allocate a Metal staging buffer"
            );
            end_frame(&device, &queue, &staging);
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
        assert_eq!(staging.stats().fallback_writes, 0);
    }
}
