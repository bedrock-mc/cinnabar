//! Attachables worn on the body rather than held.

use super::*;

#[test]
fn worn_attachables_read_owner_movement_delta_for_glide_wings() {
    let mut compiled = compiled_fixture();
    compiled.molang_symbols[1..5].rotate_left(1);
    compiled.molang_symbols[4].identifier = "query.position_delta".into();
    for op in &mut compiled.molang_ops {
        let remap = |index| match index {
            1 => 4,
            2..=4 => index - 1,
            other => other,
        };
        match op {
            MolangOp::LoadQuery(index) => *index = remap(*index),
            MolangOp::CallQuery(call) => call.symbol = remap(call.symbol),
            _ => {}
        }
    }
    let offset = compiled.molang_ops.len() as u32;
    let mut ops = compiled.molang_ops.into_vec();
    ops.extend([
        MolangOp::Push(scalar(1.0)),
        MolangOp::CallQuery(MolangCall {
            symbol: 4,
            arguments: 1,
        }),
    ]);
    compiled.molang_ops = ops.into();
    let mut expressions = compiled.molang_expressions.into_vec();
    compiled.animation_keyframes[0].expressions[1] = Some(expressions.len() as u32);
    expressions.push(CompiledMolangExpression {
        first_op: offset,
        op_count: 2,
        max_stack: 1,
    });
    compiled.molang_expressions = expressions.into();
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut rig = owner_rig();
    rig.animation_variables = ActorAnimationVariables::default().with_input(Some(ActorTickInput {
        position_delta: [0.0, -0.4, 0.0],
        swim_amount: 0.7,
        ..Default::default()
    }));
    let mut runtime = AttachablesRuntime::new(assets);
    let snapshot = runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &rig,
            AttachableAnimationInput {
                worn: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!((snapshot.pose[0].translation_scale[1] + 0.4).abs() < 1e-6);
    runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &rig,
            AttachableAnimationInput::default(),
        )
        .unwrap();
    assert_eq!(
        runtime.states.len(),
        2,
        "held and worn controllers retain independent state"
    );
}

#[test]
fn elytra_equipment_hides_and_restores_the_outer_chest_layer() {
    let mut compiled = compiled_fixture();
    compiled.molang_symbols[7].identifier = "variable.chest_layer_visible".into();
    let assets = RuntimeEntityAssets::from_compiled(compiled).unwrap();
    let layout = VariableLayout::new(&assets);
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut values = layout.fresh(1);
    let mut context = ActorTickContext::default();
    context.armor[1] = Some(WornArmor {
        item: "minecraft:elytra".into(),
        dye_rgb: None,
    });
    let refresh = |values: &mut MolangVariables, context: &ActorTickContext| {
        tick::apply_engine_variables(
            &layout.engine,
            values,
            &owner,
            context,
            &ActorTickInput::default(),
            &MotionState::default(),
        );
    };
    refresh(&mut values, &context);
    assert_eq!(values.get(layout.engine.chest_layer_visible), Some(0.0));
    context.armor[1] = None;
    refresh(&mut values, &context);
    assert_eq!(values.get(layout.engine.chest_layer_visible), Some(1.0));
}
