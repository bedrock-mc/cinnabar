//! Presentation snaps preserve tick-owned animation while discarding apparent displacement.
use std::sync::Arc;

use super::super::ActorStore;
use actor_animation::test_support;
use assets::MolangOp;
use protocol::{ActorEvent, ActorKind, ActorMetadataValue, ActorSpawnEvent};

/// Spawns a stationary local player whose render color exposes native walking speed and tick count.
fn stationary_player() -> ActorStore {
    let mut compiled = test_support::counting_random_compiled("minecraft:player");
    // Symbols must stay sorted, so the renamed query moves after the fixture's other queries.
    compiled.molang_symbols[1..5].rotate_left(1);
    compiled.molang_symbols[4].identifier = "query.modified_move_speed".into();
    for op in &mut compiled.molang_ops {
        if let MolangOp::LoadQuery(index) = op {
            *index = match *index {
                1 => 4,
                index @ 2..=4 => index - 1,
                index => index,
            };
        }
    }
    let assets = Arc::new(
        assets::RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut store = ActorStore::new_with_entity_assets(1, 0, assets);
    store.apply(
        1,
        1,
        ActorEvent::Spawn(ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Player {
                uuid: [1; 16],
                username: "fixture".into(),
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
    store.exclude_remote_state_for(1);
    store.advance_interpolation_ticks(1);
    store
}

#[test]
fn sleep_and_wake_snaps_do_not_walk_a_stationary_player_or_restart_animation() {
    for ticks_between_transitions in [0, 1, 3] {
        let mut store = stationary_player();
        let rig = store.actor_rig(1).unwrap();
        let reset = (rig.reset_generation, rig.rest_reset_generation);
        let mut tick = rig.completed_tick;
        let bed = [2.0, 0.5, -5.0];
        for (sleeping, position, transition) in [
            (true, bed, true),
            (true, bed, false),
            (false, [0.0; 3], true),
        ] {
            store
                .actors
                .get_mut(&1)
                .unwrap()
                .metadata
                .insert(26, ActorMetadataValue::Byte(if sleeping { 2 } else { 0 }));
            store.set_local_presentation_position(1, position, transition);
            let rig = store.actor_rig(1).unwrap();
            assert_eq!(
                rig.completed_tick, tick,
                "placement cannot advance animation"
            );
            assert_eq!((rig.reset_generation, rig.rest_reset_generation), reset);
            store.advance_interpolation_ticks(ticks_between_transitions);
            tick += u64::from(ticks_between_transitions);
            let rig = store.actor_rig(1).unwrap();
            assert_eq!(rig.render[0].color[2], 0.0, "native walking stays idle");
            assert_eq!(rig.java.limb_amount, [0.0; 2]);
            assert_eq!(rig.java.limb_swing, [0.0; 2]);
            assert_eq!(rig.java.cape, [[0.0; 3]; 2]);
            assert_eq!(
                rig.render[0].color[0], tick as f32,
                "controller state continues"
            );
            assert_eq!((rig.reset_generation, rig.rest_reset_generation), reset);
        }
        store.advance_interpolation_ticks(1);
        let rig = store.actor_rig(1).unwrap();
        assert_eq!(rig.render[0].color[2], 0.0);
        assert_eq!(rig.java.limb_amount, [0.0; 2]);
        assert_eq!(rig.java.cape, [[0.0; 3]; 2]);
    }
}

#[test]
fn stable_placement_keeps_observed_motion_and_animation_history() {
    let mut store = stationary_player();
    let mut reference = stationary_player();
    for x in [0.25, 0.5] {
        for store in [&mut store, &mut reference] {
            store.set_local_presentation_position(1, [x, 0.0, 0.0], false);
            store.advance_interpolation_ticks(1);
        }
        let rig = store.actor_rig(1).unwrap();
        assert!(
            rig.render[0].color[2] > 0.0,
            "native motion retains displacement"
        );
        assert!(rig.java.limb_amount[1] > 0.0);
        assert!(rig.java.limb_swing[1] > rig.java.limb_swing[0]);
    }
    store.set_local_presentation_position(1, [2.0, 70.0, -5.0], true);
    store.advance_interpolation_ticks(1);
    reference.advance_interpolation_ticks(1);
    let rig = store.actor_rig(1).unwrap();
    let ordinary = reference.actor_rig(1).unwrap();
    assert_eq!(rig.render[0].color, ordinary.render[0].color);
    assert_eq!(rig.java.limb_amount, ordinary.java.limb_amount);
    assert_eq!(rig.java.limb_swing, ordinary.java.limb_swing);
    assert_eq!(
        rig.java.cape, ordinary.java.cape,
        "existing cape lag survives the snap"
    );
}
