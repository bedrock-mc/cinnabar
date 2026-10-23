use super::*;

#[test]
fn remote_player_turns_leave_bedrock_and_java_walk_cycles_idle() {
    let actor = TestActor {
        kind: ActorKind::Player {
            uuid: [1; 16],
            username: "Player".into(),
        },
        ..Default::default()
    };
    let mut actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut store = ActorAnimationStore::with_assets(
        crate::test_support::counting_random_assets_for("minecraft:player"),
    );
    store.insert(1, 0, &actors[&1]);
    for (yaw, head_yaw) in [
        (0.0, 30.0),
        (0.0, 120.0),
        (90.0, 120.0),
        (179.0, 179.0),
        (-179.0, -179.0),
        (-90.0, -90.0),
    ] {
        let actor = actors.get_mut(&1).unwrap();
        actor.yaw = yaw;
        actor.head_yaw = head_yaw;
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        let state = store.rigs.values().next().unwrap();
        let input = state.history.back().unwrap();
        assert_eq!(input.position_delta, [0.0; 3]);
        assert_eq!(input.move_speed, 0.0, "Bedrock speed at yaw {yaw}");
        assert_eq!(input.distance_moved, 0.0, "Bedrock distance at yaw {yaw}");
        assert_eq!(input.walk_distance, 0.0);
        let rig = store.get(1).unwrap();
        assert_eq!(rig.java.limb_amount, [0.0; 2], "Java amount at yaw {yaw}");
        assert_eq!(rig.java.limb_swing, [0.0; 2], "Java phase at yaw {yaw}");
    }
    let rig = store.get(1).unwrap();
    assert_ne!(rig.body_yaw, 0.0);
    assert_ne!(rig.java.body_yaw[1], 0.0);

    actors.get_mut(&1).unwrap().position[0] = 0.2;
    store.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    let state = store.rigs.values().next().unwrap();
    let input = state.history.back().unwrap();
    assert!(input.move_speed > 0.0);
    assert!(input.distance_moved > 0.0);
    assert!(input.walk_distance > 0.0);
    let rig = store.get(1).unwrap();
    assert!(rig.java.limb_amount[1] > 0.0);
    assert!(rig.java.limb_swing[1] > 0.0);
}

/// Boats are not mobs: a standing boat faces its new rotation at once, with no body-turn lag.
#[test]
fn boats_face_their_rotation_without_mob_body_lag() {
    let actor = TestActor {
        kind: ActorKind::Entity {
            identifier: assets::facts::VanillaEntity::Boat.identifier().into(),
        },
        ..Default::default()
    };
    let mut actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut store =
        ActorAnimationStore::with_assets(crate::test_support::counting_random_assets_for(
            assets::facts::VanillaEntity::Boat.identifier(),
        ));
    store.insert(1, 0, &actors[&1]);
    for yaw in [150.0, -170.0, 20.0] {
        actors.get_mut(&1).unwrap().yaw = yaw;
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        assert_eq!(store.get(1).unwrap().body_yaw, yaw);
    }
}
