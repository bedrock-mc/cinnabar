use super::*;

/// Repeated return cycles exercise the real prepass, warm phase, current pass and final overrides.
#[test]
fn production_ticks_match_serial_with_pack_ui_resets_and_lifetime_reuse() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(MAX_HELPERS + 1)
        .build()
        .unwrap();
    pool.install(|| {
        let b = MAX_MOLANG_OPS_PER_ACTOR_TICK;
        for count in [1, 7, 8, 15, 16, 17, 50, 100] {
            for world in [
                0,
                RESUME_WARMUP_SHARE * (b + 1),
                MAX_MOLANG_OPS_PER_WORLD_TICK,
            ] {
                let native = crate::test_support::counting_random_assets();
                let pack = crate::test_support::counting_random_assets_for("minecraft:pack_test");
                let mut serial = ActorAnimationStore::with_assets(Arc::clone(&native));
                serial.schedule.serial = true;
                let mut parallel = ActorAnimationStore::with_assets(native);
                let mut actors: HashMap<_, _> = (1..=count as u64)
                    .map(|id| {
                        (
                            id,
                            TestActor {
                                runtime_id: id,
                                unique_id: id as i64,
                                kind: ActorKind::Entity {
                                    identifier: if id % 2 == 0 {
                                        "minecraft:pack_test".into()
                                    } else {
                                        "minecraft:test".into()
                                    },
                                },
                                position: [50.0, 0.0, 0.0],
                                previous_position: [50.0, 0.0, 0.0],
                                ..TestActor::default()
                            },
                        )
                    })
                    .collect();
                for store in [&mut serial, &mut parallel] {
                    store.set_pack(Some((Arc::clone(&pack), vec![0])));
                    let mut ids: Vec<_> = actors.keys().copied().collect();
                    ids.sort_unstable();
                    for id in ids {
                        store.insert(1, 0, &actors[&id]);
                    }
                }
                let near = near_view();
                let mut absent = None;
                for tick in 0..16 {
                    if tick == 8 {
                        let actor = actors.get_mut(&1).unwrap();
                        actor.spawn_revision += 1;
                        for store in [&mut serial, &mut parallel] {
                            store.insert(1, 0, actor);
                        }
                    }
                    if tick == 12 {
                        absent = actors.remove(&2);
                    }
                    if tick == 13
                        && let Some(actor) = absent.take()
                    {
                        actors.insert(2, actor);
                    }
                    let local = 1 + (tick as u64 % count as u64);
                    let exempt = Some(local);
                    for store in [&mut serial, &mut parallel] {
                        store.schedule.world_budget = world;
                        if tick == 5 {
                            store.mark_reset(1);
                        }
                        if tick == 10 {
                            store.set_pack(None);
                        }
                        if tick == 11 {
                            store.set_pack(Some((Arc::clone(&pack), vec![0])));
                        }
                        let elapsed = [None, Some(0), Some(1), Some(4), Some(120)][tick % 5];
                        let context = |actor: &TestActor| ActorTickContext {
                            animation_elapsed_ticks: elapsed,
                            appearance_pending: tick == 7 && actor.runtime_id.is_multiple_of(3),
                            is_local: exempt == Some(actor.runtime_id),
                            is_local_first_person: tick % 2 == 0
                                && exempt == Some(actor.runtime_id),
                            ..Default::default()
                        };
                        store.advance_tick(
                            &actors,
                            (tick % 4 == 1 || tick % 4 == 2).then_some(&near),
                            exempt,
                            tick != 6,
                            true,
                            context,
                        );
                        if tick % 4 == 3 {
                            store.refresh_local_view(&actors, local, context);
                        }
                    }
                    assert_eq!(
                        fingerprint(&parallel),
                        fingerprint(&serial),
                        "count {count}, world {world}, tick {tick}"
                    );
                }
            }
        }
    });
}

/// Evaluation uses T-1 while the ordered commit retains T and the warm elapsed value.
#[test]
fn warm_commit_restores_history_and_preserves_elapsed_distinctions() {
    for elapsed in [None, Some(0), Some(1), Some(4), Some(u32::MAX)] {
        with_jobs(clock_assets(None), 1, |jobs, catalogs| {
            let job = &mut jobs[0];
            job.context.animation_elapsed_ticks = elapsed;
            job.context.is_local = true;
            job.state.reset_pending = true;
            let history = format!("{:?}", job.state.history);
            let mut stats = ActorAnimationStats::default();
            let mut generation = 73;
            let mut ledger = Ledger {
                tick: catalogs.tick,
                advance_history: true,
                next_reset_generation: &mut generation,
                stats: &mut stats,
                starved: None,
            };
            let mut world = MAX_MOLANG_OPS_PER_WORLD_TICK;
            let failed = run_with(
                jobs,
                &catalogs,
                &mut ledger,
                &mut world,
                0,
                |_, _, _, _| unreachable!(),
            );
            let job = &mut jobs[0];
            assert!(failed.is_empty());
            assert_eq!(format!("{:?}", job.state.history), history);
            assert_eq!(job.state.completed_tick, catalogs.tick);
            assert_eq!(job.state.animation_epoch, catalogs.tick);
            assert_eq!(job.state.reset_generation, 73);
            assert_eq!(
                job.context.animation_elapsed_ticks,
                elapsed.map(|ticks| ticks.min(1))
            );
            let replay = job.state.replay.as_ref().unwrap();
            assert_eq!(replay.tick, catalogs.tick);
            assert_eq!(replay.elapsed, Some(elapsed.unwrap_or(1).saturating_sub(1)));
        });
    }
}

