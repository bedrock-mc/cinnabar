//! Reports a synthetic per-shape baseline alongside retained steady state and 10% churn.
use super::*;
use std::{hint::black_box, time::Instant};

/// Returns the median CPU frame sample in microseconds, without asserting clock time in CI.
fn measure(mut frame: impl FnMut()) -> f64 {
    let mut samples = [0.0_f64; 31];
    for sample in &mut samples {
        let started = Instant::now();
        frame();
        *sample = started.elapsed().as_secs_f64() * 1_000_000.0;
    }
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

#[test]
fn primitive_shapes_frame_cost_bench() {
    for count in [1_000, 10_000, 100_000] {
        let mut store = PrimitiveShapeStore::default();
        let mut baseline = Vec::with_capacity(count);
        for id in 0..count {
            apply(&mut store, update(id as u64, PrimitiveShapeKind::Sphere));
            baseline.push(PrimitiveState::new(PrimitiveShapeKind::Sphere));
        }
        flush(&mut store);
        let naive_steady = measure(|| {
            for state in &baseline {
                black_box(state.instance(u32::MAX));
            }
        });
        let retained_steady = measure(|| {
            black_box(flush(&mut store));
        });
        let mut frame = 0;
        let naive_churn = measure(|| {
            frame += 1;
            for (index, state) in baseline.iter_mut().enumerate() {
                if index % 10 == 0 {
                    state.location[0] = frame as f32;
                }
                black_box(state.instance(u32::MAX));
            }
        });
        let mut upload = PrimitiveUploadStats::default();
        let retained_churn = measure(|| {
            frame += 1;
            let changes = (0..count)
                .step_by(10)
                .map(|id| {
                    let mut patch = update(id as u64, PrimitiveShapeKind::Sphere);
                    patch.location = Some([frame as f32, 0.0, 0.0]);
                    PrimitiveShapeChange::Upsert(patch)
                })
                .collect();
            store.apply(PrimitiveShapesEvent {
                changes,
                skipped_entries: 0,
            });
            upload = flush(&mut store);
            black_box(upload);
        });
        assert_eq!(upload.slots, count / 10);
        println!(
            "PRIMITIVE_BENCH shapes={count} baseline_steady_us={naive_steady:.3} retained_steady_us={retained_steady:.3} baseline_churn_us={naive_churn:.3} retained_churn_us={retained_churn:.3} baseline_upload_bytes={} retained_steady_upload_bytes=0 retained_churn_upload_bytes={}",
            count * std::mem::size_of::<PrimitiveInstance>(),
            upload.bytes
        );
    }
}
