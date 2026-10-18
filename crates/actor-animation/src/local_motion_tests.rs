use super::*;
use protocol_ids::ActorFlag;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

/// A synthetic player carrier exercises publication without installed assets.
fn fixture() -> (HashMap<u64, TestActor>, ActorAnimationStore) {
    let mut compiled = crate::test_support::compiled_fixture();
    compiled.sources[1].path = "entity/player.json".into();
    compiled.symbols[4].kind = assets::EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:player".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = Some(3);
    compiled.animation_clips[0].symbol = 2;
    let mut actor = super::tests::actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Player {
        uuid: [1; 16],
        username: "Player".into(),
    };
    let mut store = ActorAnimationStore::with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    ));
    store.insert(1, 0, &actor);
    store.set_local_motion_authority(
        1,
        Some((
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        )),
    );
    (HashMap::from([(1, actor)]), store)
}

#[test]
fn predicted_use_starts_the_existing_hand_animation_on_the_first_tick() {
    let (mut actors, mut store) = fixture();
    actors.get_mut(&1).unwrap().metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::UsingItem.wire_value()),
    );
    let context = ActorTickContext {
        is_local: true,
        main_hand: Some("minecraft:bow".into()),
        local_item_use_ticks: Some(0),
        main_hand_max_use_ticks: 20,
        ..Default::default()
    };
    store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
    assert_eq!(
        store.get(1).unwrap().hand[1].use_ticks,
        1,
        "the hand phase must distinguish active first-tick use from idle"
    );
    let completed_tick = store.completed_tick;
    store.refresh_local_view(&actors, 1, |_| ActorTickContext {
        local_item_use_ticks: Some(7),
        ..context.clone()
    });
    assert_eq!(store.get(1).unwrap().hand[1].use_ticks, 8);
    assert_eq!(store.completed_tick, completed_tick);
}

#[test]
fn predicted_item_use_keeps_gameplay_ticks_across_actor_catchup_and_interruptions() {
    let (mut actors, mut store) = fixture();
    actors.get_mut(&1).unwrap().metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::UsingItem.wire_value()),
    );
    let context = ActorTickContext {
        is_local: true,
        local_item_use_ticks: Some(7),
        main_hand_max_use_ticks: 10,
        ..Default::default()
    };
    for _ in 0..3 {
        store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
    }
    assert_eq!(store.get(1).unwrap().hand[1].use_ticks, 8);
    let completed_tick = store.completed_tick;
    actors.get_mut(&1).unwrap().metadata.remove(&0);
    store.refresh_local_view(&actors, 1, |_| ActorTickContext {
        local_item_use_ticks: Some(0),
        ..context.clone()
    });
    assert_eq!(store.get(1).unwrap().hand[1].use_ticks, 0);
    assert_eq!(store.completed_tick, completed_tick);
}

/// Early actor ticks precede this frame's interaction admission.
fn early(actors: &HashMap<u64, TestActor>, store: &mut ActorAnimationStore, count: usize) {
    for _ in 0..count {
        store.advance_tick(actors, None, Some(1), true, true, |_| ActorTickContext {
            is_local: true,
            ..Default::default()
        });
    }
}

/// Publication refreshes the existing tick without advancing other actor state.
fn publish(
    actors: &HashMap<u64, TestActor>,
    store: &mut ActorAnimationStore,
    progress: LocalSwingProgress,
) {
    store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        ),
        [sample(101, 30.0, progress)],
    );
    store.sync_local_swing(1, progress);
    store.refresh_local_view(actors, 1, |_| ActorTickContext {
        is_local: true,
        ..Default::default()
    });
}

/// The Java torso reads this tick's post-update swing, for all effect durations.
#[test]
fn java_torso_reads_current_committed_swing_after_admission() {
    for duration in [ACTOR_SWING_TICKS, 4, 8] {
        let (mut actors, mut store) = fixture();
        early(&actors, &mut store, 1);
        actors.get_mut(&1).unwrap().yaw = 30.0;
        early(&actors, &mut store, 1);
        let completed = store.completed_tick;
        let rig = store.get(1).unwrap();
        let unchanged = (rig.java.limb_swing, rig.java.equip, rig.java.cape);
        publish(
            &actors,
            &mut store,
            LocalSwingProgress {
                bedrock: [0.0, 1.0 / duration as f32],
                java: [0.0, 1.0 / duration as f32],
                frame_alpha: Some(0.75),
            },
        );
        let rig = store.get(1).unwrap();
        assert_eq!(rig.java.body_yaw, [0.0, 9.0], "duration {duration}");
        assert_eq!(
            (rig.java.limb_swing, rig.java.equip, rig.java.cape),
            unchanged
        );
        assert_eq!(store.completed_tick, completed);
    }
}

