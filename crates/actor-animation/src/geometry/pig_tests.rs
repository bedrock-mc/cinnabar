use super::super::*;

#[test]
fn pinned_pig_walk_keeps_each_leg_at_its_authored_hip() {
    let Some(assets) = super::fixtures::entity_assets() else {
        return;
    };
    let mut actor = tests::actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:pig".into(),
    };
    actor.on_ground = Some(true);
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.insert(1, 0, &actor);
    let legs = [
        ("leg0", [3.0, 6.0, 6.0]),
        ("leg1", [-3.0, 6.0, 6.0]),
        ("leg2", [3.0, 6.0, -6.0]),
        ("leg3", [-3.0, 6.0, -6.0]),
    ];
    let mut moved = [false; 4];
    for step in 0..=12 {
        actor.position[2] = step as f32 * 0.1;
        store.advance_tick(
            &HashMap::from([(actor.runtime_id, actor.clone())]),
            None,
            None,
            true,
            false,
            |_| ActorTickContext::default(),
        );
        let rig = store.get(actor.runtime_id).expect("pig rig");
        let geometry = assets.rig_geometries()[rig.rig.0 as usize].geometry as usize;
        assert_eq!(
            &*assets.geometries()[geometry].identifier,
            "geometry.pig.v3"
        );
        assert_eq!(rig.scale, 1.0);
        for (slot, (name, pivot)) in legs.iter().enumerate() {
            let index = rig
                .bone_names
                .iter()
                .position(|bone| &**bone == *name)
                .unwrap();
            let leg = &rig.current[index];
            for (actual, expected) in leg.translation_scale[..3].iter().zip(pivot) {
                assert!(
                    (actual - expected).abs() < 1e-4,
                    "{name} hip: {actual} != {expected}"
                );
            }
            assert_eq!(leg.axis_scale, [1.0; 3]);
            moved[slot] |= leg.rotation != rig.rest[index].rotation;
        }
        assert_eq!(store.stats().frozen_actors, 0);
    }
    assert!(
        moved.into_iter().all(|moved| moved),
        "all four pig legs walk"
    );
}
