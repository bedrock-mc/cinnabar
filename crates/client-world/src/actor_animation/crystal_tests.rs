use super::*;

#[test]
fn installed_crystal_animates_nested_frames_and_obeys_show_bottom() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping crystal animation fixture: {} is absent",
                root.display()
            );
            return;
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
    if paths.is_empty() {
        eprintln!("skipping crystal animation fixture: no entity carrier");
        return;
    }
    assert_eq!(
        paths.len(),
        1,
        "ambiguous installed entity carrier fixture: {paths:?}"
    );
    let assets = Arc::new(RuntimeEntityAssets::decode(&std::fs::read(&paths[0]).unwrap()).unwrap());
    let mut actor =
        super::tests::actor_with_metadata(HashMap::from([(0, ActorMetadataValue::Flags(1 << 38))]));
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:ender_crystal".into(),
    };
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    let mut actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut previous = None;
    for _ in 0..3 {
        store.advance_tick(&actors, None, None, true, false, |_| {
            ActorTickContext::default()
        });
        let rig = store.get(1).expect("crystal rig");
        assert_eq!(rig.current.len(), 4);
        assert_eq!(rig.render.len(), 1);
        assert!(rig.render[0].hidden_bones.is_empty());
        let outer = rig
            .bone_names
            .iter()
            .position(|name| name.as_ref() == "outerglass")
            .unwrap();
        let inner = rig
            .bone_names
            .iter()
            .position(|name| name.as_ref() == "innerglass")
            .unwrap();
        let crystal = rig
            .bone_names
            .iter()
            .position(|name| name.as_ref() == "crystal")
            .unwrap();
        let base = rig
            .bone_names
            .iter()
            .position(|name| name.as_ref() == "base")
            .unwrap();
        assert_eq!(rig.current[base], rig.rest[base]);
        for index in [outer, inner, crystal] {
            assert_ne!(rig.current[index], rig.rest[index]);
        }
        if let Some(previous) = previous {
            assert_ne!(
                rig.current[outer], previous,
                "crystal rotation and bob advance"
            );
        }
        previous = Some(rig.current[outer]);
    }
    actors
        .get_mut(&1)
        .unwrap()
        .metadata
        .insert(0, ActorMetadataValue::Flags(0));
    store.advance_tick(&actors, None, None, true, false, |_| {
        ActorTickContext::default()
    });
    let rig = store.get(1).unwrap();
    let base = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "base")
        .unwrap();
    assert_eq!(rig.render[0].hidden_bones.as_ref(), &[base as u32]);
}