/// Every completed catch-up tick supplies its own swing denominator, including completion.
#[test]
fn java_torso_catchup_keeps_each_committed_swing_sample() {
    for (duration, expected) in [(ACTOR_SWING_TICKS, 22.797), (4, 19.71), (8, 22.797)] {
        let (mut actors, mut store) = fixture();
        early(&actors, &mut store, 1);
        actors.get_mut(&1).unwrap().yaw = 30.0;
        early(&actors, &mut store, 5);
        let current = if 4 < duration {
            4.0 / duration as f32
        } else {
            0.0
        };
        store.sync_local_swing_motion(
            1,
            (
                SessionGeneration::new(1),
                PositionAuthorityGeneration::new(1),
            ),
            (0..5).map(|counter| {
                let value = if counter < duration {
                    counter as f32 / duration as f32
                } else {
                    0.0
                };
                sample(
                    97 + counter as u64,
                    30.0,
                    LocalSwingProgress {
                        java: [0.0, value],
                        frame_alpha: Some(0.5),
                        ..Default::default()
                    },
                )
            }),
        );
        publish(
            &actors,
            &mut store,
            LocalSwingProgress {
                bedrock: [3.0 / duration as f32, current],
                java: [3.0 / duration as f32, current],
                frame_alpha: Some(0.5),
            },
        );
        let rig = store.get(1).unwrap();
        assert!(
            (rig.java.body_yaw[1] - expected).abs() < 1e-5,
            "duration {duration}: {:?}",
            rig.java.body_yaw
        );
    }
}

/// The body sampler receives resolved movement with each tick, independently of wire velocity.
fn sample(tick: u64, yaw: f32, progress: LocalSwingProgress) -> LocalSwingMotionSample {
    LocalSwingMotionSample {
        tick,
        delta: [0.0; 3],
        yaw,
        progress,
    }
}

/// Actor and local clocks may finish different numbers of ticks in one frame.
#[test]
fn java_torso_uses_physics_ticks_and_preserves_other_motion() {
    let (actors, mut store) = fixture();
    early(&actors, &mut store, 1);
    let before = store.get(1).unwrap().java;
    let completed = store.completed_tick;
    let progress = LocalSwingProgress {
        java: [0.0, 0.25],
        frame_alpha: Some(0.75),
        ..Default::default()
    };
    assert!(store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [sample(101, 30.0, progress)]
    ));
    let changed = store.get(1).unwrap().java;
    assert_eq!(changed.body_yaw, [0.0, 9.0]);
    assert_eq!(changed.body_yaw_at(0.0), 6.75);
    assert_eq!(changed.body_yaw_at(1.0), 6.75);
    assert_eq!(changed.limb_swing, before.limb_swing);
    assert_eq!(changed.equip, before.equip);
    assert_eq!(changed.cape, before.cape);
    assert_eq!(changed.bob, before.bob);
    assert_eq!(changed.walked, before.walked);
    assert_eq!(store.completed_tick, completed);
    early(&actors, &mut store, 5);
    assert_eq!(store.get(1).unwrap().java.body_yaw, changed.body_yaw);
    assert_eq!(store.completed_tick, completed + 5);
}

/// Actor ticks turn the native body while retaining the latest local torso and attack samples.
#[test]
fn native_visual_turns_without_repeating_local_physics_observations() {
    let (mut actors, mut store) = fixture();
    early(&actors, &mut store, 1);
    let actor = actors.get_mut(&1).unwrap();
    actor.yaw = 60.0;
    actor.head_yaw = 60.0;
    let progress = LocalSwingProgress {
        bedrock: [0.25, 0.5],
        java: [0.25, 0.5],
        frame_alpha: Some(0.75),
    };
    publish(&actors, &mut store, progress);
    let before = store.get(1).unwrap();
    let native_yaw = before.body_yaw;
    let java_yaw = before.java.body_yaw;
    let position = actors[&1].position;

    early(&actors, &mut store, 3);

    let after = store.get(1).unwrap();
    assert_ne!(after.body_yaw, native_yaw);
    assert_eq!(after.java.body_yaw, java_yaw);
    assert_eq!(after.java.swing, progress.java);
    assert_eq!(after.hand.map(|phase| phase.attack_time), progress.bedrock);
    assert_eq!(actors[&1].position, position);
}

