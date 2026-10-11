use super::*;

#[test]
#[ignore = "requires CINNABAR_ENTITY_CARRIER pointing to the pinned compiled entity carrier"]
fn pinned_stationary_fish_swim_poses_advance_without_land_roll() {
    let bytes = std::fs::read(std::env::var_os("CINNABAR_ENTITY_CARRIER").unwrap()).unwrap();
    let assets = Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    for identifier in FISH {
        let mut actor = actor(identifier);
        actor.status.fluid = Some((true, false));
        let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
        store.insert(1, 0, &actor);
        let mut previous_tail = None;
        let mut distinct_tail_poses = 0;
        for amount in 1..=8 {
            tick(&mut store, &actor, true);
            assert_eq!(
                phase(&store, &actor),
                [amount as f32, (amount - 1) as f32],
                "{identifier} stationary native phase"
            );
            assert_eq!(variables(&store, &actor), phase(&store, &actor).map(Some));
            let snapshot = store.get(actor.runtime_id).expect("native fish rig");
            assert!(!snapshot.render.is_empty(), "{identifier} render binding");
            let tail = snapshot
                .bone_names
                .iter()
                .position(|name| name.as_ref() == "tailfin")
                .expect("pinned fish tailfin");
            let tail_pose = snapshot.current[tail];
            if previous_tail.is_some_and(|previous| tail_pose != previous) {
                distinct_tail_poses += 1;
            }
            previous_tail = Some(tail_pose);
            let body_name = if identifier == "minecraft:salmon" {
                "body_front"
            } else {
                "body"
            };
            let body = snapshot
                .bone_names
                .iter()
                .position(|name| name.as_ref() == body_name)
                .expect("pinned fish body");
            // These body bones have no authored X/Z rotation in water. Yaw sway
            // therefore leaves quaternion X/Z zero; a land-flop roll cannot pass.
            let rotation = snapshot.current[body].rotation;
            assert!(
                rotation[0].abs() < 1e-5 && rotation[2].abs() < 1e-5,
                "{identifier} wet body must stay upright: {rotation:?}"
            );
        }
        assert!(
            distinct_tail_poses >= 4,
            "{identifier} authored tail sway must advance on a stationary wet actor"
        );
    }
}
