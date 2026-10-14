use std::hint::black_box;

use chunk_pipeline::benchmark_support::LodSampleFixture;
use criterion::{BenchmarkId, Criterion, Throughput};

/// Times one distant terrain sample job's capture, content hash, level-0 sample and surface
/// signature; the store's hash check and the writer's merge are not included.
pub fn benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline/lod_sample_job");
    for sub_chunks in [4_usize, 24] {
        let fixture = LodSampleFixture::new(sub_chunks);
        assert_eq!(fixture.sample().1, sub_chunks);
        group.throughput(Throughput::Elements(sub_chunks as u64));
        group.bench_with_input(
            BenchmarkId::new("column", sub_chunks),
            &fixture,
            |b, fixture| b.iter(|| black_box(fixture.sample())),
        );
    }
    group.finish();
}
