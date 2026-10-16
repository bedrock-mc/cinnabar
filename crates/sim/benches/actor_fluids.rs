//! Body and breathing material queries for a crowd of small actors.
#[path = "../tests/fixtures/actor_fluids.rs"]
mod fixture;

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sim::{PaletteWorld, Vec3, sample_actor_liquid_submersion, sample_actor_liquids};

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

const ACTORS: u64 = 128;

/// Samples both probes for every actor, retaining the results for the optimizer.
fn sample_crowd(world: &PaletteWorld<'_>) {
    for _ in 0..ACTORS {
        black_box(sample_actor_liquids(world, black_box(fixture::body())).unwrap());
        black_box(
            sample_actor_liquid_submersion(world, black_box(Vec3::new(0.5, 0.61, 0.5))).unwrap(),
        );
    }
}

/// Measures steady body and breathing work without fixture construction.
#[expect(clippy::print_stderr, reason = "reports benchmark allocation counts")]
fn actor_fluid_benches(criterion: &mut Criterion) {
    let (store, registry) = fixture::world();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let (_, allocations) = test_alloc::measure(|| sample_crowd(&world));
    eprintln!("actor_fluids/{ACTORS}: {allocations:?}");
    let mut group = criterion.benchmark_group("actor_fluids");
    group.throughput(Throughput::Elements(ACTORS));
    group.bench_function("body_and_breathing", |bencher| {
        bencher.iter(|| sample_crowd(black_box(&world)));
    });
    group.finish();
}

criterion_group!(benches, actor_fluid_benches);
criterion_main!(benches);