/// The first current pose advances from the warm endpoint rather than the held culled endpoint.
#[test]
fn previous_catalog_tick_becomes_the_first_visible_previous_pose() {
    with_jobs(clock_assets(None), 1, |jobs, catalogs| {
        let mut stats = ActorAnimationStats::default();
        let mut generation = 1;
        let mut ledger = Ledger {
            tick: catalogs.tick,
            advance_history: true,
            next_reset_generation: &mut generation,
            stats: &mut stats,
            starved: None,
        };
        let mut world = MAX_MOLANG_OPS_PER_WORLD_TICK;
        run_with(
            jobs,
            &catalogs,
            &mut ledger,
            &mut world,
            0,
            |_, _, _, _| unreachable!(),
        );
        let job = &mut jobs[0];
        let previous = job.state.current.clone();
        assert!(!job.state.culled);
        assert_eq!(
            previous[0].translation_scale[0],
            -(catalogs.tick as f32 - 1.0)
        );
        job.outcome = Some(evaluate_job(job, catalogs.view(), &mut world));
        ledger.apply(job);
        assert_eq!(job.state.previous, previous);
        assert_eq!(
            job.state.current[0].translation_scale[0],
            -(catalogs.tick as f32)
        );
    });
}

/// Main body and independent HUD evaluation share one cap, including when HUD state is retained.
#[test]
fn hud_and_body_share_the_same_actor_and_world_allowance() {
    let assets = clock_assets(Some(1_000.0));
    with_jobs(assets, 1, |jobs, mut catalogs| {
        let job = &mut jobs[0];
        let mut left = MAX_MOLANG_OPS_PER_ACTOR_TICK;
        let body = evaluate(job, catalogs.view(), &mut left);
        assert!(body.result.is_ok());
        catalogs.exempt = Some(job.lifetime.runtime_id);
        let mut shared = body.used + 1;
        let outcome = evaluate(job, catalogs.view(), &mut shared);
        assert!(outcome.result.is_ok());
        assert_eq!(outcome.used, body.used + 1);
        assert_eq!(shared, 0);
        assert!(job.state.ui_animation.is_some());
        assert!(
            job.state.ui_pose.as_ref().unwrap().is_empty(),
            "HUD cannot get a fresh cap"
        );
        let mut full = MAX_MOLANG_OPS_PER_ACTOR_TICK;
        let outcome = evaluate(job, catalogs.view(), &mut full);
        assert!(outcome.used <= MAX_MOLANG_OPS_PER_ACTOR_TICK);
    });
}

/// Interleaved noncandidates retain their contexts, history and state throughout a warm board.
#[test]
fn admission_is_frozen_and_non_candidates_are_untouched() {
    with_jobs(clock_assets(None), 2 * RIGS_PER_HELPER, |jobs, catalogs| {
        for (index, job) in jobs.iter_mut().enumerate() {
            match index % 4 {
                0 => job.step = Step::Hold,
                1 => job.step = Step::GeometryOnly,
                2 => job.step = Step::Culled,
                _ => {}
            }
        }
        // Add enough candidates to dispatch while keeping each non-evaluating kind interleaved.
        for job in jobs.iter_mut().skip(8) {
            job.step = Step::Evaluate { pack: false };
        }
        jobs[3].state.history.truncate(1);
        let untouched: Vec<_> = jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| !job.evaluates() || job.state.history.len() < 2)
            .map(|(index, job)| (index, format!("{:?} {:?}", job.state, job.context)))
            .collect();
        let mut stats = ActorAnimationStats::default();
        let mut generation = 1;
        let mut ledger = Ledger {
            tick: catalogs.tick,
            advance_history: true,
            next_reset_generation: &mut generation,
            stats: &mut stats,
            starved: None,
        };
        let mut world = MAX_MOLANG_OPS_PER_WORLD_TICK;
        let mut dispatched = 0;
        run_with(
            jobs,
            &catalogs,
            &mut ledger,
            &mut world,
            1,
            |wave, catalogs, pass, _| {
                dispatched += 1;
                for job in wave {
                    let mut own = MAX_MOLANG_OPS_PER_ACTOR_TICK;
                    pass.evaluate(job, catalogs.view(), &mut own);
                    if job.warm_candidate {
                        // A worker-side change must not make the caller silently discard its result.
                        job.state.culled = false;
                    }
                }
            },
        );
        assert!(dispatched > 0);
        for (index, expected) in untouched {
            assert_eq!(
                format!("{:?} {:?}", jobs[index].state, jobs[index].context),
                expected
            );
        }
        assert!(
            jobs.iter()
                .filter(|job| job.warm_candidate)
                .all(|job| job.state.completed_tick == catalogs.tick && job.outcome.is_none())
        );
    });
}

