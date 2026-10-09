use super::*;

fn fixture() -> Arc<RuntimeEntityAssets> {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    compiled.molang_expressions[0].op_count = 3;
    compiled.molang_expressions[0].max_stack = 1;
    compiled.molang_ops = compiled.molang_ops[..3].into();
    let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
    compiled.animation_clips[0].length_seconds = scalar(1.25);
    compiled.animation_clips[0].loop_mode = EntityAnimationLoop::HoldOnLastFrame;
    compiled.animation_channels[0].keyframe_count = 2;
    compiled.animation_keyframes[0].value = [scalar(0.0); 3];
    let mut end = compiled.animation_keyframes[0];
    end.time_seconds = scalar(1.25);
    end.value[0] = scalar(20.0);
    compiled.animation_keyframes = vec![compiled.animation_keyframes[0], end].into();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
}

#[test]
fn late_direct_attachable_activation_starts_at_its_first_delta_and_pauses_when_hidden() {
    let mut owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut runtime = AttachablesRuntime::new(fixture());
    let base = tests::owner_rig();
    let input = AttachableAnimationInput {
        first_person: true,
        ..AttachableAnimationInput::default()
    };
    let sample = |runtime: &mut AttachablesRuntime, owner: &ActorSnapshot, tick| {
        runtime
            .evaluate(
                "minecraft:test_item",
                owner,
                &ActorRigSnapshot {
                    completed_tick: tick,
                    ..base
                },
                input,
            )
            .unwrap()
            .pose[0]
            .translation_scale[0]
    };
    assert_eq!(sample(&mut runtime, &owner, 1), 0.0);
    owner.metadata.insert(0, ActorMetadataValue::Flags(1 << 40));
    assert!((sample(&mut runtime, &owner, 81) + 0.8).abs() < 1.0e-5);
    assert!((sample(&mut runtime, &owner, 82) + 1.6).abs() < 1.0e-5);
    owner.metadata.clear();
    assert_eq!(sample(&mut runtime, &owner, 200), 0.0);
    owner.metadata.insert(0, ActorMetadataValue::Flags(1 << 40));
    assert!((sample(&mut runtime, &owner, 300) + 2.4).abs() < 1.0e-5);
    for tick in 301..330 {
        sample(&mut runtime, &owner, tick);
    }
    assert_eq!(sample(&mut runtime, &owner, 330), -20.0);
    owner.metadata.clear();
    assert_eq!(sample(&mut runtime, &owner, 400), 0.0);
    owner.metadata.insert(0, ActorMetadataValue::Flags(1 << 40));
    assert_eq!(sample(&mut runtime, &owner, 500), -20.0);
}

#[test]
fn direct_attachable_clock_uses_render_delta_even_when_owner_sample_is_unchanged() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::from([(
        0,
        ActorMetadataValue::Flags(1 << 40),
    )]));
    let mut runtime = AttachablesRuntime::new(fixture());
    let base = tests::owner_rig();
    let sample = |runtime: &mut AttachablesRuntime, tick, alpha| {
        runtime
            .evaluate(
                "minecraft:test_item",
                &owner,
                &ActorRigSnapshot {
                    completed_tick: tick,
                    ..base
                },
                AttachableAnimationInput {
                    first_person: true,
                    frame_alpha: alpha,
                    delta_seconds: Some(0.01),
                    ..Default::default()
                },
            )
            .unwrap()
            .pose[0]
            .translation_scale[0]
    };
    assert!((sample(&mut runtime, 81, 0.2) + 0.16).abs() < 1.0e-5);
    assert!((sample(&mut runtime, 81, 0.2) + 0.32).abs() < 1.0e-5);
    assert!((sample(&mut runtime, 81, 0.4) + 0.48).abs() < 1.0e-5);
    assert!((sample(&mut runtime, 82, 0.0) + 0.64).abs() < 1.0e-5);
}

#[test]
fn visible_held_crowd_retains_each_owners_authored_clock() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::from([(
        0,
        ActorMetadataValue::Flags(1 << 40),
    )]));
    let mut runtime = AttachablesRuntime::new(fixture());
    let mut rig = tests::owner_rig();
    let owners = 300;
    for expected in [-0.16, -0.32] {
        for runtime_id in 1..=owners {
            rig.actor.runtime_id = runtime_id;
            for off_hand in [false, true] {
                let snapshot = runtime
                    .evaluate(
                        "minecraft:test_item",
                        &owner,
                        &rig,
                        AttachableAnimationInput {
                            off_hand,
                            delta_seconds: Some(0.01),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                assert!(
                    (snapshot.pose[0].translation_scale[0] - expected).abs() < 1.0e-5,
                    "owner {runtime_id} hand {off_hand} restarted its animation clock"
                );
            }
        }
    }
    let sample = |runtime: &mut AttachablesRuntime, rig: &ActorRigSnapshot<'_>| {
        runtime
            .evaluate(
                "minecraft:test_item",
                &owner,
                rig,
                AttachableAnimationInput {
                    delta_seconds: Some(0.01),
                    ..Default::default()
                },
            )
            .unwrap()
            .pose[0]
            .translation_scale[0]
    };
    rig.actor.runtime_id = 2;
    for _ in 0..10 {
        sample(&mut runtime, &rig);
    }
    rig.actor.runtime_id = 1;
    assert!((sample(&mut runtime, &rig) + 0.48).abs() < 1.0e-5);
    rig.actor.spawn_revision += 1;
    assert!((sample(&mut runtime, &rig) + 0.16).abs() < 1.0e-5);
    assert!(runtime.states.keys().all(|(actor, _, _, _)| {
        actor.runtime_id != 1 || actor.spawn_revision == rig.actor.spawn_revision
    }));
    rig.actor.session_id += 1;
    assert!((sample(&mut runtime, &rig) + 0.16).abs() < 1.0e-5);
    assert_eq!(runtime.states.len(), 1);
}
