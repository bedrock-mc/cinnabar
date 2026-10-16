use super::*;
use assets::{CompiledMolangExpression, EntityGeometryScalar};

/// Adds a scale that reads the counting pre-animation variable and this frame's fraction.
fn fixture() -> (
    ActorAnimationStore,
    HashMap<u64, crate::test_actor::TestActor>,
) {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    let mut ops = compiled.molang_ops.into_vec();
    let mut expressions = compiled.molang_expressions.into_vec();
    compiled.rig_bindings[0].held_item_scale = Some(expressions.len() as u32);
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 3,
        max_stack: 2,
    });
    ops.extend([
        MolangOp::LoadVariable(7),
        MolangOp::LoadQuery(1),
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
