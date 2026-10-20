use crate::queue_writes::ProfiledQueue as _;
use std::num::NonZeroU64;

use crate::chunk::gpu::arena::FreshChunkRanges;
use crate::chunk::*;
use crate::upload_staging::BufferWrite;
use meshing::PackedBiomeRecord;
use render_scene::ChunkRenderInstance;

/// Stream order used before address sorting; overlapping writes retain this order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Quads,
    Origins,
    Model,
    ModelLighting,
    ModelDraw,
    TransparentModelDraw,
    Liquid,
    LiquidLighting,
    CubeLighting,
    Biome,
}

impl Part {
    /// Returns the destination arena and the unit used by its allocation offsets.
    fn destination(self) -> (ArenaStream, u64) {
        match self {
            Self::Quads => (ArenaStream::Quads, PACKED_QUAD_BYTES),
            Self::Origins => (ArenaStream::Origins, CHUNK_ORIGIN_BYTES),
            Self::Biome => (ArenaStream::Biomes, BIOME_WORD_BYTES),
            Self::Model
            | Self::ModelLighting
            | Self::ModelDraw
            | Self::TransparentModelDraw
            | Self::Liquid
            | Self::LiquidLighting
            | Self::CubeLighting => (ArenaStream::GeometryStreams, GEOMETRY_STREAM_WORD_BYTES),
        }
    }
}

/// Immutable payloads stay borrowed through their shared owners until issue completes.
enum Payload {
    Quads(Arc<[PackedQuad]>),
    Lighting(Arc<[PackedQuadLighting]>),
    Biome(PackedBiomeRecord),
    Encoded(Range<usize>),
}

impl Payload {
    /// Borrows unchanged records or the final relocated bytes in retained scratch storage.
    fn bytes<'a>(&'a self, encoded: &'a [u8]) -> &'a [u8] {
        match self {
            Self::Quads(records) => quad_bytes(records),
            Self::Lighting(records) => lighting_bytes(records),
            Self::Biome(record) => bytemuck::cast_slice(record.words()),
            Self::Encoded(range) => &encoded[range.clone()],
        }
    }
}

struct Write {
    part: Part,
    offset: u64,
    payload: Payload,
}

/// Retained terrain encoding and descriptor scratch, cleared after each issue.
#[derive(Default)]
pub(in crate::chunk) struct ArenaWrites {
    writes: Vec<Write>,
    encoded: Vec<u8>,
    order: Vec<usize>,
    chunks: usize,
}

impl ArenaWrites {
    /// Collects admitted streams, borrowing unchanged payloads and encoding relocations once.
    pub(in crate::chunk) fn collect_chunk(
        &mut self,
        instance: &ChunkRenderInstance,
        plan: &FreshChunkRanges,
        metadata_index: u32,
        biome_words: &[u32],
        origin: GpuChunkOrigin,
    ) {
        self.push(
            Part::Quads,
            plan.quad_start,
            Payload::Quads(Arc::clone(&instance.cube_quads)),
        );
        self.encode(
            Part::Model,
            plan.model_start,
            instance.model_refs.iter().map(|record| {
                let mut words = record.words();
                super::absolutize_model_lighting_bases(
                    std::slice::from_mut(&mut words),
                    plan.model_lighting_start,
                );
                words
            }),
        );
        self.push(
            Part::ModelLighting,
            plan.model_lighting_start,
            Payload::Lighting(Arc::clone(&instance.model_lighting)),
        );
        for (part, start, records) in [
            (
                Part::ModelDraw,
                plan.model_draw_start,
                &instance.model_draw_refs,
            ),
            (
                Part::TransparentModelDraw,
                plan.transparent_model_draw_start,
                &instance.transparent_model_draw_refs,
            ),
        ] {
            self.encode(
                part,
                start,
                records.iter().map(|record| {
                    let mut words = record.words();
                    super::absolutize_partitioned_model_draw_refs(
                        std::slice::from_mut(&mut words),
                        &mut [],
                        plan.model_start,
                    )
                    .expect(
                        "validated model draw refs and atomic arena plan fit absolute addressing",
                    );
                    words
                }),
            );
        }
        self.encode(
            Part::Liquid,
            plan.liquid_start,
            instance.liquid_quads.iter().map(|record| {
                let mut words = record.words();
                stamp_liquid_metadata_index(std::slice::from_mut(&mut words), metadata_index);
                words
            }),
        );
        self.push(
            Part::LiquidLighting,
            plan.liquid_lighting_start,
            Payload::Lighting(Arc::clone(&instance.liquid_lighting)),
        );
        self.push(
            Part::CubeLighting,
            plan.cube_lighting_start,
            Payload::Lighting(Arc::clone(&instance.cube_lighting)),
        );
        if !biome_words.is_empty() {
            debug_assert_eq!(biome_words, instance.biome.words());
            self.push(
                Part::Biome,
                plan.biome_start,
                Payload::Biome(instance.biome.clone()),
            );
        }
        self.encode(Part::Origins, metadata_index, std::iter::once(origin));
        self.chunks += 1;
    }

