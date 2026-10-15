//! The cost of the log flight recorder through the installed subscriber: a `debug` event only
//! the recorder enables (its whole cost: filtering, formatting and the ring write into a mapped
//! staging file), beside a `trace` event no filter enables.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use diagnostics::logging::{self, Options};

fn flight_recorder(c: &mut Criterion) {
    logging::install(Options {
        base_filter: "info".to_owned(),
        flight_recorder: true,
        ..Options::default()
    })
    .expect("the benchmark installs logging once");
    let staging = tempfile::tempdir().expect("a staging directory");
    logging::flight::attach(staging.path()).expect("the recorder maps its ring");
    c.bench_function("flight_recorder/recorded_debug_event", |bencher| {
        bencher.iter(|| {
            tracing::debug!(
                target: "client_session::join",
                chunks = black_box(3_u32),
                server = black_box("play.example.net"),
                "joined after {} ms",
                black_box(412_u64)
            );
        });
    });
    c.bench_function("flight_recorder/disabled_trace_event", |bencher| {
        bencher.iter(|| {
            tracing::trace!(target: "client_session::join", chunks = black_box(3_u32), "ignored");
        });
    });
}

criterion_group!(benches, flight_recorder);
criterion_main!(benches);
