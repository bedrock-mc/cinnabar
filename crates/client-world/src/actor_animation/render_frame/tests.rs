use super::*;
use assets::{CompiledMolangExpression, EntityAssetKind, EntityGeometryScalar, MolangFunction};

/// A `minecraft:test` rig whose pre-animation counts ticks and draws a random number, posing both.
pub(in crate::actor_animation) fn counting_random_assets() -> Arc<RuntimeEntityAssets> {
    let mut compiled = super::super::attachable::tests::compiled_fixture();
    compiled.sources[1].path = "entity/item.json".into();
    compiled.symbols[4].kind = EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:test".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 3;
    compiled.animation_clips[0].symbol = 2;
    let scalar = |value| EntityGeometryScalar::new(value).unwrap();
    compiled.molang_ops = vec![
        MolangOp::LoadVariable(7),
        MolangOp::Push(scalar(1.0)),
        MolangOp::Add,
        MolangOp::StoreVariable(7),
        MolangOp::Push(scalar(0.0)),
        MolangOp::Push(scalar(1.0)),
        MolangOp::Call(MolangFunction::Random),
        MolangOp::StoreVariable(5),
        MolangOp::Push(scalar(0.0)),
        MolangOp::LoadVariable(7),
        MolangOp::LoadVariable(5),
        MolangOp::LoadQuery(1),
    ]
    .into_boxed_slice();
    compiled.molang_expressions = [(0, 9, 2), (9, 1, 1), (10, 1, 1), (11, 1, 1)]
        .into_iter()
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.render.layers[0].color = Some([1, 2, 3, 3]);
    Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
}

fn fixture() -> crate::actor_store::ActorStore {
    let assets = counting_random_assets();
    let mut store = crate::actor_store::ActorStore::new_with_entity_assets(1, 0, assets);
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.unique_id = 17;
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: actor.unique_id,
            runtime_id: actor.runtime_id,
            kind: actor.kind,
            position: actor.position,
            velocity: actor.velocity,
            pitch: actor.pitch,
            yaw: actor.yaw,
            head_yaw: actor.head_yaw,
            body_yaw: actor.body_yaw,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(1);
    store
}

#[test]
fn render_frame_variables_and_rng_never_advance_the_completed_tick_state() {
    let mut store = fixture();
    let initial = store.actor_rig(1).unwrap();
    let tick_layers = initial.render.to_vec();
    let tick_pose = initial.current.to_vec();
    let completed = initial.completed_tick;
    let actor = store.get(1).unwrap().clone();
    let stats = store.animation_stats();
    let mut frame = store.render_frame(0.25);
    let first = frame.layers(1).unwrap();
    assert!(matches!(first, Cow::Owned(_)));
    assert_eq!(
        first[0].color[0], 1.0,
        "pre_animation must not increment twice"
    );
    assert_eq!(first[0].color[1], tick_layers[0].color[1]);
    assert_eq!(first[0].color[2..], [0.25; 2]);
    assert_eq!(
        frame.layers(1).unwrap(),
        first,
        "RNG sampling must be repeatable"
    );
    assert_eq!(
        store.render_frame(0.75).layers(1).unwrap()[0].color[2..],
        [0.75; 2]
    );
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed);
    assert_eq!(store.actor_rig(1).unwrap().current, tick_pose);
    assert_eq!(store.actor_rig(1).unwrap().render, tick_layers);
    assert_eq!(store.get(1).unwrap(), &actor);
    assert_eq!(store.animation_stats(), stats);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.actor_rig(1).unwrap().render[0].color[0], 2.0);
    assert_eq!(store.render_frame(0.5).layers(1).unwrap()[0].color[0], 2.0);
}

#[test]
fn render_frame_budget_and_invalid_fraction_keep_the_completed_layers() {
    let store = fixture();
    let tick_layers = store.actor_rig(1).unwrap().render;
    let mut frame = store.render_frame(0.5);
    frame.remaining_ops = 1;
    assert_eq!(frame.layers(1).unwrap().as_ref(), tick_layers);
    assert_eq!(frame.remaining_ops, 0);
    assert!(matches!(frame.layers(1).unwrap(), Cow::Borrowed(_)));
    for alpha in [f32::NAN, f32::INFINITY, -1.0] {
        assert!(matches!(
            store.render_frame(alpha).layers(1).unwrap(),
            Cow::Borrowed(_)
        ));
    }
    assert_eq!(
        store.render_frame(2.0).layers(1).unwrap()[0].color[2..],
        [1.0; 2]
    );
    assert!(frame.layers(999).is_none());
}

/// Captures only authored VM/controller/clock state that render evaluation must not commit.
fn authored_state(store: &ActorAnimationStore) -> String {
    let state = store.rigs.values().next().unwrap();
    format!(
        "{:?}",
        (&state.variables, &state.controllers, &state.clip_clocks)
    )
}

#[test]
fn native_complete_sample_keeps_authored_state_and_borrows_unchanged_skin_layers() {
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let mut store = ActorAnimationStore::with_assets(counting_random_assets());
    store.insert(1, 0, &actor);
    store.advance_tick(
        &HashMap::from([(actor.runtime_id, actor.clone())]),
        None,
        None,
        true,
        true,
        |_| ActorTickContext::default(),
    );
    let before = authored_state(&store);
    let stats = store.stats();
    let rig = store.get(actor.runtime_id).unwrap();
    let completed = (
        rig.completed_tick,
        rig.previous.to_vec(),
        rig.current.to_vec(),
        rig.render.to_vec(),
    );
    for alpha in [0.25, 0.75, 0.25] {
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let sampled = store
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut budget, true)
            .unwrap();
        assert!(matches!(sampled.render, Cow::Owned(_)));
        assert!(matches!(sampled.skin, Cow::Borrowed(_)));
        assert_eq!(sampled.render[0].color[0], 1.0);
        assert_eq!(sampled.render[0].color[2..], [alpha; 2]);
        assert_eq!(authored_state(&store), before);
        assert_eq!(store.stats(), stats);
    }
    let unchanged = store.get(actor.runtime_id).unwrap();
    assert_eq!(
        (
            unchanged.completed_tick,
            unchanged.previous.to_vec(),
            unchanged.current.to_vec(),
            unchanged.render.to_vec()
        ),
        completed
    );
}

#[test]
fn native_complete_sample_budget_failure_keeps_both_completed_slices_borrowed() {
    let store = fixture();
    let rig = store.actor_rig(1).unwrap();
    let mut frame = store.render_frame(0.5);
    frame.remaining_ops = 1;
    let sampled = frame.layers_with_skin(1).unwrap();
    assert!(matches!(sampled.render, Cow::Borrowed(_)));
    assert!(matches!(sampled.skin, Cow::Borrowed(_)));
    assert!(std::ptr::eq(sampled.render.as_ptr(), rig.render.as_ptr()));
    assert!(std::ptr::eq(
        sampled.skin.as_ptr(),
        rig.skin_layers.as_ptr()
    ));
    assert_eq!(frame.remaining_ops, 0);
    assert!(matches!(
        frame.layers_with_skin(1).unwrap().render,
        Cow::Borrowed(_)
    ));
    assert!(frame.layers_with_skin(999).is_none());
    for alpha in [f32::NAN, f32::INFINITY, -1.0] {
        let sampled = store.render_frame(alpha).layers_with_skin(1).unwrap();
        assert!(matches!(sampled.render, Cow::Borrowed(_)));
        assert!(matches!(sampled.skin, Cow::Borrowed(_)));
    }
}
