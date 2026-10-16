use super::*;
use crate::tick::WeightedClip;
use assets::{EntityAnimationProperty, RuntimeEntityAssets};
use protocol_ids::{ActorDataKey, ActorFlag};

mod frame_tests;

/// A player double with no item use or special posture.
fn player() -> TestActor {
    TestActor {
        kind: ActorKind::Player {
            uuid: [0; 16],
            username: "test".into(),
        },
        ..Default::default()
    }
}

/// A body-parented skeleton with an arm child and an item child.
fn skeleton() -> (Vec<RuntimeBone>, Vec<Box<str>>) {
    let names = [
        "body",
        "head",
        "rightarm",
        "leftarm",
        "rightitem",
        "rightsleeve",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    let part = |parent, pivot| RuntimeBone {
        parent,
        pivot,
        ..Default::default()
    };
    (
        vec![
            part(None, [0.0, 24.0, 0.0]),
            part(Some(0), [0.0, 24.0, 0.0]),
            part(Some(0), [5.0, 22.0, 0.0]),
            part(Some(0), [-5.0, 22.0, 0.0]),
            part(Some(2), [6.0, 15.0, 1.0]),
            part(Some(2), [5.0, 22.0, 0.0]),
        ],
        names,
    )
}

/// Samples a synthetic modern attack clip through the production profile selector.
fn sample(
    context: ActorTickContext,
    actor: &TestActor,
    progress: f32,
    blend: Option<f32>,
) -> Vec<BoneTransform> {
    let mut compiled = crate::test_support::compiled_fixture();
    let symbol = compiled.animation_clips[0].symbol as usize;
    compiled.symbols[symbol].identifier = "animation.player.attack.rotations".into();
    compiled.animation_channels[0].bone_name = Some("rightarm".into());
    compiled.animation_channels[0].property = EntityAnimationProperty::Rotation;
    compiled.animation_keyframes[0].expressions = [None; 3];
    compiled.animation_keyframes[0].value = [40.0, 0.0, 0.0].map(crate::test_support::scalar);
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let layout = VariableLayout::new(&assets);
    let (bones, names) = skeleton();
    let input = ActorTickInput {
        attack_time: progress,
        pitch: actor.pitch,
        ..Default::default()
    };
    let evaluator = Evaluator {
        assets: &assets,
        pack: false,
        layout: &layout,
        program: None,
        actor,
        input: &input,
        context: &context,
        anim_tick: 0,
        anim_time: None,
        swell_amount: None,
        presentation_alpha: None,
        query_history: None,
        life_tick: 0,
        finished: (false, false),
        state_time: 0.0,
        bones: &bones,
        bone_names: &names,
    };
    let clip = WeightedClip {
        clip: 0,
        weight: 1.0,
        started_tick: 0,
        clock: crate::clock::Basis::Lifetime,
        time: 0.0,
        blend: None,
    };
    let clips = blend.map_or_else(
        || vec![clip],
        |amount| {
            [false, true]
                .into_iter()
                .map(|incoming| WeightedClip {
                    blend: Some(crate::tick::ControllerBlend {
                        controller: 0,
                        incoming,
                        amount,
                    }),
                    ..clip
                })
                .collect()
        },
    );
    let mut world_left = MAX_MOLANG_OPS_PER_WORLD_TICK;
    let mut budget = EvalBudget {
        actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
        world_left: &mut world_left,
        work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
        stack: Default::default(),
        static_draw: None,
    };
    let local = pose::sample_clips(
        &evaluator,
        &mut layout.fresh(1),
        &bones,
        &names,
        &clips,
        &mut budget,
    )
    .unwrap();
    pose::compose_pose(&bones, &local).unwrap()
}

#[test]
fn legacy_replaces_json_for_local_and_remote_players_and_preserves_descendants() {
    for local in [false, true] {
        for (s, b, pitch, x, z) in [
            (0.0, 0.0, 0.0, 5.0, 0.0),
            (0.25, 0.0, -1.3771007, 5.0, 0.0),
            (0.5, -0.19277565, -0.7592212, 4.907356, -0.95757717),
            (0.75, -0.1491777, -0.38595682, 4.9444666, -0.742659),
            (1.0, 0.0, 0.0, 5.0, 0.0),
        ] {
            let pose = sample(
                ActorTickContext {
                    combat_visuals: CombatVisuals::Legacy112,
                    is_local_player: local,
                    ..Default::default()
                },
                &player(),
                s,
                None,
            );
            let expected = pose::quat_from_euler([
                (-pitch as f32).to_degrees(),
                (-3.0 * b as f32).to_degrees(),
                0.0,
            ]);
            for (actual, expected) in pose[2].rotation.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 2e-6);
            }
            let body = pose::quat_from_euler([0.0, (-b as f32).to_degrees(), 0.0]);
            let left =
                pose::quat_from_euler([(-b as f32).to_degrees(), (-b as f32).to_degrees(), 0.0]);
            for (actual, expected) in pose[0]
                .rotation
                .into_iter()
                .zip(body)
                .chain(pose[3].rotation.into_iter().zip(left))
            {
                assert!((actual - expected).abs() < 2e-6);
            }
            for (actual, expected) in pose[2].translation_scale[..3]
                .iter()
                .zip([x, 22.0, z])
                .chain(pose[3].translation_scale[..3].iter().zip([-x, 22.0, -z]))
            {
                assert!((*actual - expected).abs() < 2e-6);
            }
            assert_eq!(pose[1].rotation, [0.0, 0.0, 0.0, 1.0]);
            assert_eq!(pose[5], pose[2]);
            let offset = pose::rotate_vector(pose[2].rotation, [1.0, -7.0, 1.0]);
            for ((item, arm), offset) in pose[4].translation_scale[..3]
                .iter()
                .zip(&pose[2].translation_scale[..3])
                .zip(offset)
            {
                assert!((item - arm - offset).abs() < 2e-6);
            }
        }
    }
}

