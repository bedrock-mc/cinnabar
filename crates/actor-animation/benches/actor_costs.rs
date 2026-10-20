use std::{hint::black_box, time::Duration};

use actor_animation::test_support::CompositionFixture;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

/// Compares identical nonzero poses, including validation and excluding plan construction.
fn actor_costs(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("actor_costs/composition");
    group
        .sample_size(30)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2));
    for count in [17, 64] {
        let mut fixture = CompositionFixture::new(count);
        assert_eq!(fixture.recursive(), fixture.flat());
        group.bench_with_input(BenchmarkId::new("recursive", count), &count, |bench, _| {
            bench.iter(|| black_box(fixture.recursive()));
        });
        group.bench_with_input(BenchmarkId::new("flat", count), &count, |bench, _| {
            bench.iter(|| {
                black_box(fixture.flat());
            });
        });
    }
    group.finish();
}

criterion_group!(benches, actor_costs);
criterion_main!(benches);
