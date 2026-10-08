use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use world::{
    BLOCKS_PER_SUB_CHUNK, BlockPos, ChunkKey, ChunkStore, DecodedLevelChunk, DimensionLightProfile,
    DimensionSlots, EmptyLight, LightBlockAccess, LightBlockSample, LightBounds, LightChannel,
    LightProperties, LightSolverScratch, RawBiomeIds, RawBlockIds, SolverLimits, SubChunk,
    SubChunkKey, solve_light, solve_light_with_scratch,
};

const IDS: RawBlockIds = RawBlockIds { air: 0 };
const BIOMES: RawBiomeIds = RawBiomeIds { default_biome: 0 };

fn push_var_i32(bytes: &mut Vec<u8>, value: i32) {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            break;
        }
    }
}

// The v9 network palette layouts match tests/it/sub_chunk.rs: uniform storage
// omits the palette count; packed storage includes it after little-endian words.
fn sub_chunk_wire(y: i8, mixed: bool) -> Vec<u8> {
    let mut bytes = vec![9, 1, y as u8];
    if mixed {
        bytes.push(3); // One bit per index, network palette.
        for _ in 0..128 {
            bytes.extend_from_slice(&0xaaaa_aaaa_u32.to_le_bytes());
        }
        push_var_i32(&mut bytes, 2);
        push_var_i32(&mut bytes, 20);
        push_var_i32(&mut bytes, 30);
    } else {
        bytes.push(1); // Zero bits per index, network palette.
        push_var_i32(&mut bytes, 20);
    }
    bytes
}

fn check_sub_chunk(decoded: &SubChunk, y: i8, mixed: bool) {
    assert_eq!(decoded.version(), 9);
    assert_eq!(decoded.y_index(), Some(y));
    assert_eq!(decoded.storages().len(), 1);
    let storage = &decoded.storages()[0];
    assert_eq!(storage.bits_per_index(), u8::from(mixed));
    assert_eq!(storage.palette().len(), if mixed { 2 } else { 1 });
    assert_eq!(storage.packed_words().len(), if mixed { 128 } else { 0 });
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..16 {
                let expected = if mixed && y % 2 == 1 { 30 } else { 20 };
                assert_eq!(decoded.runtime_id(0, x, y, z), Some(expected));
            }
        }
    }
}

fn decode_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("world/sub_chunk_decode");
    for (name, mixed) in [("uniform_0bit", false), ("mixed_1bit", true)] {
        let payload = sub_chunk_wire(-4, mixed);
        let (decoded, consumed) = SubChunk::decode_prefix(&payload, &IDS);
        assert_eq!(consumed, payload.len());
        check_sub_chunk(&decoded, -4, mixed);
        group.throughput(Throughput::Bytes(payload.len() as u64));
        group.bench_with_input(
            BenchmarkId::new(name, "4096_blocks"),
            &payload,
            |b, bytes| {
                // Decoder allocations and owned-output destruction are both timed.
                b.iter(|| {
                    drop(black_box(SubChunk::decode(
                        black_box(bytes.as_slice()),
                        &IDS,
                    )))
                });
            },
        );
    }
    group.finish();

    let mut group = c.benchmark_group("world/inline_column_decode");
    for count in [4_usize, 24] {
        let slots = DimensionSlots {
            base_sub_chunk_y: -4,
            count,
        };
        let chunk = ChunkKey::new(0, 0, 0);
        let mut payload = Vec::new();
        for index in 0..count {
            payload.extend(sub_chunk_wire((index as i32 - 4) as i8, true));
        }
        let block_bytes = payload.len();
        for _ in 0..count {
            payload.push(1); // Uniform network biome storage.
            push_var_i32(&mut payload, 7);
        }
        payload.push(0); // Zero border blocks, followed by an empty entity tail.
        let decoded =
            DecodedLevelChunk::decode_inline(chunk, slots, count, &payload, &IDS, &BIOMES);
        assert_eq!(decoded.bytes_consumed(), payload.len());
        assert_eq!(decoded.block_bytes_consumed(), block_bytes);
        assert_eq!(decoded.sub_chunks().len(), count);
        for (y, section) in decoded.sub_chunks() {
            check_sub_chunk(&section, y as i8, true);
        }
        // Commit only validates the decoded biome column; it is not measured.
        let mut store = ChunkStore::new();
        store
            .commit_level_chunk(chunk, decoded)
            .expect("valid inline column");
        for index in 0..count {
            let key = SubChunkKey::from_chunk(chunk, index as i32 - 4);
            assert_eq!(store.biome_id(key, 0, 0, 0), Some(7));
            assert_eq!(store.biome_id(key, 15, 15, 15), Some(7));
        }
        group.throughput(Throughput::Bytes(payload.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("mixed_1bit_sections", count),
            &payload,
            |b, bytes| {
                // Includes full block/biome/tail decoding, allocations and output drop;
                // fixture construction and committing to the world store are excluded.
                b.iter(|| {
                    drop(black_box(DecodedLevelChunk::decode_inline(
                        chunk,
                        slots,
                        count,
                        black_box(bytes.as_slice()),
                        &IDS,
                        &BIOMES,
                    )));
                });
            },
        );
    }
    group.finish();
}

