use super::*;
use assets::{MolangCall, MolangSymbol, MolangSymbolKind};

/// Builds a non-player rig whose only dynamic expression is its held-item scale.
fn motion_fixture(
    query: &str,
    call: bool,
    reselect: bool,
) -> (
    ActorAnimationStore,
    HashMap<u64, crate::test_actor::TestActor>,
) {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    compiled.rig_bindings[0].pre_animation = None;
    compiled.rig_bindings[0].held_item_scale = Some(0);
    compiled.render.layers[0].color = Some([1; 4]);
    compiled.animation_keyframes[0].expressions = [None; 3];
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, query),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    let read = if call {
        MolangOp::CallQuery(MolangCall {
            symbol: 1,
            arguments: 0,
        })
    } else {
        MolangOp::LoadQuery(1)
    };
    compiled.molang_ops = vec![
        MolangOp::Push(crate::test_support::scalar(1.0)),
        read,
        MolangOp::Add,
        MolangOp::Push(crate::test_support::scalar(1.0)),
        read,
        MolangOp::Push(crate::test_support::scalar(0.0)),
        MolangOp::Greater,
    ]
    .into();
    compiled.molang_expressions = [(0, 3, 2), (3, 1, 1), (4, 3, 2)]
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .into();
    if reselect {
        let mut choices = compiled.rig_geometries.to_vec();
        let mut moving = choices[0];
        moving.condition = Some(2);
        moving.first_animation = compiled.rig_animations.len() as u32;
        choices.push(moving);
        compiled.rig_geometries = choices.into();
        compiled.rig_bindings[0].geometry_count = 2;
        compiled.rig_animations = vec![compiled.rig_animations[0]; 2].into();
    }
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut store = ActorAnimationStore::with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    ));
    store.insert(1, 0, &actor);
    store.advance_tick(&actors, None, None, true, true, |_| Default::default());
    (store, actors)
}

/// Checks interpolation, frame reuse, budget fallback and tick-state isolation after motion.
fn check_motion(query: &str, value: impl Fn(ActorTickInput) -> f32) {
    for call in [false, true] {
        for reselect in [false, true] {
            let (mut store, mut actors) = motion_fixture(query, call, reselect);
            for (position, yaw) in [(0.25, 90.0), (0.6, 135.0)] {
                let actor = actors.get_mut(&1).unwrap();
                actor.previous_position = actor.position;
                actor.position[0] = position;
                actor.yaw = yaw;
                actor.head_yaw = yaw;
                store.advance_tick(&actors, None, None, true, true, |_| Default::default());
            }
            let state = store.rigs.values().next().unwrap();
            assert_eq!(state.geometry_binding, usize::from(reselect));
            let mut history = state.history.iter().rev();
            let current = value(*history.next().unwrap());
            let previous = value(*history.next().unwrap());
            assert_ne!(previous, current);
            let before = authored_state(&store);
            let tick = store.get(1).unwrap();
            assert!((tick.held_item_scale.unwrap() - (1.0 + current)).abs() < 1.0e-5);
            for alpha in [0.0, 0.25, 0.75, 1.0] {
                let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], alpha);
                let expected = 1.0 + previous + (current - previous) * alpha;
                let sampled = frame.sample_held_item_scale(tick).held_item_scale.unwrap();
                assert!(
                    (sampled - expected).abs() < 1.0e-5,
                    "{query} at {alpha}, call {call}, reselect {reselect}: {sampled} vs {expected}"
                );
                let remaining = frame.remaining_ops();
                assert!(remaining < MAX_MOLANG_OPS_PER_RENDER_FRAME);
                assert_eq!(
                    frame.layers_with_skin(1).unwrap().held_item_scale,
                    Some(sampled)
                );
                assert_eq!(frame.remaining_ops(), remaining);
                assert_eq!(authored_state(&store), before);
            }
            for budget in [0, 1] {
                let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.5);
                frame.set_remaining_ops(budget);
                assert_eq!(
                    frame.sample_held_item_scale(tick).held_item_scale,
                    tick.held_item_scale
                );
                assert_eq!(authored_state(&store), before);
            }
        }
    }
}

#[test]
fn non_player_held_scale_samples_walk_queries() {
    check_motion("query.modified_move_speed", |input| input.move_speed);
    check_motion("query.modified_distance_moved", |input| {
        input.distance_moved
    });
}

#[test]
fn non_player_held_scale_samples_rotation_queries() {
    check_motion("query.body_y_rotation", |input| input.body_yaw);
}