    /// Appends one nonempty payload in its original stream order.
    fn push(&mut self, part: Part, offset: u32, payload: Payload) {
        if !payload.bytes(&self.encoded).is_empty() {
            self.writes.push(Write {
                part,
                offset: u64::from(offset) * part.destination().1,
                payload,
            });
        }
    }

    /// Encodes relocated records directly into reusable storage, without a temporary vector.
    fn encode<T: bytemuck::Pod>(
        &mut self,
        part: Part,
        offset: u32,
        records: impl ExactSizeIterator<Item = T>,
    ) {
        let start = self.encoded.len();
        self.encoded
            .reserve(records.len() * std::mem::size_of::<T>());
        for record in records {
            self.encoded.extend_from_slice(bytemuck::bytes_of(&record));
        }
        self.push(part, offset, Payload::Encoded(start..self.encoded.len()));
    }

    /// Reports whether any admitted writes still need to reach staging.
    pub(in crate::chunk) fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }

    /// Issues writes before migration slices copy their ranges, then retains all scratch capacity.
    pub(in crate::chunk) fn issue(&mut self, arena: &ChunkGpuArena, uploads: TerrainUploads<'_>) {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("terrain.arena_upload", chunks = self.chunks).entered();
        for (stream, buffer) in [
            (ArenaStream::Quads, &arena.quad_buffer),
            (ArenaStream::Origins, &arena.origin_buffer),
            (ArenaStream::GeometryStreams, &arena.geometry_stream_buffer),
            (ArenaStream::Biomes, &arena.biome_buffer),
        ] {
            self.order.clear();
            self.order.extend(
                self.writes
                    .iter()
                    .enumerate()
                    .filter_map(|(i, write)| (write.part.destination().0 == stream).then_some(i)),
            );
            if self.order.is_empty() {
                continue;
            }
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!(
                "terrain.merge_upload_ranges",
                ranges = self.order.len(),
                bytes = self
                    .order
                    .iter()
                    .map(|&i| self.writes[i].payload.bytes(&self.encoded).len())
                    .sum::<usize>(),
            )
            .entered();
            self.order
                .sort_unstable_by_key(|&i| (self.writes[i].offset, i));
            let overlapping = self.order.windows(2).any(|pair| {
                let a = &self.writes[pair[0]];
                a.offset + a.payload.bytes(&self.encoded).len() as u64 > self.writes[pair[1]].offset
            });
            if overlapping {
                self.order
                    .sort_unstable_by_key(|&i| (self.writes[i].part, i));
            }
            let mut writes: Vec<BufferWrite<'_>> = self
                .order
                .iter()
                .map(|&i| {
                    let write = &self.writes[i];
                    (&**buffer, write.offset, write.payload.bytes(&self.encoded))
                })
                .collect();
            if let Some(migration) = arena
                .migration
                .as_ref()
                .filter(|migration| migration.stream == stream)
            {
                for write in &mut writes {
                    write.0 = &migration.buffer;
                }
                write_merged(uploads, &writes, overlapping);
                for write in &mut writes {
                    write.0 = buffer;
                }
            }
            write_merged(uploads, &writes, overlapping);
        }
        self.writes.clear();
        self.encoded.clear();
        self.order.clear();
        self.chunks = 0;
    }
}

/// Borrows the native word representation of immutable cube records without repacking.
fn quad_bytes(records: &[PackedQuad]) -> &[u8] {
    const {
        assert!(std::mem::size_of::<PackedQuad>() == std::mem::size_of::<[u32; 2]>());
    }
    // SAFETY: PackedQuad is repr(C), with two initialized u32 fields and no padding.
    // The view keeps the shared borrow's lifetime and never mutates its source.
    unsafe { std::slice::from_raw_parts(records.as_ptr().cast(), std::mem::size_of_val(records)) }
}

