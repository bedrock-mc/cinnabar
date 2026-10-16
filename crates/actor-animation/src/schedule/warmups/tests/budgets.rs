use super::*;

/// A rig whose only Molang work is one expression of exactly `cost` operations.
pub(super) fn cost_assets(cost: usize) -> Arc<RuntimeEntityAssets> {
    use assets::{CompiledMolangExpression, MolangOp};
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    compiled.rig_bindings[0].initialize = None;
    compiled.rig_bindings[0].pre_animation = (cost > 0).then_some(0);
    compiled.rig_geometries[0].animation_count = 0;
    compiled.rig_animations = Box::new([]);
    compiled.render = assets::EntityRenderData::default();
    let scalar = crate::test_support::scalar;
    let (mut ops, max_stack) = if cost <= assets::MAX_MOLANG_OPS_PER_EXPRESSION {
        let mut ops = vec![MolangOp::Push(scalar(0.0))];
        ops.extend(std::iter::repeat_n(
            MolangOp::Negate,
            cost.saturating_sub(1),
        ));
        (ops, 1)
    } else {
        // Three setup operations and three operations per iteration fit costs above the
        // per-expression storage bound without changing the exact runtime charge.
        let iterations = ((cost - 3) / 3).min(assets::MAX_MOLANG_LOOP_ITERATIONS as usize);
        let mut ops = vec![
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(iterations as f32)),
            MolangOp::LoopStart(6),
            MolangOp::Negate,
            MolangOp::Negate,
            MolangOp::LoopNext(3),
        ];
        ops.extend(std::iter::repeat_n(
            MolangOp::Negate,
            cost - 3 - 3 * iterations,
        ));
        (ops, 2)
    };
    let mut expressions = vec![CompiledMolangExpression {
        first_op: 0,
        op_count: ops.len() as u16,
        max_stack,
    }];
    // Unused authored clips still refer to these expressions; keep all table ranges valid.
    for _ in 1..compiled.molang_expressions.len() {
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 1,
            max_stack: 1,
        });
        ops.push(MolangOp::Push(scalar(0.0)));
    }
    compiled.molang_expressions = expressions.into();
    compiled.molang_ops = ops.into();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// Exact cap and partial-cap errors retain the actor-first classification and actual charge.
#[test]
fn exact_costs_keep_actor_first_errors_and_restore_transient_inputs() {
    let b = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    for cost in [0, 1, b - 1, b, b + 1] {
        with_jobs(cost_assets(cost), 1, |jobs, catalogs| {
            let job = &mut jobs[0];
            let history = format!("{:?}", job.state.history);
            for allowance in [1, b - 1, b, b + 1] {
                let mut left = allowance;
                let outcome = evaluate(job, catalogs.view(), &mut left);
                let used = cost.min(allowance).min(b);
                assert_eq!(outcome.used, used);
                assert_eq!(left, allowance - used);
                let error = outcome.result.err();
                let expected = if cost <= used {
                    None
                } else if used == b {
                    Some(EvalError::ActorBudget)
                } else {
                    Some(EvalError::WorldBudget)
                };
                assert_eq!(error, expected, "cost {cost}, allowance {allowance}");
                assert_eq!(format!("{:?}", job.state.history), history);
                assert_eq!(job.context.animation_elapsed_ticks, Some(4));
            }
        });
    }
}

/// Full waves and the serial tail agree at every world admission boundary, even for failures.
#[test]
fn warm_budget_boundaries_match_the_serial_reference() {
    let b = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    for cost in [0, 1, b - 1, b, b + 1] {
        let assets = cost_assets(cost);
        for warm in [
            0,
            1,
            b - 1,
            b,
            b + 1,
            2 * b - 1,
            2 * b,
            8 * b - 1,
            8 * b,
            8 * b + 1,
            16 * b,
        ] {
            let world = warm * RESUME_WARMUP_SHARE;
            let expected = warm_snapshot(Arc::clone(&assets), 17, world, None);
            assert_eq!(
                warm_snapshot(Arc::clone(&assets), 17, world, Some(1)),
                expected,
                "cost {cost}, warm {warm}"
            );
        }
    }
}

/// A cheap first actor leaves the second its full cap; reserving or splitting caps would differ.
#[test]
fn cheap_then_heavy_uses_actual_charges_and_preserves_the_partial_tail() {
    let b = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    with_jobs(cost_assets(1), 3, |jobs, mut catalogs| {
        let heavy = cost_assets(b);
        catalogs.pack = Some((Arc::clone(&heavy), Arc::new(VariableLayout::new(&heavy))));
        jobs[1].step = Step::Evaluate { pack: true };
        jobs[1].state.pack = true;
        let mut stats = ActorAnimationStats::default();
        let mut generation = 1;
        let mut ledger = Ledger {
            tick: catalogs.tick,
            advance_history: true,
            next_reset_generation: &mut generation,
            stats: &mut stats,
            starved: None,
        };
        let mut world = RESUME_WARMUP_SHARE * 2 * b;
        let failed = run_with(
            jobs,
            &catalogs,
            &mut ledger,
            &mut world,
            MAX_HELPERS,
            |_, _, _, _| panic!("three candidates cannot launch helpers"),
        );
        assert!(failed.is_empty());
        assert_eq!(ledger.stats.evaluated_molang_ops, (b + 2) as u64);
        assert_eq!(world, RESUME_WARMUP_SHARE * 2 * b - b - 2);
        assert!(
            jobs.iter()
                .all(|job| job.state.completed_tick == catalogs.tick)
        );
    });
}

