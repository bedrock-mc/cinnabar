use super::*;

fn fixture() -> Arc<RuntimeEntityAssets> {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
    compiled.animation_clips[0].length_seconds = scalar(1.25);
    compiled.animation_clips[0].loop_mode = EntityAnimationLoop::HoldOnLastFrame;
    compiled.animation_channels[0].keyframe_count = 2;
    compiled.animation_keyframes[0].value = [scalar(0.0); 3];
    let mut end = compiled.animation_keyframes[0].clone();
    end.time_seconds = scalar(1.25);
    end.value[0] = scalar(20.0);
    compiled.animation_keyframes = vec![compiled.animation_keyframes[0].clone(), end].into();
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
