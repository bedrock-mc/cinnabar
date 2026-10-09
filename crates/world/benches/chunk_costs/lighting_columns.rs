use std::{hint::black_box, time::Instant};

use criterion::{BenchmarkId, Criterion, Throughput};
use world::{
    BLOCKS_PER_SUB_CHUNK, BlockPos, DimensionLightProfile, EmptyLight, LightBlockAccess,
    LightBlockSample, LightBounds, LightProperties, LightReadAccess, LightSolveStats,
    LightSolverScratch, SUB_CHUNK_SIDE, SolverLimits, solve_light_with_scratch,
};

const LONG_COLUMN_SECTIONS: usize = 24;
const QUEUE_WORK_PER_VOXEL: usize = 64;
const PROFILE: DimensionLightProfile = DimensionLightProfile::Overworld {
    direct_sky_down: true,
};

struct Column {
    bounds: LightBounds,
    mixed: bool,
}

impl LightBlockAccess for Column {
    /// Competing weak and strong emitters exercise repeated increases in mixed columns.
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        if !self.bounds.contains(position) {
            return LightBlockSample::Unknown;
        }
        if self.mixed {
            let pattern = ((position.x * 3) ^ position.y ^ (position.z * 5)) & 31;
            if pattern == 0 {
                return LightBlockSample::Resident(LightProperties::new(9, 2).unwrap());
            }
            if pattern == 1 {
                return LightBlockSample::Resident(LightProperties::new(15, 0).unwrap());
            }
        }
        LightBlockSample::KnownAir
    }

    /// Only the top layer contributes local sky; downward propagation fills the column.
    fn sky_seed(&self, position: BlockPos) -> u8 {
        if position.y == self.bounds.max().y {
            15
        } else {
            0
        }
    }
}

/// Measures whole-column solves with retained worker scratch and independently owned output.
pub fn benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("world/light_columns");
    for sections in [10, LONG_COLUMN_SECTIONS] {
        let volume = sections * BLOCKS_PER_SUB_CHUNK;
        let bounds = column_bounds(sections);
        let profile = PROFILE;
        let limits = SolverLimits::new(volume, volume * QUEUE_WORK_PER_VOXEL);
        group.throughput(Throughput::Elements(volume as u64));
        for (name, mixed) in [("sky", false), ("mixed", true)] {
            let fixture = Column { bounds, mixed };
            let mut scratch = LightSolverScratch::default();
            let prior = solve_light_with_scratch(
                &fixture,
                &EmptyLight,
                bounds,
                1,
                profile,
                limits,
                &mut scratch,
            )
            .unwrap();
            eprintln!("light_columns/{name}/{sections} fresh: {:?}", prior.stats());
            let retained = solve_light_with_scratch(
                &fixture,
                &prior,
                bounds,
                2,
                profile,
                limits,
                &mut scratch,
            )
            .unwrap();
            assert_eq!(prior.sub_chunks().len(), sections);
            for position in [bounds.min(), bounds.max()] {
                assert_eq!(
                    prior.light_at(position, world::LightChannel::Sky),
                    retained.light_at(position, world::LightChannel::Sky),
                );
            }
            eprintln!(
                "light_columns/{name}/{sections} retained: {:?}",
                retained.stats()
            );
            group.bench_function(BenchmarkId::new(format!("{name}_fresh"), sections), |b| {
                b.iter(|| {
                    black_box(
                        solve_light_with_scratch(
                            black_box(&fixture),
                            &EmptyLight,
                            bounds,
                            2,
                            profile,
                            limits,
                            &mut scratch,
                        )
                        .unwrap(),
                    );
                });
            });
            group.bench_function(
                BenchmarkId::new(format!("{name}_retained"), sections),
                |b| {
                    b.iter(|| {
                        black_box(
                            solve_light_with_scratch(
                                black_box(&fixture),
                                &prior,
                                bounds,
                                2,
                                profile,
                                limits,
                                &mut scratch,
                            )
                            .unwrap(),
                        );
                    });
                },
            );
        }
    }
    group.finish();
}

/// Constructs the same section-aligned column for Criterion and individual solve samples.
fn column_bounds(sections: usize) -> LightBounds {
    LightBounds::new(
        0,
        BlockPos::new(0, 0, 0),
        BlockPos::new(
            (SUB_CHUNK_SIDE - 1) as i32,
            (sections * SUB_CHUNK_SIDE - 1) as i32,
            (SUB_CHUNK_SIDE - 1) as i32,
        ),
    )
    .unwrap()
}

/// Times individual warm solves, including output destruction, without timing assertions or I/O.
fn sample_fixture<P: LightReadAccess>(
    fixture: &Column,
    prior: &P,
    limits: SolverLimits,
    scratch: &mut LightSolverScratch,
) -> (LightSolveStats, Vec<u128>) {
    const WARMUP_COUNT: usize = 8;
    const SAMPLE_COUNT: usize = 256;
    let mut durations = Vec::with_capacity(SAMPLE_COUNT);
    let mut stats = LightSolveStats::default();
    for iteration in 0..WARMUP_COUNT + SAMPLE_COUNT {
        let started = Instant::now();
        let output = solve_light_with_scratch(
            black_box(fixture),
            prior,
            fixture.bounds,
            2,
            PROFILE,
            limits,
            scratch,
        )
        .unwrap();
        stats = output.stats();
        drop(black_box(output));
        let elapsed = started.elapsed().as_nanos();
        if iteration >= WARMUP_COUNT {
            durations.push(elapsed);
        }
    }
    (stats, durations)
}

/// Emits fixed-count per-solve samples so medians and tails are not averages of timed batches.
pub fn samples() {
    let sections = LONG_COLUMN_SECTIONS;
    let bounds = column_bounds(sections);
    let volume = sections * BLOCKS_PER_SUB_CHUNK;
    let limits = SolverLimits::new(volume, volume * QUEUE_WORK_PER_VOXEL);
    let mut results = Vec::new();
    for (name, mixed) in [("sky", false), ("mixed", true)] {
        let fixture = Column { bounds, mixed };
        let mut scratch = LightSolverScratch::default();
        let prior = solve_light_with_scratch(
            &fixture,
            &EmptyLight,
            bounds,
            1,
            PROFILE,
            limits,
            &mut scratch,
        )
        .unwrap();
        for retained in [false, true] {
            let (stats, durations) = if retained {
                sample_fixture(&fixture, &prior, limits, &mut scratch)
            } else {
                sample_fixture(&fixture, &EmptyLight, limits, &mut scratch)
            };
            results.push((name, retained, stats, durations));
        }
    }
    println!("case,sections,iteration,nanoseconds");
    for (name, retained, stats, durations) in results {
        let state = if retained { "retained" } else { "fresh" };
        eprintln!("light_columns/{name}_{state}/{sections}: {stats:?}");
        for (iteration, duration) in durations.into_iter().enumerate() {
            println!("{name}_{state},{sections},{iteration},{duration}");
        }
    }
}
