//! Synthetic solver diagnostics; this does not measure scheduler skips or live frame budgets.

#[path = "light_work/allocations.rs"]
mod allocations;
#[path = "light_work/fixtures.rs"]
mod fixtures;

use std::{hint::black_box, time::Instant};

use fixtures::{Fixture, Prior, Scene};
use world::{
    LightBlockSample, LightChannel, LightReadAccess, LightSolveOutput, LightSolverScratch,
    solve_light_with_scratch,
};

#[global_allocator]
static ALLOCATOR: allocations::CountedAllocator = allocations::CountedAllocator;

struct Options {
    samples: usize,
    warmups: usize,
    sections: Vec<usize>,
    filter: String,
}

impl Options {
    fn parse() -> Self {
        let mut options = Self {
            samples: 32,
            warmups: 4,
            sections: vec![1, 10, 24],
            filter: String::new(),
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
                "--bench" => {}
                "--help" | "-h" => {
                    println!(
                        "light_work [--samples N] [--warmups N] [--sections 1,10,24] [--case substring]\n\
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

fn solve(
    fixture: &Fixture,
    prior: &Prior<'_>,
    scratch: &mut LightSolverScratch,
) -> LightSolveOutput {
    solve_light_with_scratch(
        black_box(fixture),
        black_box(prior),
        fixture.bounds,
        17,
        fixture.profile(),
        fixture.limits(),
        scratch,
    )
    .expect("valid synthetic lighting fixture")
}

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
        drop(black_box(solve(&fixture, &prior, &mut scratch)));
    }
    let (output, allocations, allocated_bytes) =
        allocations::measure(|| solve(&fixture, &prior, &mut scratch));
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
    drop(output);

    let mut times = Vec::with_capacity(options.samples);
    for _ in 0..options.samples {
        let started = Instant::now();
        drop(black_box(solve(&fixture, &prior, &mut scratch)));
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

fn main() {
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
