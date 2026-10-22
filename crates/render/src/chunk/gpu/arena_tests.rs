use super::*;

fn fragmented_arena(holes: u32) -> ChunkGpuArena {
    let (device, _queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut arena = ChunkGpuArena::new(&RenderDevice::from(device));
    arena.free_quads = (0..holes).map(|hole| hole * 8..hole * 8 + 3).collect();
    arena.free_geometry_stream_words = (0..holes)
        .map(|hole| hole * 64 + 5..hole * 64 + 30)
        .collect();
    arena.free_biomes = (0..holes).map(|hole| hole * 8..hole * 8 + 2).collect();
    arena.quad_len = holes as usize * 8;
    arena.geometry_stream_len = holes as usize * 64;
    arena.biome_len = holes as usize * 8;
    arena
}

fn holes(free: &FreeRanges) -> Vec<Range<u32>> {
    free.iter().collect()
}

/// Probing in place must choose exactly what the copying planner chose.
#[test]
fn fresh_range_probe_matches_the_copying_planner_on_fragmented_lists() {
    let counts = [(2, 3, 1), (3, 6, 2), (9, 40, 5), (0, 0, 0)];
    for (cube, liquid, biome) in counts {
        let mut arena = fragmented_arena(64);
        let stream_counts = GeometryStreamCounts {
            cube,
            cube_lighting: cube,
            liquid,
            liquid_lighting: liquid,
            ..GeometryStreamCounts::default()
        };
        let copied = plan_chunk_range_update(
            arena.quad_len,
            &holes(&arena.free_quads),
            arena.geometry_stream_len,
            &holes(&arena.free_geometry_stream_words),
            arena.biome_len,
            &holes(&arena.free_biomes),
            stream_counts,
            biome,
            None,
            false,
            arena.limits,
        )
        .unwrap();
        let probed = plan_fresh_chunk_ranges(&arena, stream_counts, biome).unwrap();
        assert_eq!(
            (
                probed.quad_start,
                probed.geometry_stream_start,
                probed.biome_start
            ),
            (
                copied.quad_start,
                copied.geometry_stream_start,
                copied.biome_start
            )
        );
        assert_eq!(
            (probed.liquid_start, probed.cube_lighting_start),
            (copied.liquid_start, copied.cube_lighting_start)
        );
        commit_fresh_chunk_ranges(&mut arena, &probed);
        assert_eq!(arena.free_quads, copied.free_quads);
        assert_eq!(
            arena.free_geometry_stream_words,
            copied.free_geometry_stream_words
        );
        assert_eq!(arena.free_biomes, copied.free_biomes);
        assert_eq!(
            (arena.quad_len, arena.geometry_stream_len, arena.biome_len),
            (
                copied.quad_len,
                copied.geometry_stream_len,
                copied.biome_len
            )
        );
    }
}

/// Run: `cargo test -p render --lib arena_planning_cost -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark"]
fn arena_planning_cost_with_4096_holes() {
    let arena = fragmented_arena(4_096);
    let counts = GeometryStreamCounts {
        cube: 64,
        cube_lighting: 64,
        ..GeometryStreamCounts::default()
    };
    let plans = 512;
    let (quads, geometry, biomes) = (
        holes(&arena.free_quads),
        holes(&arena.free_geometry_stream_words),
        holes(&arena.free_biomes),
    );
    let started = Instant::now();
    for _ in 0..plans {
        std::hint::black_box(plan_chunk_range_update(
            arena.quad_len,
            &quads,
            arena.geometry_stream_len,
            &geometry,
            arena.biome_len,
            &biomes,
            counts,
            4,
            None,
            false,
            arena.limits,
        ));
    }
    let old = started.elapsed();
    let started = Instant::now();
    for _ in 0..plans {
        std::hint::black_box(plan_fresh_chunk_ranges(&arena, counts, 4));
    }
    let new = started.elapsed();
    eprintln!(
        "FRAME_COST arena_planning_{plans}_items_4096_holes: old={:.3}ms new={:.3}ms",
        old.as_secs_f64() * 1e3,
        new.as_secs_f64() * 1e3
    );
}
