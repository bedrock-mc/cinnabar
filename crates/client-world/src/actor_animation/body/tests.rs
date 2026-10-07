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
    let mut actor = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
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
        let first_context = |_: &ActorSnapshot| ActorTickContext {
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