/// History lengths zero and one never enter a warm pass, even when they would otherwise evaluate.
#[test]
fn short_histories_and_visible_rigs_are_not_warmed() {
    with_jobs(clock_assets(None), 3, |jobs, catalogs| {
        jobs[0].state.history.clear();
        jobs[1].state.history.truncate(1);
        jobs[2].state.culled = false;
        let before: Vec<_> = jobs
            .iter()
            .map(|job| format!("{:?} {:?}", job.state, job.context))
            .collect();
        let mut stats = ActorAnimationStats::default();
        let mut generation = 1;
        let mut ledger = Ledger {
            tick: catalogs.tick,
            advance_history: true,
            next_reset_generation: &mut generation,
            stats: &mut stats,
            starved: None,
        };
        let mut world = MAX_MOLANG_OPS_PER_WORLD_TICK;
        assert!(
            run_with(
                jobs,
                &catalogs,
                &mut ledger,
                &mut world,
                MAX_HELPERS,
                |_, _, _, _| panic!("noncandidates cannot dispatch")
            )
            .is_empty()
        );
        for (job, expected) in jobs.iter().zip(before) {
            assert_eq!(format!("{:?} {:?}", job.state, job.context), expected);
        }
        assert_eq!(ledger.stats.evaluated_molang_ops, 0);
    });
}

/// Failed warm-ups receive the final override even when current evaluation fails too.
/// The no-budget candidate keeps culling and is the only warm attempt on the next tick.
#[test]
fn failed_warmups_are_not_retried_but_budget_skips_remain_eligible() {
    let b = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    let count = MAX_MOLANG_OPS_PER_WORLD_TICK / RESUME_WARMUP_SHARE / b + 1;
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
    let mut store = ActorAnimationStore::with_assets(budgets::cost_assets(b + 1));
    for actor in actors.values() {
        store.insert(1, 0, actor);
    }
    for _ in 0..3 {
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
    }
    for state in store.rigs.values_mut() {
        state.culled = true;
    }
    let before = store.stats.evaluated_molang_ops;
    store.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    assert_eq!(
        store.stats.evaluated_molang_ops - before,
        ((count * 2 - 1) * b) as u64
    );
    for (index, state) in store.rigs.values().enumerate() {
        assert_eq!(state.culled, index == count - 1);
    }
    let before = store.stats.evaluated_molang_ops;
    store.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    assert_eq!(
        store.stats.evaluated_molang_ops - before,
        ((count + 1) * b) as u64
    );
    assert!(store.rigs.values().all(|state| !state.culled));
    let before = store.stats.evaluated_molang_ops;
    store.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    assert_eq!(
        store.stats.evaluated_molang_ops - before,
        (count * b) as u64
    );
}

/// Warm and current evaluation together consume exactly the original elapsed interval.
#[test]
fn elapsed_splits_do_not_double_script_advancement() {
    use assets::{MolangOp, MolangSymbolKind};
    let assets = rig_assets(
        &[
            (MolangSymbolKind::Query, "query.delta_time"),
            (MolangSymbolKind::Variable, "variable.elapsed"),
        ],
        vec![
            MolangOp::LoadVariable(2),
            MolangOp::LoadQuery(1),
            MolangOp::Add,
            MolangOp::StoreVariable(2),
            MolangOp::LoadVariable(2),
            MolangOp::Push(crate::test_support::scalar(20.0)),
            MolangOp::Multiply,
        ],
        2,
        None,
    );
    for elapsed in [None, Some(0), Some(1), Some(4), Some(120)] {
        with_jobs(Arc::clone(&assets), 1, |jobs, catalogs| {
            jobs[0].context.animation_elapsed_ticks = elapsed;
            let before = jobs[0].state.current[0].translation_scale[0];
            let mut stats = ActorAnimationStats::default();
            let mut generation = 1;
            let mut ledger = Ledger {
                tick: catalogs.tick,
                advance_history: true,
                next_reset_generation: &mut generation,
                stats: &mut stats,
                starved: None,
            };
            let mut world = MAX_MOLANG_OPS_PER_WORLD_TICK;
            run_with(
                jobs,
                &catalogs,
                &mut ledger,
                &mut world,
                0,
                |_, _, _, _| unreachable!(),
            );
            let job = &mut jobs[0];
            let warm_delta = job.state.current[0].translation_scale[0] - before;
            assert!((warm_delta + elapsed.unwrap_or(1).saturating_sub(1) as f32).abs() < 1e-3);
            job.outcome = Some(evaluate_job(job, catalogs.view(), &mut world));
            ledger.apply(job);
            let delta = job.state.current[0].translation_scale[0] - before;
            assert!((delta + elapsed.unwrap_or(1) as f32).abs() < 1e-3);
        });
    }
}
