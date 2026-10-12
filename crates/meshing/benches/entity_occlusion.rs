use criterion::{Criterion, criterion_group, criterion_main};
use meshing::occlusion::OcclusionBudget;
use std::hint::black_box;

/// Measures a whole frame budget with clear sight or a solid wall in the same section.
fn entity_occlusion(c: &mut Criterion) {
    for wall in [false, true] {
        c.bench_function(
            if wall {
                "entity_occlusion/wall_frame"
            } else {
                "entity_occlusion/clear_frame"
            },
            |b| {
                b.iter(|| {
                    let mut budget = OcclusionBudget::default();
                    let mut hidden = 0;
                    for _ in 0..256 {
                        hidden += usize::from(budget.hides_box(
                            black_box([0.5, 4.5, 4.5]),
                            [12.0, 3.0, 3.0],
                            [13.0, 6.0, 6.0],
                            |cell, _| black_box(wall && cell[0] == 8),
                        ));
                    }
                    black_box((hidden, budget.remaining()))
                });
            },
        );
    }
}

criterion_group!(benches, entity_occlusion);
criterion_main!(benches);
