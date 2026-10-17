use super::*;

fn installed_assets() -> Option<Arc<RuntimeEntityAssets>> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping local world-body animation: missing entity fixture {}",
                root.display()
            );
            return None;
        }
        Err(error) => panic!("read {}: {error}", root.display()),
    };
    let mut paths: Vec<_> = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
        .collect();
    paths.sort();
    let Some(path) = paths.first() else {
        eprintln!(
            "skipping local world-body animation: missing entity fixture {}/*.mcbeent",
            root.display()
        );
        return None;
    };
    Some(Arc::new(
        RuntimeEntityAssets::decode(&std::fs::read(path).unwrap()).unwrap(),
    ))
}

#[test]
fn first_person_shadow_body_matches_walking_and_crouching_world_pose_without_changing_hands() {
    let Some(assets) = installed_assets() else {
        return;
    };
    let mut actor = crate::tests::actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "Offline".into(),
    };
    let mut first = ActorAnimationStore::with_assets(Arc::clone(&assets));
    let mut vanilla = ActorAnimationStore::with_assets(Arc::clone(&assets));
    let mut third = ActorAnimationStore::with_assets(assets);
    for store in [&mut first, &mut vanilla, &mut third] {
        store.insert(1, 0, &actor);
    }
    assert!(first.set_local_body_enabled(true));
    let mut actors = HashMap::from([(1, actor)]);
    let mut prior_body = None;
    for tick in 0..8 {
        let actor = actors.get_mut(&1).unwrap();
        actor.position[0] = tick as f32 * 0.2;
        actor.on_ground = Some(true);
        actor.metadata.insert(
            0,
            ActorMetadataValue::Flags(if tick >= 4 { 1 << 1 } else { 0 }),
        );
        let first_context = |_: &TestActor| ActorTickContext {
            is_local_first_person: true,
            ..Default::default()
        };
        first.advance_tick(&actors, None, Some(1), true, false, first_context);
        vanilla.advance_tick(&actors, None, Some(1), true, false, first_context);
        third.advance_tick(&actors, None, Some(1), true, false, |_| {
            ActorTickContext::default()
        });
        let body = first.world_body(1).expect("world body pose");
        let world = third.get(1).unwrap();
        assert_eq!(body.current, world.current, "world body at tick {tick}");
        if let Some(previous) = &prior_body {
            assert_eq!(body.previous, previous, "tick endpoints interpolate");
        }
        assert_eq!(
            first.get(1).unwrap().current,
            vanilla.get(1).unwrap().current,
            "hand pose"
        );
        assert_eq!(first.ui_pose(1), vanilla.ui_pose(1), "HUD pose");
        assert!(
            vanilla.world_body(1).is_none(),
            "ordinary rendering creates no body carrier"
        );
        prior_body = Some(body.current.to_vec());
    }
    let body = first.world_body(1).unwrap();
    assert_ne!(
        body.current, body.rest,
        "walking crouch uses authored animation"
    );
    let (previous, current, tick) = (
        body.previous.to_vec(),
        body.current.to_vec(),
        body.completed_tick,
    );
    first.refresh_local_view(&actors, 1, |_| ActorTickContext {
        is_local_first_person: true,
        ..Default::default()
    });
    first.refresh_local_view(&actors, 1, |_| ActorTickContext {
        is_local_first_person: true,
        ..Default::default()
    });
    let refreshed = first.world_body(1).unwrap();
    assert_eq!(refreshed.completed_tick, tick);
    assert_eq!(refreshed.previous, previous);
    assert_eq!(
        refreshed.current, current,
        "refresh does not advance body clip clocks twice"
    );
    assert!(first.set_local_body_enabled(false));
    first.refresh_local_view(&actors, 1, |_| ActorTickContext {
        is_local_first_person: true,
        ..Default::default()
    });
    assert!(first.world_body(1).is_none());
}