#[test]
fn modern_and_special_postures_keep_the_authored_attack() {
    let expected = pose::quat_from_euler([-40.0, 0.0, 0.0]);
    let actor = player();
    for context in [
        ActorTickContext {
            combat_visuals: CombatVisuals::Modern,
            ..Default::default()
        },
        ActorTickContext {
            main_hand: Some("minecraft:bow".into()),
            ..Default::default()
        },
        ActorTickContext {
            main_hand: Some("minecraft:crossbow".into()),
            ..Default::default()
        },
        ActorTickContext {
            main_hand_is_spear: true,
            ..Default::default()
        },
    ] {
        assert_eq!(sample(context, &actor, 0.5, None)[2].rotation, expected);
    }
    for flag in [
        ActorFlag::Swimming.wire_value(),
        ActorFlag::Blocking.wire_value(),
        ActorFlag::UsingItem.wire_value(),
        ActorFlag::Gliding.wire_value(),
        ActorFlag::Crawling.wire_value(),
        ActorFlag::Emoting.wire_value(),
    ] {
        let mut actor = player();
        if flag < 64 {
            actor
                .metadata
                .insert(0, ActorMetadataValue::Flags(1u64 << flag));
        } else {
            actor.metadata.insert(
                ActorDataKey::FlagsTwo.wire_value(),
                ActorMetadataValue::FlagsExtended(1u64 << (flag - 64)),
            );
        }
        assert_eq!(
            sample(ActorTickContext::default(), &actor, 0.5, None)[2].rotation,
            expected
        );
    }
}

#[test]
fn registered_maps_keep_the_authored_attack_in_either_hand() {
    for identifier in ["minecraft:empty_map", "minecraft:filled_map"] {
        // The full pinned item table, since the creative-only registry omits filled maps.
        assert!(assets::item::names::serialization_ids(identifier).is_some());
        for off_hand in [false, true] {
            let mut context = ActorTickContext {
                combat_visuals: CombatVisuals::Legacy112,
                ..Default::default()
            };
            if off_hand {
                context.off_hand = Some(identifier.into());
            } else {
                context.main_hand = Some(identifier.into());
            }
            assert_eq!(
                sample(context, &player(), 0.5, None)[2].rotation,
                pose::quat_from_euler([-40.0, 0.0, 0.0])
            );
        }
    }
}

#[test]
fn blending_two_attack_states_keeps_one_complete_swing() {
    for combat_visuals in [CombatVisuals::Legacy112, CombatVisuals::Modern] {
        let context = ActorTickContext {
            combat_visuals,
            ..Default::default()
        };
        let expected = sample(context.clone(), &player(), 0.5, None);
        for amount in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(
                sample(context.clone(), &player(), 0.5, Some(amount)),
                expected
            );
        }
    }
}

#[test]
fn head_pitch_reaches_the_strike_through_the_actor_input() {
    let mut actor = player();
    actor.pitch = 1.0_f32.to_degrees();
    let actual = sample(
        ActorTickContext {
            combat_visuals: CombatVisuals::Legacy112,
            ..Default::default()
        },
        &actor,
        0.5,
        None,
    );
    let expected = pose::quat_from_euler([
        0.009221196_f32.to_degrees(),
        0.57832694_f32.to_degrees(),
        0.0,
    ]);
    for (actual, expected) in actual[2].rotation.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 2e-6);
    }
}
