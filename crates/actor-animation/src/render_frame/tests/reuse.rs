use super::*;

/// Two independently tinted layers; only the second reads the frame fraction.
fn fixture() -> (ActorAnimationStore, TestActor) {
    let actor = TestActor::default();
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    compiled.rig_bindings[0].pre_animation = None;
    compiled.animation_keyframes[0].expressions = [None; 3];
    compiled.render.layers[0].color = Some([1; 4]);
    let mut layers = compiled.render.layers.to_vec();
    let mut layer = layers[0];
    layer.color = Some([3; 4]);
    let mut slots = compiled.render.slots.to_vec();
    let mut candidates = compiled.render.candidates.to_vec();
    let mut slot = slots[0];
    slot.first_candidate = candidates.len() as u32;
    candidates.push(candidates[0]);
    layer.first_slot = slots.len() as u32;
    slots.push(slot);
    compiled.render.slots = slots.into();
    compiled.render.candidates = candidates.into();
    layers.push(layer);
    compiled.render.layers = layers.into();
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    store.advance_tick(
        &HashMap::from([(1, actor.clone())]),
        None,
        None,
        true,
        false,
        |_| ActorTickContext::default(),
    );
    (store, actor)
}

/// Samples through the store so each new consumer exercises the retained actor state.
fn sample(store: &ActorAnimationStore, actor: &TestActor, alpha: f32) -> Vec<RenderTextureLayer> {
    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
    store
        .render_layers(actor, alpha, [0.0; 2], [0.0; 3], &mut budget, true)
        .unwrap()
        .render
        .into_owned()
}

#[test]
fn unchanged_frame_inputs_run_no_layer_bytecode() {
    let (store, actor) = fixture();
    let first = sample(&store, &actor, 0.25);
    evaluation::take_expression_work();
    assert_eq!(sample(&store, &actor, 0.25), first);
    assert_eq!(evaluation::take_expression_work().iter().sum::<usize>(), 0);
}

#[test]
fn changed_fraction_evaluates_only_its_dependent_layer() {
    let (store, actor) = fixture();
    let first = sample(&store, &actor, 0.25);
    evaluation::take_expression_work();
    let second = sample(&store, &actor, 0.75);
    let work = evaluation::take_expression_work();
    assert_eq!(second[0], first[0]);
    assert_ne!(second[1].color, first[1].color);
    assert_eq!(work[1], 0);
    assert_eq!(work[3], 1);
}

/// Walk and attack queries feed separate axes while the layer reads the current frame fraction.
fn moving_fixture() -> (ActorAnimationStore, HashMap<u64, TestActor>) {
    let actor = TestActor {
        kind: ActorKind::Player {
            uuid: [1; 16],
            username: "fixture".into(),
        },
        ..Default::default()
    };
    let mut compiled = crate::test_support::counting_random_compiled(
        assets::facts::VanillaEntity::Player.identifier(),
    );
    compiled.rig_bindings[0].pre_animation = None;
    compiled.molang_symbols[4].identifier = "query.modified_distance_moved".into();
    compiled.molang_symbols[7].identifier = "variable.attack_time".into();
    let mut ops = compiled.molang_ops.to_vec();
    ops[9] = MolangOp::LoadQuery(4);
    compiled.molang_ops = ops.into();
    let mut expressions = compiled.molang_expressions.to_vec();
    expressions.push(assets::CompiledMolangExpression {
        first_op: compiled.molang_ops.len() as u32,
        op_count: 1,
        max_stack: 1,
    });
    let mut ops = compiled.molang_ops.to_vec();
    ops.push(MolangOp::LoadVariable(7));
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled.animation_keyframes[0].expressions = [Some(1), Some(4), None];
    compiled.render.layers[0].color = Some([3; 4]);
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    (store, HashMap::from([(1, actor)]))
}

