use super::*;

mod first_presentation;

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
    let mut store = ActorAnimationStore::with_assets(
        crate::actor_animation::test_support::counting_random_assets(),
    );
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