/// Zero-cost waves cannot create unbounded callbacks, and zero-helper warm runs never dispatch.
#[test]
fn helper_launches_use_the_existing_formula_and_a_phase_quota() {
    let b = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    for available in [0, 1, MAX_HELPERS, MAX_HELPERS + 1] {
        with_jobs(cost_assets(0), 100, |jobs, catalogs| {
            let mut stats = ActorAnimationStats::default();
            let mut generation = 1;
            let mut ledger = Ledger {
                tick: catalogs.tick,
                advance_history: true,
                next_reset_generation: &mut generation,
                stats: &mut stats,
                starved: None,
            };
            let mut world = RESUME_WARMUP_SHARE * RIGS_PER_HELPER * b;
            let mut launches = 0;
            let mut boards = 0;
            let failed = run_with(
                jobs,
                &catalogs,
                &mut ledger,
                &mut world,
                available,
                |wave, catalogs, pass, helpers| {
                    let candidates = wave.iter().filter(|job| job.warm_candidate).count();
                    assert_eq!(
                        helpers,
                        helper_count(candidates, available.min(MAX_HELPERS - launches))
                    );
                    launches += helpers;
                    boards += 1;
                    for job in wave {
                        let mut own = b;
                        pass.evaluate(job, catalogs.view(), &mut own);
                    }
                },
            );
            assert!(launches <= MAX_HELPERS && boards <= MAX_HELPERS);
            assert_eq!(launches, if available == 0 { 0 } else { MAX_HELPERS });
            assert_eq!(world, RESUME_WARMUP_SHARE * RIGS_PER_HELPER * b);
            assert_eq!(ledger.stats.evaluated_molang_ops, 0);
            assert!(failed.is_empty());
            assert!(
                jobs.iter()
                    .all(|job| job.state.completed_tick == catalogs.tick)
            );
        });
    }
}

/// A failed warm result is consumed without freeze/starvation accounting; a skip is distinct.
#[test]
fn failed_and_skipped_warmups_do_not_leak_into_current_results() {
    for warm in [0, 1] {
        with_jobs(cost_assets(2), 1, |jobs, catalogs| {
            let mut stats = ActorAnimationStats::default();
            let mut generation = 1;
            let mut ledger = Ledger {
                tick: catalogs.tick,
                advance_history: true,
                next_reset_generation: &mut generation,
                stats: &mut stats,
                starved: None,
            };
            let mut world = RESUME_WARMUP_SHARE * warm;
            let failed = run_with(
                jobs,
                &catalogs,
                &mut ledger,
                &mut world,
                0,
                |_, _, _, _| unreachable!(),
            );
            assert_eq!(failed, if warm == 0 { vec![] } else { vec![0] });
            assert_eq!(ledger.stats.evaluated_molang_ops, warm as u64);
            assert_eq!(ledger.stats.world_budget_exhaustions, 0);
            assert_eq!(ledger.stats.actor_budget_exhaustions, 0);
            assert!(ledger.starved.is_none());
            assert!(jobs[0].outcome.is_none());
            assert_eq!(jobs[0].context.animation_elapsed_ticks, Some(1));
        });
    }
}

/// Invalid evaluations retain partial work but do not adopt body variables or lose history.
#[test]
fn invalid_partial_work_matches_the_serial_error_path() {
    with_jobs(
        crate::test_support::counting_random_assets(),
        1,
        |jobs, mut catalogs| {
            catalogs.vanilla.1 = Arc::new(VariableLayout::default());
            let job = &mut jobs[0];
            let history = format!("{:?}", job.state.history);
            let variables = format!("{:?}", job.state.variables);
            let mut left = MAX_MOLANG_OPS_PER_ACTOR_TICK;
            let outcome = evaluate(job, catalogs.view(), &mut left);
            assert_eq!(outcome.result.err(), Some(EvalError::Invalid));
            assert!(outcome.used > 0);
            assert_eq!(format!("{:?}", job.state.variables), variables);
            assert_eq!(format!("{:?}", job.state.history), history);
            assert_eq!(job.context.animation_elapsed_ticks, Some(4));
        },
    );
}

/// A heterogeneous prefix, including cap failures, preserves canonical lifetime priority.
#[test]
fn mixed_catalog_costs_match_serial_in_claim_and_completion_permutations() {
    let b = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    for (cheap, heavy) in [(0, b), (1, b - 1), (b, b + 1)] {
        let native = cost_assets(cheap);
        let pack = cost_assets(heavy);
        for warm in [8 * b - 1, 8 * b, 16 * b + 1] {
            let snapshot = |order| {
                warm_snapshot_with(
                    Arc::clone(&native),
                    50,
                    RESUME_WARMUP_SHARE * warm,
                    order,
                    |jobs, catalogs| {
                        catalogs.pack =
                            Some((Arc::clone(&pack), Arc::new(VariableLayout::new(&pack))));
                        for (index, job) in jobs.iter_mut().enumerate() {
                            if index % 3 != 0 {
                                job.step = Step::Evaluate { pack: true };
                                job.state.pack = true;
                            }
                        }
                    },
                )
            };
            let serial = snapshot(None);
            for order in 0..3 {
                assert_eq!(snapshot(Some(order)), serial);
            }
        }
    }
}