/// A body enabled after first-person initialisation runs its own initialisation script
/// instead of inheriting values the first-person context cached there.
#[test]
fn a_body_enabled_after_initialisation_initialises_in_the_world_context() {
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(
            crate::test_support::initialized_pose_compiled().with_unresolved_admissions(),
        )
        .unwrap(),
    );
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    let runtime_id = 1;
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.insert(1, 0, &actor);
    let actors = HashMap::from([(runtime_id, actor)]);
    let first_person = |_: &TestActor| ActorTickContext {
        is_local_first_person: true,
        ..Default::default()
    };
    store.advance_tick(&actors, None, Some(runtime_id), true, false, first_person);
    let slot = store.layout.named_slot(&assets, "variable.charge_amount");
    let lifetime = store.runtime_to_lifetime[&runtime_id];
    let state = store.rigs.get_mut(&lifetime).unwrap();
    assert!(state.initialized && state.world_body.is_none());
    // Stand in for a value the first-person context cached during initialisation.
    state.variables.set(slot, -100.0);

    assert!(store.set_local_body_enabled(true));
    store.advance_tick(&actors, None, Some(runtime_id), true, false, first_person);
    let state = &store.rigs[&lifetime];
    let body = state.world_body.as_ref().expect("world body evaluated");
    assert!(body.initialized);
    assert!(
        body.variables.get(slot).is_some_and(|value| value > 0.0),
        "the body ran its own initialisation script"
    );
    assert!(
        state.variables.get(slot).is_some_and(|value| value < 0.0),
        "the first-person evaluator is not re-initialised"
    );
}

#[test]
fn world_body_keeps_owner_position_delta_item_context_and_animation_clock() {
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(
            crate::test_support::initialized_pose_compiled().with_unresolved_admissions(),
        )
        .unwrap(),
    );
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.insert(1, 0, &actor);
    store.set_local_body_enabled(true);
    let mut actors = HashMap::from([(1, actor)]);
    let context = |_: &TestActor| ActorTickContext {
        is_local_first_person: true,
        main_hand_is_spear: true,
        main_hand_swing_seconds: Some(0.75),
        ..Default::default()
    };
    store.advance_tick(&actors, None, Some(1), true, false, context);
    let delta = [0.25, -0.4, 0.5];
    for (position, movement) in actors.get_mut(&1).unwrap().position.iter_mut().zip(delta) {
        *position += movement;
    }
    store.advance_tick(&actors, None, Some(1), true, false, context);
    let slot = store.layout.named_slot(&assets, "variable.charge_amount");
    let lifetime = store.runtime_to_lifetime[&1];
    let state = store.rigs.get_mut(&lifetime).unwrap();
    state.animation_epoch = state.completed_tick.saturating_sub(1);
    state.history.back_mut().unwrap().swim_amount = 0.7;
    state.variables.set(slot, 99.0);
    state.world_body.as_mut().unwrap().variables.set(slot, 77.0);
    let owner = store.get(1).unwrap();
    let body = store.world_body(1).unwrap();
    let input = body
        .animation_variables
        .input()
        .expect("body retains owner movement queries");
    let owner_input = owner.animation_variables.input().unwrap();
    assert_eq!(input.position_delta, owner_input.position_delta);
    assert_eq!(input.swim_amount, owner_input.swim_amount);
    assert_eq!(input.swim_amount, 0.7);
    assert_eq!(body.animation_variables.anim_tick(), 1);
    assert_eq!(
        body.animation_variables.life_tick(),
        owner.animation_variables.life_tick()
    );
    assert!(body.animation_variables.is_spear());
    assert_eq!(body.animation_variables.item_timings().1, Some(0.75));
    assert_eq!(
        body.animation_variables
            .value("variable.charge_amount")
            .unwrap()
            .number(),
        77.0
    );
    assert_eq!(
        owner
            .animation_variables
            .value("variable.charge_amount")
            .unwrap()
            .number(),
        99.0
    );
    let query = crate::query::QueryInputs {
        actor: &actors[&1],
        input: &input,
        context: body.animation_variables.owner_context().unwrap(),
        anim_tick: body.animation_variables.anim_tick(),
        anim_time: None,
        swell_amount: None,
        life_tick: body.animation_variables.life_tick(),
        finished: (false, false),
        state_time: 0.0,
        bones: &[],
        bone_names: &[],
    };
    for (axis, expected) in delta.into_iter().enumerate() {
        let actual = crate::query::named_query(
            &query,
            "query.position_delta",
            &[MolangValue::Number(axis as f32)],
        )
        .number();
        assert!(
            (actual - expected).abs() < 0.00001,
            "axis {axis} keeps the owner's movement delta"
        );
    }
}
