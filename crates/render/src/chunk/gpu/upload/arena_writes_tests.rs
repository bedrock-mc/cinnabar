use super::*;
use crate::chunk::gpu::arena::{ChunkRangePlan, plan_chunk_range_update};
use render_scene::{ChunkRenderInstance, ChunkUploadPriority};

/// Exercises every terrain stream with nonzero relocation bases and distinct records.
fn mixed_chunk() -> (ChunkRenderInstance, FreshChunkRanges, GpuChunkOrigin) {
    let instance = ChunkRenderInstance {
        key: SubChunkKey::new(0, 0, 0, 0),
        cube_quads: (0..256)
            .map(|i| PackedQuad::new([i as u8 % 16, 2, 3], Face::PositiveY, 1, 1, i))
            .collect::<Vec<_>>()
            .into(),
        cube_lighting: vec![PackedQuadLighting::new([1, 2, 3, 4]); 256].into(),
        cube_layout: CubeQuadLayout::default(),
        model_refs: (0..64)
            .map(|i| PackedModelRef::new(i, i + 1, i * 2, !i))
            .collect::<Vec<_>>()
            .into(),
        model_lighting: vec![PackedQuadLighting::new([5, 6, 7, 8]); 128].into(),
        model_draw_refs: (0..64)
            .map(|i| PackedModelDrawRef::new(i, i + 2))
            .collect::<Vec<_>>()
            .into(),
        transparent_model_draw_refs: (0..32)
            .map(|i| PackedModelDrawRef::new(i, i + 3))
            .collect::<Vec<_>>()
            .into(),
        liquid_quads: (0..128)
            .map(|i| {
                PackedLiquidQuad::try_pack(
                    [i as u8 % 16, 4, 5],
                    Face::PositiveY,
                    [255; 4],
                    i,
                    i,
                    [0, 0],
                    false,
                )
                .unwrap()
            })
            .collect::<Vec<_>>()
            .into(),
        liquid_lighting: vec![PackedQuadLighting::new([9, 10, 11, 12]); 128].into(),
        has_depth_liquid: false,
        has_transparent_liquid: true,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::default(),
        generation: 1,
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
        origin: [16, 32, 48],
    };
    let plan = plan_chunk_range_update(
        16,
        &[],
        16,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        GeometryStreamCounts {
            cube: instance.cube_quads.len() as u32,
            cube_lighting: instance.cube_lighting.len() as u32,
            model: instance.model_refs.len() as u32,
            model_lighting: instance.model_lighting.len() as u32,
            model_draw: instance.model_draw_refs.len() as u32,
            transparent_model_draw: instance.transparent_model_draw_refs.len() as u32,
            liquid: instance.liquid_quads.len() as u32,
            liquid_lighting: instance.liquid_lighting.len() as u32,
        },
        instance.biome.words().len() as u32,
        None,
        false,
        ArenaLimits {
            max_quad_items: u32::MAX as usize,
            max_geometry_stream_words: u32::MAX as usize,
            max_origin_items: u32::MAX as usize,
            max_biome_words: u32::MAX as usize,
        },
    )
    .unwrap();
    let origin = gpu_chunk_origin(
        instance.origin,
        plan.biome_start,
        plan.quad_start,
        plan.cube_lighting_start,
        plan.liquid_start,
        plan.liquid_lighting_start,
    )
    .unwrap();
    (instance, fresh_ranges(&plan), origin)
}

/// Counts payload materialized by collection before it reaches mapped staging.
fn materialized_bytes(writes: &ArenaWrites) -> usize {
    writes.encoded.len()
}

