//! CPU meshing and publication payload comparisons, independent of installed game assets.

#![expect(
    clippy::print_stderr,
    reason = "benchmark preflight reports actual admitted work"
)]

use chunk_pipeline::benchmark_support::DispatchFixture;
use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use render_api::terrain_distance::VERTICAL_DISTANCE_UNLIMITED;
use std::{hint::black_box, time::Duration};

const SIDE: usize = 8;

/// Compares identical tall terrain with unlimited height and two optional vertical radii.
fn vertical_distance_benches(c: &mut Criterion) {
    let height = client_world::ingestion::vanilla_dimension_range(0)
        .unwrap()
        .sub_chunk_count;
    let mut group = c.benchmark_group("pipeline/vertical_distance_mesh");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(3));
    for radius in [VERTICAL_DISTANCE_UNLIMITED, 4, 2] {
        let mut fixture = DispatchFixture::vertical_grid(SIDE);
        fixture.set_vertical_distance(radius);
        let (sections, bytes) = fixture.mesh_admitted();
        let expected_height = if radius == VERTICAL_DISTANCE_UNLIMITED {
            height
        } else {
            (2 * radius as usize + 1).min(height)
        };
        assert_eq!(sections, SIDE * SIDE * expected_height);
        assert!(bytes > 0, "the benchmark must generate real geometry");
        eprintln!(
            "vertical_distance radius={radius} sections={sections} publication_bytes={bytes}"
        );
        group.bench_function(BenchmarkId::from_parameter(radius), |b| {
            b.iter(|| black_box(fixture.mesh_admitted()));
        });
    }
    group.finish();

    let mut crossing = c.benchmark_group("pipeline/vertical_distance_crossing");
    crossing.sample_size(10);
    crossing.warm_up_time(Duration::from_millis(100));
    crossing.measurement_time(Duration::from_secs(1));
    crossing.bench_function("4", |b| {
        b.iter_batched_ref(
            || {
                let mut fixture = DispatchFixture::vertical_grid(SIDE);
                fixture.set_vertical_distance(4);
                fixture
            },
            DispatchFixture::cross_vertical_band,
            BatchSize::PerIteration,
        );
    });
    crossing.finish();
}

criterion_group!(benches, vertical_distance_benches);
criterion_main!(benches);
