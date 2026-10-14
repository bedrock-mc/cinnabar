use super::*;
use assets::{
    CompiledMolangExpression, EntityAnimationProperty, MolangFunction, MolangOp, MolangSymbol,
    MolangSymbolKind,
};

/// A limb swinging as `cos(stride·38.17)·speed·57.3`, the vanilla walk curve, under a layer
/// tinted by the stride.
fn walk_assets() -> Arc<RuntimeEntityAssets> {
    let scalar = crate::test_support::scalar;
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:player");
    compiled.rig_bindings[0].initialize = None;
    compiled.rig_bindings[0].pre_animation = None;
    compiled.render.layers[0].color = Some([1; 4]);
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.modified_distance_moved"),
        (MolangSymbolKind::Query, "query.modified_move_speed"),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    compiled.molang_ops = vec![
        MolangOp::LoadQuery(1),
        MolangOp::Push(scalar(38.17)),
        MolangOp::Multiply,
        MolangOp::Call(MolangFunction::Cos),
        MolangOp::LoadQuery(2),
        MolangOp::Multiply,
        MolangOp::Push(scalar(57.3)),
        MolangOp::Multiply,
        MolangOp::LoadQuery(1),
    ]
    .into();
    compiled.molang_expressions = [(0, 8, 2), (8, 1, 1)]
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .into();
    compiled.animation_channels[0].property = EntityAnimationProperty::Rotation;
    compiled.animation_keyframes[0].expressions = [Some(0), None, None];
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// An accelerating walker's store with its previous and current tick inputs.
fn walker(context: ActorTickContext) -> (ActorAnimationStore, TestActor, [ActorTickInput; 2]) {
    let mut actor = TestActor {
        kind: ActorKind::Player {
            uuid: [1; 16],
            username: "walker".into(),
        },
        on_ground: Some(true),
        ..Default::default()
    };
    let mut store = ActorAnimationStore::with_assets(walk_assets());
    store.insert(1, 0, &actor);
    let exempt = context.is_local.then_some(1);
    for step in [0.1, 0.2, 0.25, 0.3, 0.32, 0.35] {
        actor.previous_position = actor.position;
        actor.position[0] += step;
        let actors = HashMap::from([(1, actor.clone())]);
        store.advance_tick(&actors, None, exempt, true, true, |_| context.clone());
    }
    let state = store.rigs.values().next().unwrap();
    let mut recent = state.history.iter().rev();
    let (current, previous) = (*recent.next().unwrap(), *recent.next().unwrap());
    assert_ne!(previous.distance_moved, current.distance_moved);
    (store, actor, [previous, current])
}

/// The drawn limb follows the walk curve at the frame's interpolated stride and speed, not a
/// blend of the two tick poses, in third and first person.
#[test]
fn walking_limbs_evaluate_the_walk_curve_at_the_frame_fraction() {
    let first_person = ActorTickContext {
        is_local: true,
        is_local_player: true,
        is_local_first_person: true,
        ..Default::default()
    };
    for context in [ActorTickContext::default(), first_person] {
        let (store, actor, [previous, current]) = walker(context.clone());
        for alpha in [0.25, 0.5, 0.75] {
            let lerp = |from: f32, to: f32| from + (to - from) * alpha;
            let distance = lerp(previous.distance_moved, current.distance_moved);
            let speed = lerp(previous.move_speed, current.move_speed);
            let angle = (distance * 38.17).to_radians().cos() * speed * 57.3;
            let expected = pose::quat_from_euler([-angle, 0.0, 0.0]);
            let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
            let layers = store
                .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut budget, false)
                .unwrap();
            let rig = store.get(1).unwrap();
            let (from, to) = match &layers.render {
                Cow::Owned(layers) if !layers[0].pose.is_empty() => {
                    (layers[0].previous_pose[0], layers[0].pose[0])
                }
                _ => (rig.previous[0], rig.current[0]),
            };
            for ((from, to), expected) in from.rotation.into_iter().zip(to.rotation).zip(expected) {
                let drawn = from + (to - from) * alpha;
                assert!(
                    (drawn - expected).abs() < 1.0e-4,
                    "walk at {alpha}, first person {}: {drawn} vs {expected}",
                    context.is_local_first_person
                );
            }
        }
    }
}

/// Unblended walk-driven layer values start each tick from the previous stride, at fraction 0 too.
#[test]
fn walk_driven_layer_values_never_jump_at_the_tick_edge() {
    let (store, actor, [previous, current]) = walker(ActorTickContext::default());
    for alpha in [0.0, 0.5, 0.99] {
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let layers = store
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut budget, false)
            .unwrap();
        let expected =
            previous.distance_moved + (current.distance_moved - previous.distance_moved) * alpha;
        assert!(
            (layers.render[0].color[0] - expected).abs() < 1.0e-4,
            "tint at {alpha}: {} vs {expected}",
            layers.render[0].color[0]
        );
    }
}