#[test]
fn cached_walk_and_swing_match_uncached_samples_and_retain_pose_allocations() {
    let (mut store, mut actors) = moving_fixture();
    for tick in 0..12 {
        let actor = actors.get_mut(&1).unwrap();
        actor.previous_position = actor.position;
        actor.position[0] += 0.1 + tick as f32 * 0.01;
        if tick % 3 == 0 {
            store.start_swing(1, 6, 6);
        }
        store.advance_tick(&actors, None, None, true, false, |_| {
            ActorTickContext::default()
        });
        let authored = authored_state(&store);
        for alpha in [0.0, 0.25, 0.5, 0.75, 0.99, 0.25] {
            let cached = sample(&store, &actors[&1], alpha);
            let expected = cache::uncached(|| sample(&store, &actors[&1], alpha));
            assert_eq!(cached, expected, "tick {tick}, fraction {alpha}");
            evaluation::take_expression_work();
            let repeated = sample(&store, &actors[&1], alpha);
            assert_eq!(evaluation::take_expression_work().iter().sum::<usize>(), 0);
            assert!(Arc::ptr_eq(&cached[0].pose, &repeated[0].pose));
            assert!(Arc::ptr_eq(
                &cached[0].previous_pose,
                &repeated[0].previous_pose
            ));
            assert_eq!(authored_state(&store), authored);
        }
    }
}

#[test]
fn cached_random_and_variable_effects_match_the_ordinary_replay() {
    let actor = TestActor::default();
    let mut store = ActorAnimationStore::with_assets(crate::test_support::counting_random_assets());
    store.insert(1, 0, &actor);
    let actors = HashMap::from([(1, actor.clone())]);
    for _ in 0..3 {
        store.advance_tick(&actors, None, None, true, false, |_| {
            ActorTickContext::default()
        });
        let authored = authored_state(&store);
        for alpha in [0.25, 0.25, 0.75, 0.25] {
            let cached = sample(&store, &actor, alpha);
            assert_eq!(cached, cache::uncached(|| sample(&store, &actor, alpha)));
            assert_eq!(authored_state(&store), authored);
        }
    }
}

#[test]
fn held_tick_history_invalidates_a_retained_frame_sample() {
    let (mut store, mut actors) = moving_fixture();
    for position in [0.1, 0.4] {
        let actor = actors.get_mut(&1).unwrap();
        actor.previous_position = actor.position;
        actor.position[0] = position;
        store.advance_tick(&actors, None, None, true, false, |_| {
            ActorTickContext::default()
        });
    }
    let before = sample(&store, &actors[&1], 0.5);
    store
        .rigs
        .values_mut()
        .next()
        .unwrap()
        .render_frame
        .as_mut()
        .unwrap()
        .hold_motion();
    let after = sample(&store, &actors[&1], 0.5);
    assert_eq!(after, cache::uncached(|| sample(&store, &actors[&1], 0.5)));
    assert_ne!(after, before);
}

#[test]
fn unchanged_frames_replay_each_occurrence_of_a_side_effecting_expression() {
    let actor = TestActor::default();
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    compiled.rig_bindings[0].pre_animation = None;
    compiled.animation_keyframes[0].expressions = [None; 3];
    let program = [
        MolangOp::LoadVariable(7),
        MolangOp::Push(crate::test_support::scalar(1.0)),
        MolangOp::Add,
        MolangOp::StoreVariable(7),
        MolangOp::Push(crate::test_support::scalar(0.0)),
        MolangOp::CallQuery(assets::MolangCall {
            symbol: 1,
            arguments: 1,
        }),
        MolangOp::Pop,
        MolangOp::LoadVariable(7),
    ];
    let mut expressions = compiled.molang_expressions.to_vec();
    let expression = expressions.len() as u32;
    expressions.push(assets::CompiledMolangExpression {
        first_op: compiled.molang_ops.len() as u32,
        op_count: program.len() as u16,
        max_stack: 2,
    });
    let mut ops = compiled.molang_ops.to_vec();
    ops.extend(program);
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled.render.layers[0].color = Some([expression; 4]);
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    store.advance_tick(
        &HashMap::from([(1, actor.clone())]),
        None,
        None,
        true,
        false,
        |_| ActorTickContext::default(),
    );
    let authored = authored_state(&store);
    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
    let first = store
        .render_layers(&actor, 0.25, [0.0; 2], [0.0; 3], &mut budget, false)
        .unwrap();
    assert!(matches!(first.render, Cow::Owned(_)));
    assert_eq!(first.render[0].color, [1.0, 2.0, 3.0, 4.0]);
    evaluation::take_expression_work();
    assert_eq!(sample(&store, &actor, 0.25), first.render.as_ref());
    assert_eq!(evaluation::take_expression_work().iter().sum::<usize>(), 0);
    assert_eq!(authored_state(&store), authored);
}