/// Borrows the native sample representation of immutable lighting records without repacking.
fn lighting_bytes(records: &[PackedQuadLighting]) -> &[u8] {
    const {
        assert!(std::mem::size_of::<PackedQuadLighting>() == std::mem::size_of::<[u16; 4]>());
    }
    // SAFETY: PackedQuadLighting is repr(transparent) over four initialized u16 samples.
    // There is no padding, and the view keeps the shared borrow's lifetime.
    unsafe { std::slice::from_raw_parts(records.as_ptr().cast(), std::mem::size_of_val(records)) }
}

/// Uses the retained pool first and merges only the unstaged suffix on the queue path.
fn write_merged(uploads: TerrainUploads<'_>, writes: &[BufferWrite<'_>], overlapping: bool) {
    uploads.write(writes, |rest, site| {
        queue_merged(uploads.queue, rest, overlapping, site)
    });
}

/// Writes abutting runs together, preserving original write order whenever ranges overlap.
fn queue_merged(
    queue: &RenderQueue,
    order: &[BufferWrite<'_>],
    overlapping: bool,
    site: &'static std::panic::Location<'static>,
) {
    if overlapping {
        for &(buffer, offset, bytes) in order {
            #[cfg(feature = "tracy")]
            let _span =
                bevy::log::info_span!("terrain.arena_write", offset, bytes = bytes.len()).entered();
            queue.profiled_write_buffer_at(buffer, offset, bytes, site);
        }
        return;
    }
    for run in abutting_runs(order) {
        let run = &order[run];
        let (buffer, offset, _) = run[0];
        if let [(_, _, bytes)] = run {
            #[cfg(feature = "tracy")]
            let _span =
                bevy::log::info_span!("terrain.arena_write", offset, bytes = bytes.len()).entered();
            queue.profiled_write_buffer_at(buffer, offset, bytes, site);
            continue;
        }
        let size = run.iter().map(|write| write.2.len() as u64).sum();
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "terrain.arena_write_merged",
            offset,
            bytes = size,
            ranges = run.len()
        )
        .entered();
        if let Some(size) = NonZeroU64::new(size)
            && let Some(mut view) = queue.profiled_write_buffer_with_at(buffer, offset, size, site)
        {
            let mut at = 0;
            for &(_, _, bytes) in run {
                view.slice(at..at + bytes.len()).copy_from_slice(bytes);
                at += bytes.len();
            }
        }
    }
}