#[test]
fn warm_mixed_terrain_collection_reuses_storage_and_borrows_unchanged_streams() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
    let (instance, plan, origin) = mixed_chunk();
    let arena = mixed_arena(&device, &expected_streams(&instance, &plan, 7, origin));
    let uploads = TerrainUploads {
        device: &device,
        queue: &queue,
        staging: None,
    };
    let mut writes = ArenaWrites::default();
    writes.collect_chunk(&instance, &plan, 7, instance.biome.words(), origin);
    writes.issue(&arena, uploads);
    let (_, allocations) = test_alloc::measure(|| {
        writes.collect_chunk(&instance, &plan, 7, instance.biome.words(), origin);
    });
    let materialized = materialized_bytes(&writes);
    let mesh_bytes = super::super::chunk_instance_upload_byte_len(&instance) as usize;
    let relocated = instance.model_refs.len() * PACKED_MODEL_REF_BYTES as usize
        + (instance.model_draw_refs.len() + instance.transparent_model_draw_refs.len())
            * PACKED_MODEL_DRAW_REF_BYTES as usize
        + instance.liquid_quads.len() * PACKED_LIQUID_QUAD_BYTES as usize
        + CHUNK_ORIGIN_BYTES as usize;
    eprintln!(
        "terrain collection: mesh_bytes={mesh_bytes} materialized_bytes={materialized} allocations={} allocated_bytes={}",
        allocations.calls, allocations.bytes
    );
    assert_eq!(
        materialized, relocated,
        "unchanged streams must reach staging without a payload copy"
    );
    assert_eq!(
        allocations.calls, 0,
        "warm collection must reuse its storage"
    );
}

/// Recreates the former word-vector encoder as an independent byte-level oracle.
fn expected_streams(
    instance: &ChunkRenderInstance,
    plan: &FreshChunkRanges,
    metadata: u32,
    origin: GpuChunkOrigin,
) -> [(ArenaStream, Vec<u8>); 4] {
    let mut quads = vec![0; plan.quad_len * PACKED_QUAD_BYTES as usize];
    let mut geometry = vec![0; plan.geometry_stream_len * GEOMETRY_STREAM_WORD_BYTES as usize];
    let mut origins = vec![0; (metadata as usize + 1) * CHUNK_ORIGIN_BYTES as usize];
    let mut biomes = vec![0; plan.biome_len * BIOME_WORD_BYTES as usize];
    let copy = |target: &mut [u8], offset: u64, bytes: &[u8]| {
        target[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes);
    };
    let cube_words: Vec<_> = instance.cube_quads.iter().map(PackedQuad::words).collect();
    copy(
        &mut quads,
        u64::from(plan.quad_start) * PACKED_QUAD_BYTES,
        bytemuck::cast_slice(&cube_words),
    );
    let mut model_words: Vec<_> = instance.model_refs.iter().map(|v| v.words()).collect();
    super::super::absolutize_model_lighting_bases(&mut model_words, plan.model_lighting_start);
    let mut draws: Vec<_> = instance.model_draw_refs.iter().map(|v| v.words()).collect();
    let mut transparent: Vec<_> = instance
        .transparent_model_draw_refs
        .iter()
        .map(|v| v.words())
        .collect();
    super::super::absolutize_partitioned_model_draw_refs(
        &mut draws,
        &mut transparent,
        plan.model_start,
    )
    .unwrap();
    let mut liquids: Vec<_> = instance.liquid_quads.iter().map(|v| v.words()).collect();
    stamp_liquid_metadata_index(&mut liquids, metadata);
    for (start, bytes) in [
        (
            plan.model_start,
            bytemuck::cast_slice(model_words.as_slice()),
        ),
        (
            plan.model_draw_start,
            bytemuck::cast_slice(draws.as_slice()),
        ),
        (
            plan.transparent_model_draw_start,
            bytemuck::cast_slice(transparent.as_slice()),
        ),
        (plan.liquid_start, bytemuck::cast_slice(liquids.as_slice())),
    ] {
        copy(
            &mut geometry,
            u64::from(start) * GEOMETRY_STREAM_WORD_BYTES,
            bytes,
        );
    }
    for (start, lighting) in [
        (plan.model_lighting_start, &instance.model_lighting),
        (plan.liquid_lighting_start, &instance.liquid_lighting),
        (plan.cube_lighting_start, &instance.cube_lighting),
    ] {
        let words = super::super::packed_lighting_records(lighting);
        copy(
            &mut geometry,
            u64::from(start) * GEOMETRY_STREAM_WORD_BYTES,
            bytemuck::cast_slice(&words),
        );
    }
    copy(
        &mut origins,
        u64::from(metadata) * CHUNK_ORIGIN_BYTES,
        bytemuck::bytes_of(&origin),
    );
    copy(
        &mut biomes,
        u64::from(plan.biome_start) * BIOME_WORD_BYTES,
        bytemuck::cast_slice(instance.biome.words()),
    );
    [
        (ArenaStream::Quads, quads),
        (ArenaStream::GeometryStreams, geometry),
        (ArenaStream::Origins, origins),
        (ArenaStream::Biomes, biomes),
    ]
}