struct LightFixture {
    bounds: LightBounds,
    source: Option<LightProperties>,
}

impl LightBlockAccess for LightFixture {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        if !self.bounds.contains(position) {
            LightBlockSample::Unknown
        } else if position == BlockPos::new(8, 8, 8) {
            self.source
                .map_or(LightBlockSample::KnownAir, LightBlockSample::Resident)
        } else {
            LightBlockSample::KnownAir
        }
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        if self.source.is_none() && self.bounds.contains(position) && position.y == 15 {
            15
        } else {
            0
        }
    }
}

fn light_benches(c: &mut Criterion) {
    let bounds = LightBounds::new(0, BlockPos::new(0, 0, 0), BlockPos::new(15, 15, 15))
        .expect("valid light bounds");
    let limits = SolverLimits::new(BLOCKS_PER_SUB_CHUNK, 1_000_000);
    let mut group = c.benchmark_group("world/solve_light");
    group.throughput(Throughput::Elements(BLOCKS_PER_SUB_CHUNK as u64));
    for (name, source, profile) in [
        (
            "air_top_sky",
            None,
            DimensionLightProfile::Overworld {
                direct_sky_down: true,
            },
        ),
        (
            "emitting_source",
            Some(LightProperties::new(15, 0).unwrap()),
            DimensionLightProfile::Nether,
        ),
    ] {
        let fixture = LightFixture { bounds, source };
        let output = solve_light(&fixture, &EmptyLight, bounds, 73, profile, limits)
            .expect("valid light solve fixture");
        assert_eq!(output.sub_chunks().len(), 1);
        assert_eq!(
            output.sub_chunks()[&SubChunkKey::new(0, 0, 0, 0)].generation(),
            73
        );
        assert!(output.stats().increase_dequeued > 0);
        for x in 0_i32..16 {
            for y in 0_i32..16 {
                for z in 0_i32..16 {
                    let position = BlockPos::new(x, y, z);
                    let distance = (x - 8).abs() + (y - 8).abs() + (z - 8).abs();
                    let block = if source.is_some() {
                        (15 - distance).max(0) as u8
                    } else {
                        0
                    };
                    let sky = if source.is_none() { 15 } else { 0 };
                    assert_eq!(output.light_at(position, LightChannel::Block), block);
                    assert_eq!(output.light_at(position, LightChannel::Sky), sky);
                }
            }
        }
        group.bench_with_input(
            BenchmarkId::new(name, "16x16x16_voxels"),
            &fixture,
            |b, fixture| {
                // Fresh solves from EmptyLight include solver allocation, packing and
                // destruction of the owned output, not fixture setup or validation.
                b.iter(|| {
                    drop(black_box(
                        solve_light(black_box(fixture), &EmptyLight, bounds, 73, profile, limits)
                            .expect("light solve"),
                    ));
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new(format!("{name}_reused"), "16x16x16_voxels"),
            &fixture,
            |b, fixture| {
                let mut scratch = LightSolverScratch::default();
                drop(
                    solve_light_with_scratch(
                        fixture,
                        &EmptyLight,
                        bounds,
                        73,
                        profile,
                        limits,
                        &mut scratch,
                    )
                    .expect("warm light solve"),
                );
                // Each solve still samples new input and packs an independently owned output.
                b.iter(|| {
                    drop(black_box(
                        solve_light_with_scratch(
                            black_box(fixture),
                            &EmptyLight,
                            bounds,
                            73,
                            profile,
                            limits,
                            &mut scratch,
                        )
                        .expect("light solve"),
                    ));
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, decode_benches, light_benches);
criterion_main!(benches);