/// Iterates abutting byte ranges without allocating another descriptor vector.
fn abutting_runs<'a>(sorted: &'a [BufferWrite<'_>]) -> impl Iterator<Item = Range<usize>> + 'a {
    let mut start = 0;
    std::iter::from_fn(move || {
        if start == sorted.len() {
            return None;
        }
        let first = start;
        let mut next = sorted[start].1 + sorted[start].2.len() as u64;
        start += 1;
        while start < sorted.len() && sorted[start].1 == next {
            next += sorted[start].2.len() as u64;
            start += 1;
        }
        Some(first..start)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upload_staging::TerrainUploadStaging;

    /// Merged runs write exactly the bytes the individual writes would, in any issue order.
    #[test]
    fn abutting_runs_reproduce_individual_writes() {
        let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 1024,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let records: Vec<Vec<u8>> = (0..12_u8)
            .map(|seed| vec![seed; 4 + 4 * (seed as usize % 3)])
            .collect();
        let mut offset = 0;
        let mut writes = Vec::new();
        for (index, bytes) in records.iter().enumerate() {
            writes.push((&buffer, offset, bytes.as_slice()));
            // Leave a gap after every third record so runs split.
            offset += bytes.len() as u64 + if index % 3 == 2 { 8 } else { 0 };
        }
        writes.reverse();
        let apply = |buffer: &mut Vec<u8>, at: u64, bytes: &[u8]| {
            buffer[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
        };
        let mut expected = vec![0; offset as usize];
        for &(_, at, bytes) in &writes {
            apply(&mut expected, at, bytes);
        }
        let mut sorted = writes.clone();
        sorted.sort_by_key(|(_, at, _)| *at);
        let runs: Vec<_> = abutting_runs(&sorted).collect();
        assert_eq!(runs.len(), 4);
        let mut merged = vec![0; offset as usize];
        for run in runs {
            let joined: Vec<u8> = sorted[run.clone()]
                .iter()
                .flat_map(|(_, _, bytes)| bytes.iter().copied())
                .collect();
            apply(&mut merged, sorted[run.start].1, &joined);
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
        let mut writes = ArenaWrites::default();
        writes.encode(Part::Quads, 16, std::iter::repeat_n([frame, 2], 16));
        writes.encode(Part::Quads, 0, std::iter::repeat_n([frame, 1], 16));
        writes.encode(Part::Quads, 40, std::iter::repeat_n([frame, 3], 8));
        writes
    }

    /// Ends a frame as the graph prefix and device poll do, so completed slots return.
    pub(super) fn end_frame(
        device: &RenderDevice,
        queue: &RenderQueue,
        staging: &TerrainUploadStaging,
    ) {
        let mut encoder = device.create_command_encoder(&Default::default());
        staging.encode_pending(&mut encoder);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::Poll).unwrap();
    }

    /// Copies the first `bytes` of `buffer` out through a mappable buffer.
    pub(super) fn read(
        device: &RenderDevice,
        queue: &RenderQueue,
        buffer: &Buffer,
        bytes: u64,
    ) -> Vec<u8> {
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

    /// Checks the original caller and suffix length exposed to a custom fallback.
    #[track_caller]
    fn assert_fallback_origin(
        uploads: TerrainUploads<'_>,
        writes: &[BufferWrite<'_>],
        expected_bytes: usize,
    ) {
        let expected_site = std::panic::Location::caller();
        let mut observed = None;
        uploads.write(writes, |rest, site| {
            observed = Some((site, rest.iter().map(|write| write.2.len()).sum::<usize>()));
        });
        assert_eq!(observed, Some((expected_site, expected_bytes)));
    }

    /// Direct and partially staged terrain writes preserve their owner through the callback.
    #[test]
    fn terrain_fallback_retains_the_original_caller_and_unstaged_suffix() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
        let staging = TerrainUploadStaging::new(&device);
        let tail = wgpu::COPY_BUFFER_ALIGNMENT as usize;
        let bytes = vec![1_u8; staging.capacity_bytes() as usize + tail];
        let target = create_storage_buffer(
            &device,
            "caller-labelled terrain upload",
            bytes.len() as u64,
        );
        let writes = [(&*target, 0, bytes.as_slice())];
        let uploads = TerrainUploads {
            device: &device,
            queue: &queue,
            staging: None,
        };
        assert_fallback_origin(uploads, &writes, bytes.len());
        assert_fallback_origin(
            TerrainUploads {
                staging: Some(&staging),
                ..uploads
            },
            &writes,
            tail,
        );
        assert_eq!(staging.stats().staged_bytes as usize + tail, bytes.len());
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
        let mut writes = ArenaWrites::default();
        writes.encode(Part::Quads, half, words[half as usize..].iter().copied());
        writes.encode(Part::Quads, 0, words[..half as usize].iter().copied());
        writes.issue(&arena, uploads);
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
        assert!(begin_arena_migration(
            &mut arena,
            &device,
            ArenaStream::Quads,
            ArenaGrowthPlan {
                new_capacity: 1024,
                gpu_copy_bytes: copy_bytes,
            },
        ));
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

    /// A write to an already-copied prefix is retained when later slices finish the migration.
    #[test]
    fn writes_during_migration_update_both_copied_and_uncopied_ranges() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
        let (staging, mut arena) = staged_arena(&device);
        let uploads = TerrainUploads {
            device: &device,
            queue: &queue,
            staging: Some(&staging),
        };
        assert!(begin_arena_migration(
            &mut arena,
            &device,
            ArenaStream::Quads,
            ArenaGrowthPlan {
                new_capacity: 1024,
                gpu_copy_bytes: 512 * PACKED_QUAD_BYTES,
            },
        ));
        assert_eq!(advance_arena_migration(&mut arena, uploads, 128), 128);
        quad_writes(9).issue(&arena, uploads);
        assert_eq!(
            advance_arena_migration(&mut arena, uploads, u64::MAX),
            512 * PACKED_QUAD_BYTES - 128
        );
        end_frame(&device, &queue, &staging);
        let bytes = read(&device, &queue, &arena.quad_buffer, 32 * PACKED_QUAD_BYTES);
        let expected: Vec<[u32; 2]> = [[9, 1]; 16].into_iter().chain([[9, 2]; 16]).collect();
        assert_eq!(bytes, bytemuck::cast_slice::<[u32; 2], u8>(&expected));
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

#[cfg(test)]
#[path = "arena_writes_tests.rs"]
mod staging_tests;