/// Backpressure can change only the latest published tick; duplicate and phase-only samples stay inert.
#[test]
fn java_torso_retries_latest_tick_without_double_turning() {
    let (actors, mut store) = fixture();
    early(&actors, &mut store, 1);
    let idle = sample(101, 30.0, LocalSwingProgress::default());
    assert!(!store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [idle]
    ));
    let active = sample(
        101,
        30.0,
        LocalSwingProgress {
            java: [0.0, 0.25],
            frame_alpha: Some(0.75),
            ..Default::default()
        },
    );
    assert!(store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [active]
    ));
    assert!(!store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [active]
    ));
    assert_eq!(store.get(1).unwrap().java.body_yaw, [0.0, 9.0]);
    let phase = LocalSwingMotionSample {
        progress: LocalSwingProgress {
            frame_alpha: Some(0.5),
            ..active.progress
        },
        ..active
    };
    assert!(!store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [phase]
    ));
    assert_eq!(store.get(1).unwrap().java.body_yaw_at(0.0), 4.5);
    assert!(!store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [LocalSwingMotionSample {
            tick: 100,
            ..active
        }]
    ));
    assert_eq!(store.get(1).unwrap().java.body_yaw, [0.0, 9.0]);
    assert!(store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [LocalSwingMotionSample {
            tick: 102,
            ..active
        }]
    ));
    assert!((store.get(1).unwrap().java.body_yaw[1] - 15.3).abs() < 1e-5);
}

/// Each completed movement sample owns its facing instead of the frame's final displacement.
#[test]
fn java_torso_catchup_reads_per_tick_displacement() {
    let (actors, mut store) = fixture();
    early(&actors, &mut store, 1);
    let first = LocalSwingMotionSample {
        delta: [0.1, 0.0, 0.0],
        ..sample(100, 0.0, LocalSwingProgress::default())
    };
    let second = LocalSwingMotionSample {
        tick: 101,
        delta: [0.0, 0.0, 0.1],
        ..first
    };
    assert!(store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [first, second]
    ));
    let heading = store.get(1).unwrap().java.body_yaw;
    assert!((heading[0] + 27.0).abs() < 1e-5);
    assert!((heading[1] + 18.9).abs() < 1e-5);
}

/// A new simulation identity and a replacement actor cannot inherit a stale consumed tick.
#[test]
fn java_torso_authority_and_respawn_reset_the_replay_clock() {
    let (actors, mut store) = fixture();
    early(&actors, &mut store, 1);
    let active = sample(
        101,
        30.0,
        LocalSwingProgress {
            java: [0.0, 0.25],
            ..Default::default()
        },
    );
    store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        ),
        [active],
    );
    store.set_local_motion_authority(
        1,
        Some((
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(2),
        )),
    );
    assert!(!store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [active]
    ));
    assert!(store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(2)
        ),
        [LocalSwingMotionSample { tick: 1, ..active }]
    ));
    assert!((store.get(1).unwrap().java.body_yaw[1] - 15.3).abs() < 1e-5);
    let mut respawn = actors[&1].clone();
    respawn.spawn_revision += 1;
    store.insert(1, 0, &respawn);
    assert!(store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(2)
        ),
        [active]
    ));
    assert_eq!(store.get(1).unwrap().java.body_yaw, [0.0, 9.0]);
    store.set_local_motion_authority(1, None);
    assert_eq!(store.get(1).unwrap().java.body_frame_alpha, None);
    assert!(!store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(2)
        ),
        [active]
    ));
}

/// Native motion keeps its actor cadence, and Bedrock-only acceleration cannot turn the Java torso.
#[test]
fn native_body_is_unchanged_by_java_physics_replay() {
    let (mut actors, mut store) = fixture();
    actors.get_mut(&1).unwrap().yaw = 30.0;
    early(&actors, &mut store, 1);
    let native = (
        store.get(1).unwrap().previous_body_yaw,
        store.get(1).unwrap().body_yaw,
    );
    let progress = LocalSwingProgress {
        bedrock: [0.0, 0.25],
        java: [0.0; 2],
        ..Default::default()
    };
    assert!(!store.sync_local_swing_motion(
        1,
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1)
        ),
        [sample(100, 30.0, progress)]
    ));
    let rig = store.get(1).unwrap();
    assert_eq!(rig.java.body_yaw, [0.0; 2]);
    assert_eq!((rig.previous_body_yaw, rig.body_yaw), native);
}
