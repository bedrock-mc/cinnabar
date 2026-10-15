//! CSV timing and allocation diagnostics for the synthetic lighting scenes.
#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the diagnostics write help and per-solve CSV on stdout and percentile summaries on stderr"
)]

use std::{hint::black_box, time::Instant};

use super::{
    DEFAULT_SECTIONS,
    fixtures::{Fixture, Prior, Scene},
    solve,
};
use world::{
    LightBlockSample, LightChannel, LightReadAccess, LightSolveOutput, LightSolverScratch,
};

struct Options {
    samples: usize,
    warmups: usize,
    sections: Vec<usize>,
    filter: String,
    /// Times the full solve, which ignores retained light, instead of the incremental one.
    full: bool,
}

impl Options {
    /// Reads the CSV diagnostic options without changing their defaults.
    fn parse() -> Self {
        let mut options = Self {
            samples: 32,
            warmups: 4,
            sections: DEFAULT_SECTIONS.to_vec(),
            filter: String::new(),
            full: false,
        };
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--samples" => {
                    options.samples = arguments.next().expect("sample count").parse().unwrap();
                    assert!(options.samples > 0);
                }
                "--warmups" => {
                    options.warmups = arguments.next().expect("warmup count").parse().unwrap();
                }
                "--sections" => {
                    options.sections = arguments
                        .next()
                        .expect("comma-separated section counts")
                        .split(',')
                        .map(|value| value.parse().expect("integer section count"))
                        .collect();
                    assert!(
                        options
                            .sections
                            .iter()
                            .all(|value| (1..=24).contains(value))
                    );
                }
                "--case" => options.filter = arguments.next().expect("case substring"),
                "--full" => options.full = true,
                "--bench" => {}
                "--help" | "-h" => {
                    println!(
                        "light_work [--samples N] [--warmups N] [--sections 1,10,24] [--case substring] [--full]\n\
                         stdout: individual solve CSV; stderr: p50/p95/p99/max summaries.\n\
                         Times include output destruction. Allocation requests are measured separately.\n\
                         All inputs are synthetic; retained cases reuse a solved field, removal deletes its sources."
                    );
                    std::process::exit(0);
                }
                _ => panic!("unknown argument: {argument}"),
            }
        }
        options
    }
}

/// Hashes every solved light value and its direct-sky provenance.
fn fingerprint(fixture: &Fixture, output: &LightSolveOutput) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for position in fixture.positions() {
        let block = output.light_at(position, LightChannel::Block);
        let sky = output.light_at(position, LightChannel::Sky);
        let direct = output.has_direct_sky_provenance(0, position);
        hash ^= u64::from(block | sky << 4) | (u64::from(direct) << 8);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Checks one synthetic case and emits its timings and allocation counters.
fn run(options: &Options, scene: Scene, sections: usize, retained: bool) {
    let state = if retained { "retained" } else { "fresh" };
    let name = format!("synthetic_{}_{state}", scene.name());
    if !name.contains(&options.filter) {
        return;
    }
    let mut fixture = Fixture::new(scene, sections);
    let mut scratch = LightSolverScratch::default();
    let original = solve(
        &fixture,
        &Prior {
            output: None,
            fixture: &fixture,
        },
        &mut scratch,
        false,
    );
    if scene == Scene::Removal {
        fixture.samples.fill(LightBlockSample::KnownAir);
    }
    let prior = Prior {
        output: retained.then_some(&original),
        fixture: &fixture,
    };
    // Always establish capacity for this prior/input pair before measuring allocation requests.
    for _ in 0..options.warmups.max(1) {
        drop(black_box(solve(
            &fixture,
            &prior,
            &mut scratch,
            options.full,
        )));
    }
    let (
        output,
        test_alloc::Allocations {
            calls: allocations,
            bytes: allocated_bytes,
        },
    ) = test_alloc::measure(|| solve(&fixture, &prior, &mut scratch, options.full));
    let stats = output.stats();
    let hash = fingerprint(&fixture, &output);
    if scene == Scene::Removal || scene == Scene::Solid {
        assert!(fixture.positions().all(|position| {
            output.light_at(position, LightChannel::Block) == 0
                && output.light_at(position, LightChannel::Sky) == 0
        }));
    }
    if retained && scene != Scene::Removal {
        assert_eq!(hash, fingerprint(&fixture, &original));
    }
    // Both solvers reach the same light from the same inputs.
    let reference = solve(&fixture, &prior, &mut scratch, false);
    assert_eq!(hash, fingerprint(&fixture, &reference), "{name}/{sections}");
    drop(reference);
    drop(output);

    let mut times = Vec::with_capacity(options.samples);
    for _ in 0..options.samples {
        let started = Instant::now();
        drop(black_box(solve(
            &fixture,
            &prior,
            &mut scratch,
            options.full,
        )));
        times.push(started.elapsed().as_nanos());
    }
    for (iteration, nanoseconds) in times.iter().enumerate() {
        println!(
            "{name},{sections},{iteration},{nanoseconds},{allocations},{allocated_bytes},{},{},{},{},{hash:016x}",
            stats.darken_seeded, stats.darken_dequeued, stats.increase_dequeued, stats.queue_peak,
        );
    }
    times.sort_unstable();
    let quantile =
        |percent: usize| times[((times.len() * percent).div_ceil(100) - 1).min(times.len() - 1)];
    eprintln!(
        "{name}/{sections}: p50={} p95={} p99={} max={} ns; alloc={allocations}/{allocated_bytes} bytes; stats={stats:?}; output={hash:016x}",
        quantile(50),
        quantile(95),
        quantile(99),
        times.last().unwrap(),
    );
}

/// Runs the original CSV diagnostic over the requested scenes.
pub(super) fn main() {
    let options = Options::parse();
    println!(
        "case,sections,iteration,nanoseconds,allocation_calls,allocated_bytes,darken_seeded,darken_dequeued,increase_dequeued,queue_peak,fingerprint"
    );
    for &sections in &options.sections {
        for scene in Scene::ALL {
            if scene != Scene::Removal {
                run(&options, scene, sections, false);
            }
            if scene != Scene::Solid {
                run(&options, scene, sections, true);
            }
        }
    }
}