/// Allocates exactly enough storage for the oracle, including gaps before each stream.
fn mixed_arena(device: &RenderDevice, expected: &[(ArenaStream, Vec<u8>)]) -> ChunkGpuArena {
    let mut arena = ChunkGpuArena::new(device);
    for (stream, bytes) in expected {
        let buffer = create_storage_buffer(device, "mixed terrain stream", bytes.len() as u64);
        match stream {
            ArenaStream::Quads => {
                arena.quad_buffer = buffer;
                arena.quad_capacity = bytes.len() / PACKED_QUAD_BYTES as usize;
            }
            ArenaStream::GeometryStreams => {
                arena.geometry_stream_buffer = buffer;
                arena.geometry_stream_capacity = bytes.len() / GEOMETRY_STREAM_WORD_BYTES as usize;
            }
            ArenaStream::Origins => {
                arena.origin_buffer = buffer;
                arena.origin_capacity = bytes.len() / CHUNK_ORIGIN_BYTES as usize;
            }
            ArenaStream::Biomes => {
                arena.biome_buffer = buffer;
                arena.biome_capacity = bytes.len() / BIOME_WORD_BYTES as usize;
            }
        }
    }
    arena
}

#[test]
fn mixed_streams_match_word_encoder_with_and_without_migration_and_staging() {
    use super::tests::{end_frame, read};
    use crate::upload_staging::TerrainUploadStaging;
    let (instance, plan, origin) = mixed_chunk();
    let expected = expected_streams(&instance, &plan, 7, origin);
    for retained in [false, true] {
        for migrating in
            std::iter::once(None).chain(expected.iter().map(|(stream, _)| Some(*stream)))
        {
            let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
            let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
            let staging = TerrainUploadStaging::new(&device);
            let uploads = TerrainUploads {
                device: &device,
                queue: &queue,
                staging: retained.then_some(&staging),
            };
            let mut arena = mixed_arena(&device, &expected);
            if let Some(stream) = migrating {
                let bytes = expected.iter().find(|(s, _)| *s == stream).unwrap().1.len() as u64;
                let stride = match stream {
                    ArenaStream::Quads => PACKED_QUAD_BYTES,
                    ArenaStream::GeometryStreams => GEOMETRY_STREAM_WORD_BYTES,
                    ArenaStream::Origins => CHUNK_ORIGIN_BYTES,
                    ArenaStream::Biomes => BIOME_WORD_BYTES,
                };
                assert!(begin_arena_migration(
                    &mut arena,
                    &device,
                    stream,
                    ArenaGrowthPlan {
                        new_capacity: (bytes / stride) as usize * 2,
                        gpu_copy_bytes: bytes,
                    },
                ));
                advance_arena_migration(&mut arena, uploads, 128);
            }
            let mut writes = ArenaWrites::default();
            writes.collect_chunk(&instance, &plan, 7, instance.biome.words(), origin);
            writes.issue(&arena, uploads);
            assert!(writes.is_empty());
            end_frame(&device, &queue, &staging);
            if let Some(migration) = &arena.migration {
                let bytes = &expected
                    .iter()
                    .find(|(s, _)| *s == migration.stream)
                    .unwrap()
                    .1;
                assert_eq!(
                    read(&device, &queue, &migration.buffer, bytes.len() as u64),
                    *bytes
                );
            }
            advance_arena_migration(&mut arena, uploads, u64::MAX);
            for (stream, bytes) in &expected {
                let buffer = match stream {
                    ArenaStream::Quads => &arena.quad_buffer,
                    ArenaStream::GeometryStreams => &arena.geometry_stream_buffer,
                    ArenaStream::Origins => &arena.origin_buffer,
                    ArenaStream::Biomes => &arena.biome_buffer,
                };
                assert_eq!(
                    read(&device, &queue, buffer, bytes.len() as u64),
                    *bytes,
                    "retained={retained} migration={migrating:?} stream={stream:?}"
                );
            }
        }
    }
}

