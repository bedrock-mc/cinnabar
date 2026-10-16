use super::*;
use assets::{CompiledMolangExpression, MolangCall, MolangFunction, MolangSymbol};

/// Admits a synthetic program, then evaluates it with live query inputs rather than constants.
fn evaluate(mut ops: Vec<MolangOp>, input: ActorTickInput) -> f32 {
    let mut compiled = crate::test_support::compiled_fixture();
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.head_y_rotation"),
        (MolangSymbolKind::Query, "query.health"),
    ]
    .map(|(kind, name)| MolangSymbol {
        kind,
        identifier: name.into(),
    })
    .into();
    let first = CompiledMolangExpression {
        first_op: 0,
        op_count: ops.len() as u16,
        max_stack: assets::molang_program_stack(&ops).unwrap(),
    };
    // The fixture's animation and render layers also address expressions 1–3.
    let mut expressions = vec![first];
    for _ in 1..4 {
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 1,
            max_stack: 1,
        });
        ops.push(MolangOp::Push(crate::test_support::scalar(0.0)));
    }
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let layout = VariableLayout::new(&assets);
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    let context = ActorTickContext::default();
    let evaluator = Evaluator {
        assets: &assets,
        pack: false,
        layout: &layout,
        program: None,
        actor: &actor,
        input: &input,
        context: &context,
        anim_tick: 0,
        anim_time: None,
        swell_amount: None,
        presentation_alpha: None,
        query_history: None,
        life_tick: 0,
        finished: (false, false),
        state_time: 0.0,
        bones: &[],
        bone_names: &[],
    };
    let mut world_left = MAX_MOLANG_OPS_PER_WORLD_TICK;
    let mut budget = EvalBudget {
        actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
        world_left: &mut world_left,
        work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
        stack: Default::default(),
        static_draw: None,
    };
    evaluator
        .number(0, &mut layout.fresh(1), 0.0, &mut budget)
        .unwrap()
}

/// Computes sqrt(query.health - 1); the fixture actor has no health and reads zero.
fn computed_nan() -> Vec<MolangOp> {
    vec![
        MolangOp::LoadQuery(2),
        MolangOp::Push(crate::test_support::scalar(1.0)),
        MolangOp::Subtract,
        MolangOp::Call(MolangFunction::Sqrt),
    ]
}

#[test]
fn head_yaw_query_idles_for_a_computed_nan_limit_and_keeps_finite_clamps() {
    let input = ActorTickInput {
        head_yaw: 60.0,
        ..Default::default()
    };
    let call = MolangOp::CallQuery(MolangCall {
        symbol: 1,
        arguments: 1,
    });
    let mut ops = computed_nan();
    ops.push(call);
    assert_eq!(evaluate(ops, input), 0.0);
    for (limit, expected) in [(-30.0, 30.0), (0.0, 0.0), (30.0, 30.0), (90.0, 60.0)] {
        assert_eq!(
            evaluate(
                vec![MolangOp::Push(crate::test_support::scalar(limit)), call],
                input
            ),
            expected
        );
    }
}

#[test]
fn integer_random_calls_propagate_computed_nan_in_either_bound() {
    for function in [
        MolangFunction::RandomInteger,
        MolangFunction::DieRollInteger,
    ] {
        for nan_first in [true, false] {
            let mut ops = Vec::new();
            if function == MolangFunction::DieRollInteger {
                ops.push(MolangOp::Push(crate::test_support::scalar(3.0)));
            }
            if !nan_first {
                ops.push(MolangOp::Push(crate::test_support::scalar(0.0)));
            }
            ops.extend(computed_nan());
            if nan_first {
                ops.push(MolangOp::Push(crate::test_support::scalar(0.0)));
            }
            ops.push(MolangOp::Call(function));
            assert!(
                evaluate(ops, ActorTickInput::default()).is_nan(),
                "{function:?}, nan_first={nan_first}"
            );
        }
    }
}
