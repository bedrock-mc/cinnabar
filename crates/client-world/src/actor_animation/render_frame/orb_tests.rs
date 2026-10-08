use super::*;

fn installed_assets() -> Option<Arc<RuntimeEntityAssets>> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping installed XP animation fixture: {} is absent",
                root.display()
            );
            return None;
        }
        Err(error) => panic!("read {}: {error}", root.display()),
    };
    let paths = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
        .collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!(
            "skipping installed XP animation fixture: no entity carrier in {}",
            root.display()
        );
        return None;
    }
    assert_eq!(
        paths.len(),
        1,
        "ambiguous installed entity carriers: {paths:?}"
    );
    Some(Arc::new(
        RuntimeEntityAssets::decode(&std::fs::read(&paths[0]).unwrap()).unwrap(),
    ))
}

fn installed_orb() -> Option<crate::actor_store::ActorStore> {
    let assets = installed_assets()?;
    let mut store = crate::actor_store::ActorStore::new_with_entity_assets(1, 0, assets);
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: protocol::ActorKind::Entity {
                identifier: "minecraft:xp_orb".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(2);
    Some(store)
}

#[test]
fn installed_xp_colour_uses_current_actor_age_when_world_animation_budget_is_exhausted() {
    let Some(assets) = installed_assets() else {
        return;
    };
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.kind = protocol::ActorKind::Entity {
        identifier: "minecraft:xp_orb".into(),
    };
    actor.runtime_id = u64::MAX;
    let mut actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut starved = ActorAnimationStore::with_assets(Arc::clone(&assets));
    let mut visible = ActorAnimationStore::with_assets(assets);
    for animation in [&mut starved, &mut visible] {
        animation.insert(1, 0, &actor);
        for _ in 0..2 {
            animation.advance_tick(&actors, None, None, true, true, |_| {
                ActorTickContext::default()
            });
        }
    }
    let before_ops = visible.stats().evaluated_molang_ops;
    visible.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    let actor_ops = visible.stats().evaluated_molang_ops - before_ops;
    assert!(actor_ops > 0);
    let pressure_actors = MAX_MOLANG_OPS_PER_WORLD_TICK / actor_ops as usize + 2;
    assert!(pressure_actors < crate::actor_store::MAX_TRACKED_ACTORS);
    for runtime_id in 1..=pressure_actors as u64 {
        let mut pressure = actor.clone();
        pressure.runtime_id = runtime_id;
        pressure.unique_id = runtime_id as i64;
        starved.insert(1, 0, &pressure);
        actors.insert(runtime_id, pressure);
    }
    let held = starved.get(actor.runtime_id).unwrap();
    let held_tick = held.completed_tick;
    let held_pose = held.current.to_vec();
    let held_layers = held.render.to_vec();
    starved.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    assert!(starved.stats().world_budget_exhaustions > 0);
    assert_eq!(
        starved.get(actor.runtime_id).unwrap().completed_tick,
        held_tick
    );
    let stats = starved.stats();
    for alpha in [0.25, 0.75] {
        let mut expected_ops = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let expected = visible
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut expected_ops, false)
            .unwrap();
        let mut actual_ops = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let actual = starved
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut actual_ops, false)
            .unwrap();
        assert_eq!(
            actual.render[0].overlay, expected.render[0].overlay,
            "rendered XP colour must use live actor age while the world budget holds its pose"
        );
    }
    assert_eq!(starved.get(actor.runtime_id).unwrap().current, held_pose);
    assert_eq!(starved.get(actor.runtime_id).unwrap().render, held_layers);
    assert_eq!(starved.stats(), stats);
}

#[test]
fn installed_xp_colour_advances_between_ticks_without_mutating_actor_or_rig_state() {
    let Some(mut store) = installed_orb() else {
        return;
    };
    let rig = store.actor_rig(1).unwrap();
    let pose = rig.current.to_vec();
    let completed_tick = rig.completed_tick;
    let tick_layers = rig.render.to_vec();
    let actor = store.get(1).unwrap().clone();
    let stats = store.animation_stats();
    let early = store.render_frame(0.25).layers(1).unwrap().into_owned();
    let late = store.render_frame(0.75).layers(1).unwrap().into_owned();
    assert!(
        (late[0].overlay[0] - early[0].overlay[0]).abs() > 0.001,
        "the authored XP lifetime overlay must animate within a single completed tick: early {:?}, late {:?}",
        early[0].overlay,
        late[0].overlay
    );
    assert_eq!(early[0].overlay[1], 1.0);
    assert_eq!(late[0].overlay[3], 0.5);
    assert_eq!(early, store.render_frame(0.25).layers(1).unwrap().as_ref());
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
    assert_eq!(store.actor_rig(1).unwrap().current, pose);
    assert_eq!(store.actor_rig(1).unwrap().render, tick_layers);
    assert_eq!(store.get(1).unwrap(), &actor);
    assert_eq!(store.animation_stats(), stats);
    store.advance_interpolation_ticks(1);
    assert_ne!(
        store.actor_rig(1).unwrap().render[0].overlay,
        tick_layers[0].overlay
    );
}

#[test]
fn installed_xp_colour_uses_current_actor_age_after_culled_ticks_without_a_new_tick() {
    let (Some(mut culled), Some(mut visible)) = (installed_orb(), installed_orb()) else {
        return;
    };
    culled.set_animation_view(Some(ActorAnimationView {
        planes: [[0.0, 0.0, 0.0, -1.0]; 6],
        camera: [0.0; 3],
        player_distance: 100.0,
        entity_radius: 100.0,
    }));
    culled.advance_interpolation_ticks(7);
    visible.advance_interpolation_ticks(7);
    culled.set_animation_view(None);
    let rig = culled.actor_rig(1).unwrap();
    let completed_tick = rig.completed_tick;
    let pose = rig.current.to_vec();
    let tick_layers = rig.render.to_vec();
    let actor = culled.get(1).unwrap().clone();
    let stats = culled.animation_stats();
    for alpha in [0.0, 0.25, 0.75] {
        let expected = visible.render_frame(alpha).layers(1).unwrap().into_owned();
        let actual = culled.render_frame(alpha).layers(1).unwrap().into_owned();
        assert_eq!(
            actual[0].overlay, expected[0].overlay,
            "rendered XP colour must follow the same actor age after culled ticks: frame fraction {alpha}"
        );
    }
    assert_eq!(culled.actor_rig(1).unwrap().completed_tick, completed_tick);
    assert_eq!(culled.actor_rig(1).unwrap().current, pose);
    assert_eq!(culled.actor_rig(1).unwrap().render, tick_layers);
    assert_eq!(culled.get(1).unwrap(), &actor);
    assert_eq!(culled.animation_stats(), stats);
}
