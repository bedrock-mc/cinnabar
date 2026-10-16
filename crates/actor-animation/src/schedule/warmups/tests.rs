use super::*;
use crate::schedule::tests::{clock_assets, fingerprint, near_view, rig_assets};

mod budgets;
mod lifecycle;
mod production;

/// The original serial loop, kept independent of warm admission, guard and commit helpers.
pub(super) fn serial_reference(
    jobs: &mut [TickJob<'_>],
    catalogs: &OwnedCatalogs,
    ledger: &mut Ledger<'_>,
    world_left: &mut usize,
) -> Vec<usize> {
    let mut failed = Vec::new();
    let mut warm_left = *world_left / RESUME_WARMUP_SHARE;
    for (index, job) in jobs
        .iter_mut()
        .enumerate()
        .filter(|(_, job)| job.evaluates() && job.state.culled && job.state.history.len() > 1)
    {
        let elapsed = job.context.animation_elapsed_ticks;
        job.context.animation_elapsed_ticks = Some(elapsed.unwrap_or(1).saturating_sub(1));
        let latest = job.state.history.pop_back();
        let previous = Catalogs {
            tick: catalogs.tick.saturating_sub(1),
            ..catalogs.view()
        };
        let outcome = (warm_left > 0).then(|| evaluate_job(job, previous, &mut warm_left));
        job.state.history.extend(latest);
        match outcome {
            Some(Evaluation {
                result: Ok(evaluated),
                used,
            }) => {
                *world_left = world_left.saturating_sub(used);
                job.outcome = Some(Evaluation {
                    result: Ok(evaluated),
                    used,
                });
                ledger.apply(job);
            }
            Some(Evaluation { used, .. }) => {
                *world_left = world_left.saturating_sub(used);
                ledger.stats.evaluated_molang_ops = ledger
                    .stats
                    .evaluated_molang_ops
                    .saturating_add(used as u64);
                failed.push(index);
            }
            None => {}
        }
        job.context.animation_elapsed_ticks = elapsed.map(|ticks| ticks.min(1));
    }
    failed
}

/// Builds real initialized rigs and borrows their disjoint states only for the supplied operation.
fn with_jobs<R>(
    assets: Arc<RuntimeEntityAssets>,
    count: usize,
    use_jobs: impl FnOnce(&mut [TickJob<'_>], OwnedCatalogs) -> R,
) -> R {
    let actors: HashMap<_, _> = (1..=count as u64)
        .map(|id| {
            (
                id,
                TestActor {
                    runtime_id: id,
                    unique_id: id as i64,
                    ..TestActor::default()
                },
            )
        })
        .collect();
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    for id in 1..=count as u64 {
        store.insert(1, 0, &actors[&id]);
    }
    for _ in 0..3 {
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
    }
    let catalogs = OwnedCatalogs {
        vanilla: (assets, Arc::clone(&store.layout)),
        pack: None,
        tick: store.completed_tick + 1,
        exempt: None,
    };
    let mut jobs: Vec<_> = store
        .rigs
        .iter_mut()
        .map(|(lifetime, state)| {
            state.culled = true;
            TickJob {
                lifetime: *lifetime,
                state,
                actor: &actors[&lifetime.runtime_id],
                context: ActorTickContext {
                    animation_elapsed_ticks: Some(4),
                    ..Default::default()
                },
                view_changed: false,
                refresh_view: false,
                step: Step::Evaluate { pack: false },
                warm_candidate: false,
                outcome: None,
            }
        })
        .collect();
    use_jobs(&mut jobs, catalogs)
}

/// Captures every retained rig field, context and serial ledger value after one warm phase.
fn warm_snapshot(
    assets: Arc<RuntimeEntityAssets>,
    count: usize,
    world: usize,
    order: Option<usize>,
) -> String {
    warm_snapshot_with(assets, count, world, order, |_, _| {})
}

/// Configures mixed catalogs and inputs before comparing their retained states and actual charges.
fn warm_snapshot_with(
    assets: Arc<RuntimeEntityAssets>,
    count: usize,
    world: usize,
    order: Option<usize>,
    configure: impl FnOnce(&mut [TickJob<'_>], &mut OwnedCatalogs),
) -> String {
    with_jobs(assets, count, |jobs, mut catalogs| {
        configure(jobs, &mut catalogs);
        let mut stats = ActorAnimationStats::default();
        let mut generation = 100;
        let mut ledger = Ledger {
            tick: catalogs.tick,
            advance_history: true,
            next_reset_generation: &mut generation,
            stats: &mut stats,
            starved: None,
        };
        let mut left = world;
        let failed = if let Some(order) = order {
            run_with(
                jobs,
                &catalogs,
                &mut ledger,
                &mut left,
                MAX_HELPERS,
                |wave, catalogs, pass, _| {
                    let mut indices: Vec<_> = (0..wave.len()).collect();
                    match order {
                        1 => indices.reverse(),
                        2 => indices.sort_by_key(|index| (index % 2, *index)),
                        _ => {}
                    }
                    for index in indices {
                        let mut own = MAX_MOLANG_OPS_PER_ACTOR_TICK;
                        pass.evaluate(&mut wave[index], catalogs.view(), &mut own);
                    }
                },
            )
        } else {
            serial_reference(jobs, &catalogs, &mut ledger, &mut left)
        };
        assert!(jobs.iter().all(|job| job.outcome.is_none()));
        format!(
            "{:?} {:?} {left} {:?} {} {:?}",
            jobs.iter()
                .map(|job| format!("{:?} {:?}", job.state, job.context.animation_elapsed_ticks))
                .collect::<Vec<_>>(),
            failed,
            ledger.stats,
            ledger.next_reset_generation,
            ledger.starved
        )
    })
}

/// Completion order cannot change any retained rig field, budget, generation or warm failure.
#[test]
fn warm_crowds_match_the_frozen_serial_loop_in_every_completion_order() {
    let b = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    for assets in [
        crate::test_support::counting_random_assets(),
        clock_assets(Some(1_000.0)),
    ] {
        for count in [1, 7, 8, 15, 16, 17, 50, 100] {
            for world in [
                0,
                RESUME_WARMUP_SHARE * (b - 1),
                RESUME_WARMUP_SHARE * (8 * b + 1),
                MAX_MOLANG_OPS_PER_WORLD_TICK,
            ] {
                let expected = warm_snapshot(Arc::clone(&assets), count, world, None);
                for order in 0..3 {
                    assert_eq!(
                        warm_snapshot(Arc::clone(&assets), count, world, Some(order)),
                        expected,
                        "count {count}, world {world}, order {order}"
                    );
                }
            }
        }
    }
}
