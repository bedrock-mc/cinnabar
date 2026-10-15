use chunk_pipeline::benchmark_support::RetentionFixture;
use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use std::time::Duration;

/// Measures only the crossing; fixture construction and retirement drops are untimed.
fn retention_benches(c: &mut Criterion) {
    for (name, fixture) in [
        (
            "pipeline/retention_boundary",
            RetentionFixture::new as fn(i32) -> RetentionFixture,
        ),
        (
            "pipeline/retention_connected_boundary",
            RetentionFixture::with_connectivity,
        ),
    ] {
        let mut group = c.benchmark_group(name);
        group.sample_size(10);
        group.warm_up_time(Duration::from_millis(100));
        group.measurement_time(Duration::from_secs(1));
        for radius in [16, 64, 128] {
            group.bench_function(BenchmarkId::from_parameter(radius), |b| {
                b.iter_batched_ref(
                    || fixture(radius),
                    RetentionFixture::cross,
                    BatchSize::PerIteration,
                );
            });
        }
        group.finish();
    }
}

criterion_group!(benches, retention_benches);
criterion_main!(benches);