#[test]
fn overlapping_ranges_preserve_original_stream_order_in_both_destinations() {
    use super::tests::{end_frame, read};
    use crate::upload_staging::TerrainUploadStaging;
    for retained in [false, true] {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
        let staging = TerrainUploadStaging::new(&device);
        let uploads = TerrainUploads {
            device: &device,
            queue: &queue,
            staging: retained.then_some(&staging),
        };
        let mut arena = ChunkGpuArena::new(&device);
        arena.geometry_stream_capacity = 8;
        arena.geometry_stream_buffer = create_storage_buffer(
            &device,
            "overlapping terrain records",
            arena.geometry_stream_capacity as u64 * GEOMETRY_STREAM_WORD_BYTES,
        );
        let new_capacity = arena.geometry_stream_capacity * 2;
        assert!(begin_arena_migration(
            &mut arena,
            &device,
            ArenaStream::GeometryStreams,
            ArenaGrowthPlan {
                new_capacity,
                gpu_copy_bytes: 0,
            },
        ));
        let mut writes = ArenaWrites::default();
        // Deliberately append in chunk order; stream order still places models before lights.
        writes.encode(Part::CubeLighting, 0, std::iter::once([3_u32; 4]));
        writes.encode(Part::Model, 2, std::iter::once([1_u32; 4]));
        writes.encode(Part::Model, 1, std::iter::once([2_u32; 4]));
        writes.issue(&arena, uploads);
        end_frame(&device, &queue, &staging);
        let expected = [3_u32, 3, 3, 3, 2, 1];
        for buffer in [
            &arena.geometry_stream_buffer,
            &arena.migration.as_ref().unwrap().buffer,
        ] {
            assert_eq!(
                read(
                    &device,
                    &queue,
                    buffer,
                    std::mem::size_of_val(&expected) as u64
                ),
                bytemuck::cast_slice::<u32, u8>(&expected)
            );
        }
    }
}

/// The fresh-allocation view of a planned update, which is what uploads consume.
fn fresh_ranges(plan: &ChunkRangePlan) -> FreshChunkRanges {
    FreshChunkRanges {
        quad_start: plan.quad_start,
        quad_capacity: plan.quad_capacity,
        geometry_stream_start: plan.geometry_stream_start,
        geometry_stream_capacity: plan.geometry_stream_capacity,
        model_start: plan.model_start,
        model_lighting_start: plan.model_lighting_start,
        model_draw_start: plan.model_draw_start,
        transparent_model_draw_start: plan.transparent_model_draw_start,
        liquid_start: plan.liquid_start,
        liquid_lighting_start: plan.liquid_lighting_start,
        cube_lighting_start: plan.cube_lighting_start,
        biome_start: plan.biome_start,
        biome_capacity: plan.biome_capacity,
        quad_len: plan.quad_len,
        geometry_stream_len: plan.geometry_stream_len,
        biome_len: plan.biome_len,
    }
}
