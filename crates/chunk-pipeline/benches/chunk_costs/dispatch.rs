use std::hint::black_box;

use chunk_pipeline::benchmark_support::DispatchFixture;
use criterion::{BenchmarkId, Criterion, Throughput};

#[path = "../../src/stream/tests/allocation_count.rs"]
mod allocation_count;

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
            let before = allocation_count::thread_allocations();
            capture(&fixture);
            let allocations = allocation_count::thread_allocations() - before;
            eprintln!("DISPATCH_INPUTS kind={name} jobs={count} allocations={allocations}");
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::new(name, count), &fixture, |b, fixture| {
                b.iter(|| capture(black_box(fixture)));
            });
        }
    }
    group.finish();
}
