use std::hint::black_box;

use chunk_pipeline::benchmark_support::DispatchFixture;
use criterion::{BenchmarkId, Criterion, Throughput};

/// Isolates job input capture and destruction from queue waits, solving and meshing.
pub fn benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline/dispatch_inputs");
    for count in [4_usize, 16, 64, 871] {
        let fixture = DispatchFixture::new(count);
        for (name, capture) in [
            (
                "light",
                DispatchFixture::capture_light as fn(&DispatchFixture),
            ),
            (
                "mesh",
                DispatchFixture::capture_mesh as fn(&DispatchFixture),
            ),
        ] {
            capture(&fixture);
            #[cfg(not(feature = "runtime-profile"))]
            let before = test_alloc::thread_allocations();
            capture(&fixture);
            #[cfg(not(feature = "runtime-profile"))]
            {
                let allocations = test_alloc::thread_allocations() - before;
                eprintln!("DISPATCH_INPUTS kind={name} jobs={count} allocations={allocations}");
            }
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::new(name, count), &fixture, |b, fixture| {
                b.iter(|| capture(black_box(fixture)));
            });
        }
    }
    group.finish();
}

/// Compares identical column snapshot inputs with and without shared ceiling preparation.
pub fn light_column_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline/light_column_inputs");
    for count in [4_usize, 16, 24, 32] {
        let fixture = DispatchFixture::column(count);
        group.throughput(Throughput::Elements(count as u64));
        for (name, capture) in [
            (
                "uncached",
                DispatchFixture::capture_light_column_uncached as fn(&DispatchFixture),
            ),
            (
                "shared",
                DispatchFixture::capture_light_column_shared as fn(&DispatchFixture),
            ),
        ] {
            group.bench_with_input(BenchmarkId::new(name, count), &fixture, |b, fixture| {
                b.iter(|| capture(black_box(fixture)));
            });
        }
    }
    group.finish();
}
