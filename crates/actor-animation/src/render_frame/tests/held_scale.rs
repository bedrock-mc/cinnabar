use super::*;
use assets::{CompiledMolangExpression, EntityGeometryScalar};

mod motion;

/// Adds a scale that reads the counting pre-animation variable and this frame's fraction.
fn fixture() -> (
    ActorAnimationStore,
    HashMap<u64, crate::test_actor::TestActor>,
) {
    fixture_with(crate::test_support::counting_random_compiled(
        "minecraft:test",
    ))
}

/// Starts an owner with a held scale reading its authored variable and frame fraction.
fn fixture_with(
    compiled: assets::CompiledEntityAssets,
) -> (
    ActorAnimationStore,
    HashMap<u64, crate::test_actor::TestActor>,
) {
    fixture_with_query(compiled, 1)
}

/// Keeps query symbols ordered while selecting which live frame input drives held scale.
fn fixture_with_query(
    mut compiled: assets::CompiledEntityAssets,
    query: u32,
) -> (
    ActorAnimationStore,
    HashMap<u64, crate::test_actor::TestActor>,
) {
    let mut ops = compiled.molang_ops.into_vec();
    let mut expressions = compiled.molang_expressions.into_vec();
    compiled.rig_bindings[0].initialize = Some(expressions.len() as u32);
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 3,
        max_stack: 1,
    });
    ops.extend([
        MolangOp::Push(EntityGeometryScalar::new(2.0).unwrap()),
        MolangOp::StoreVariable(7),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
    ]);
    compiled.rig_bindings[0].held_item_scale = Some(expressions.len() as u32);
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 3,
        max_stack: 2,
    });
    ops.extend([
        MolangOp::LoadVariable(7),
        MolangOp::LoadQuery(query),
        MolangOp::Add,
    ]);
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut store = ActorAnimationStore::with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    ));
    store.insert(1, 0, &actor);
    store.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    (store, actors)
}

#[test]
fn held_scale_keeps_tick_and_frame_script_order() {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    let mut ops = compiled.molang_ops.into_vec();
    let mut expressions = compiled.molang_expressions.into_vec();
    compiled.rig_bindings[0].scale_expressions = Some([expressions.len() as u32; 4]);
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 5,
        max_stack: 2,
    });
    ops.extend([
        MolangOp::LoadVariable(7),
        MolangOp::Push(EntityGeometryScalar::new(1.0).unwrap()),
        MolangOp::Add,
        MolangOp::StoreVariable(7),
        MolangOp::Push(EntityGeometryScalar::new(1.0).unwrap()),
    ]);
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    let (store, actors) = fixture_with(compiled);
    let before = authored_state(&store);
    let tick = store.get(1).unwrap();
    assert_eq!(
        tick.held_item_scale,
        Some(2.0),
        "held scale reads the initialized value before pre-animation and model scale"
    );
    for alpha in [0.25, 0.75] {
        let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], alpha);
        assert_eq!(
            frame.sample_held_item_scale(tick).held_item_scale,
            tick.held_item_scale.map(|value| value + alpha),
            "held scale uses the same script order at the tick and each frame"
        );
        assert_eq!(authored_state(&store), before);
    }
}

#[test]
fn held_scale_samples_once_without_committing_owner_variables() {
    let (store, actors) = fixture();
    let before = authored_state(&store);
    let tick = store.get(1).unwrap();
    for alpha in [0.25, 0.75] {
        let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], alpha);
        frame.layers(1).unwrap();
        let body_remaining = frame.remaining_ops();
        let sampled = frame.sample_held_item_scale(tick);
        assert_eq!(
            sampled.held_item_scale,
            tick.held_item_scale.map(|scale| scale + alpha)
        );
        let remaining = frame.remaining_ops();
        assert_eq!(
            remaining, body_remaining,
            "body-first publication reuses the same sample"
        );
        assert!(remaining < MAX_MOLANG_OPS_PER_RENDER_FRAME);
        assert_eq!(
            frame.layers_with_skin(1).unwrap().held_item_scale,
            sampled.held_item_scale
        );
        assert_eq!(
            frame.sample_held_item_scale(tick).held_item_scale,
            sampled.held_item_scale
        );
        assert_eq!(frame.remaining_ops(), remaining);
        assert_eq!(authored_state(&store), before);
        assert_eq!(store.get(1).unwrap().held_item_scale, tick.held_item_scale);
    }
}

#[test]
fn held_scale_lifetime_stays_live_during_isolated_rotation_sampling() {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    compiled.molang_symbols[1].identifier = "query.body_y_rotation".into();
    compiled.molang_symbols[2].identifier = "query.life_time".into();
    compiled.molang_ops[10] = MolangOp::LoadQuery(1);
    let (mut store, mut actors) = fixture_with_query(compiled, 2);
    let actor = actors.get_mut(&1).unwrap();
    actor.yaw = 90.0;
    actor.body_yaw = 90.0;
    store.advance_tick(&actors, None, None, true, true, |_| Default::default());
    let before = authored_state(&store);
    let tick = store.get(1).unwrap();
    for alpha in [0.25, 0.75] {
        let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], alpha);
        let sampled = frame.sample_held_item_scale(tick).held_item_scale.unwrap();
        let expected = tick.held_item_scale.unwrap() + alpha * ACTOR_TICK_DURATION.as_secs_f32();
        assert!(
            (sampled - expected).abs() < 1.0e-6,
            "lifetime follows the frame while rotation is sampled: {sampled} vs {expected}"
        );
        assert_eq!(authored_state(&store), before);
    }
}

#[test]
fn exhausted_held_scale_sample_keeps_completed_tick_value() {
    let (store, actors) = fixture();
    let tick = store.get(1).unwrap();
    for budget in [0, 1] {
        let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.5);
        frame.set_remaining_ops(budget);
        assert_eq!(
            frame.sample_held_item_scale(tick).held_item_scale,
            tick.held_item_scale
        );
    }
}

#[test]
fn culled_held_scale_samples_without_changing_completed_state() {
    let (mut store, actors) = fixture();
    store.rigs.values_mut().next().unwrap().culled = true;
    let tick = store.get(1).unwrap();
    let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.5);
    assert_eq!(
        frame.sample_held_item_scale(tick).held_item_scale,
        tick.held_item_scale.map(|scale| scale + 0.5)
    );
}

#[test]
fn nonpositive_held_scale_keeps_unit_size() {
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    for value in [0.0, -2.0] {
        let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
        let mut ops = compiled.molang_ops.into_vec();
        let mut expressions = compiled.molang_expressions.into_vec();
        compiled.rig_bindings[0].held_item_scale = Some(expressions.len() as u32);
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 1,
            max_stack: 1,
        });
        ops.push(MolangOp::Push(EntityGeometryScalar::new(value).unwrap()));
        compiled.molang_ops = ops.into();
        compiled.molang_expressions = expressions.into();
        let mut store = ActorAnimationStore::with_assets(Arc::new(
            RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
        ));
        store.insert(1, 0, &actor);
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        assert_eq!(store.get(1).unwrap().held_item_scale, Some(1.0));
    }
}
