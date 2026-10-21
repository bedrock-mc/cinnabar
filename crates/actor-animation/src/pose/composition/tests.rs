use super::*;

/// Advances a deterministic fixture stream without external random state.
fn next(seed: &mut u64) -> usize {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (*seed >> 32) as usize
}

/// Produces exactly representable signed values, including negative zero.
fn value(seed: &mut u64) -> f32 {
    let bits = next(seed);
    let magnitude = (bits % 257) as f32 / 16.0;
    if bits & 512 == 0 {
        magnitude
    } else {
        -magnitude
    }
}

/// Makes forests, chains and branches, then permutes storage independently of parent order.
fn tree(seed: &mut u64, count: usize) -> Vec<RuntimeBone> {
    let mut bones: Vec<_> = (0..count)
        .map(|index| RuntimeBone {
            parent: (index > 0 && !next(seed).is_multiple_of(5)).then(|| next(seed) % index),
            pivot: std::array::from_fn(|_| value(seed)),
            rotation: std::array::from_fn(|_| value(seed)),
            ..Default::default()
        })
        .collect();
    let mut order: Vec<_> = (0..count).collect();
    for index in (1..count).rev() {
        order.swap(index, next(seed) % (index + 1));
    }
    let mut inverse = vec![0; count];
    for (index, &old) in order.iter().enumerate() {
        inverse[old] = index;
    }
    for bone in &mut bones {
        bone.parent = bone.parent.map(|parent| inverse[parent]);
    }
    order
        .into_iter()
        .map(|index| bones[index].clone())
        .collect()
}

/// Compares every float's bits, including signed zero, rather than approximate equality.
fn assert_bits(expected: &[BoneTransform], actual: &[BoneTransform]) {
    assert_eq!(expected.len(), actual.len());
    for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
        let bits = |bone: &BoneTransform| {
            bone.rotation
                .iter()
                .chain(&bone.translation_scale)
                .chain(&bone.axis_scale)
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        };
        assert_eq!(bits(expected), bits(actual), "bone {index}");
    }
}

/// Checks success/failure and exact output against both retained recursive entry points.
fn differential(bones: &[RuntimeBone], local: &[LocalDelta], targets: &[Option<BoneTransform>]) {
    let program = Composition::new(bones).unwrap();
    let mut output = vec![BoneTransform::default(); bones.len()];
    let delta = |index| local.get(index).copied().unwrap_or_default();
    let old = compose_pose(bones, local);
    let new = program.compose_into(bones, delta, &mut output);
    assert_eq!(old.is_some(), new.is_some());
    if let Some(expected) = old {
        assert_bits(&expected, &output);
    }
    let old = compose_pose_with_targets(bones, local, targets);
    let new = program.compose_with_targets_into(bones, delta, targets, &mut output);
    assert_eq!(old.is_some(), new.is_some());
    if let Some(expected) = old {
        assert_bits(&expected, &output);
    }
}

#[test]
fn arbitrary_admitted_trees_match_recursive_bits() {
    let mut seed = 0xa152_8fc3_748a_e02f;
    for count in [0, 1, 2, 3, 17, 64, 129] {
        for case in 0..32_usize {
            let mut bones = tree(&mut seed, count);
            let program = Composition::new(&bones).unwrap();
            // Root modes can change after the immutable program is shared.
            for bone in &mut bones {
                bone.attachable_root = match next(&mut seed) % 3 {
                    0 => AttachableRootFrame::Actor,
                    1 => AttachableRootFrame::MatchingOwnerName,
                    _ => AttachableRootFrame::BindingExpression,
                };
            }
            let mut local: Vec<_> = (0..count)
                .map(|_| LocalDelta {
                    translation: std::array::from_fn(|_| value(&mut seed)),
                    rotation: std::array::from_fn(|_| value(&mut seed)),
                    scale: std::array::from_fn(|_| value(&mut seed) / 16.0),
                    rotation_relative_to_entity: next(&mut seed) & 1 != 0,
                    ..Default::default()
                })
                .collect();
            if case.is_multiple_of(2) {
                let names: Vec<Box<str>> = (0..count)
                    .map(|index| ["head", "body", "rightarm", "leftarm", "child"][index % 5].into())
                    .collect();
                crate::legacy_swing::apply(&names, &mut local, case as f32 / 32.0, 32.0, 0.75)
                    .unwrap();
            }
            // Also exercise missing local deltas and ignored entries past the output span.
            if case.is_multiple_of(5) {
                local.truncate(count / 2);
            }
            if case.is_multiple_of(7) {
                local.push(LocalDelta::default());
            }
            let targets: Vec<_> = (0..count)
                .map(|_| {
                    (next(&mut seed).is_multiple_of(4)).then(|| {
                        with_scale(
                            quat_from_euler([17.0, -8.0, 23.0]),
                            std::array::from_fn(|_| value(&mut seed)),
                            [-0.5, 1.25, 2.0],
                        )
                    })
                })
                .collect();
            differential(&bones, &local, &targets);
            let mut output = vec![BoneTransform::default(); count];
            program
                .compose_into(
                    &bones,
                    |index| local.get(index).copied().unwrap_or_default(),
                    &mut output,
                )
                .unwrap();
            assert_bits(&compose_pose(&bones, &local).unwrap(), &output);
        }
    }
}

#[test]
fn invalid_dynamic_values_and_overridden_values_match_recursive_validation() {
    let bones = [
        RuntimeBone::default(),
        RuntimeBone {
            parent: Some(0),
            pivot: [1.0; 3],
            ..Default::default()
        },
    ];
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
        for index in 0..bones.len() {
            for field in 0..9 {
                let mut local = [LocalDelta::default(); 2];
                let values = match field / 3 {
                    0 => &mut local[index].translation,
                    1 => &mut local[index].rotation,
                    _ => &mut local[index].scale,
                };
                values[field % 3] = invalid;
                differential(&bones, &local, &[]);
                // A target overrides even a nonfinite local delta, as before.
                differential(
                    &bones,
                    &local,
                    &[Some(with_scale([0.0, 0.0, 0.0, 1.0], [0.0; 3], [1.0; 3])); 2],
                );
            }
            for field in 0..11 {
                let mut target = with_scale([0.0, 0.0, 0.0, 1.0], [0.0; 3], [1.0; 3]);
                match field {
                    0..4 => target.rotation[field] = invalid,
                    4..8 => target.translation_scale[field - 4] = invalid,
                    _ => target.axis_scale[field - 8] = invalid,
                }
                let mut targets = [None; 2];
                targets[index] = Some(target);
                differential(&bones, &[], &targets);
            }
        }
    }
    let mut local = [LocalDelta::default(); 2];
    local[0].scale = [f32::MAX; 3];
    local[1].scale = [2.0; 3];
    differential(&bones, &local, &[]);
    let names = ["body".into(), "rightarm".into()];
    crate::legacy_swing::apply(&names, &mut local, 0.5, 10.0, f32::INFINITY).unwrap();
    differential(&bones, &local, &[]);
}

#[test]
fn malformed_programs_and_incorrect_output_spans_are_rejected() {
    for parent in [0, 1, usize::MAX] {
        assert!(
            Composition::new(&[RuntimeBone {
                parent: Some(parent),
                ..Default::default()
            }])
            .is_none()
        );
    }
    assert!(
        Composition::new(&[
            RuntimeBone {
                parent: Some(1),
                ..Default::default()
            },
            RuntimeBone {
                parent: Some(0),
                ..Default::default()
            }
        ])
        .is_none()
    );
    let bones = [RuntimeBone::default()];
    let program = Composition::new(&bones).unwrap();
    assert!(
        program
            .compose_into(&bones, |_| LocalDelta::default(), &mut [])
            .is_none()
    );
    assert!(
        program
            .compose_into(
                &[],
                |_| LocalDelta::default(),
                &mut [BoneTransform::default()]
            )
            .is_none()
    );
}

#[test]
fn hot_composition_allocates_nothing_including_legacy_and_driver_mapping() {
    let mut seed = 42;
    for count in [17, 64] {
        let bones = tree(&mut seed, count);
        let program = Composition::new(&bones).unwrap();
        let mut local = vec![
            LocalDelta {
                rotation: [12.0, -30.0, 17.0],
                scale: [-0.5, 0.75, 1.25],
                ..Default::default()
            };
            count
        ];
        let names: Vec<Box<str>> = (0..count)
            .map(|index| {
                if index.is_multiple_of(2) {
                    "rightarm".into()
                } else {
                    "child".into()
                }
            })
            .collect();
        let drivers: Vec<_> = (0..count).rev().collect();
        let mut output = vec![BoneTransform::default(); count];
        let (_, old_calls) = test_alloc::count(|| compose_pose(&bones, &local));
        assert!(old_calls > 0);
        for legacy in [false, true] {
            if legacy {
                crate::legacy_swing::apply(&names, &mut local, 0.4, 13.0, 1.0).unwrap();
            }
            let (result, allocated) = test_alloc::measure(|| {
                program.compose_into(&bones, |index| local[drivers[index]], &mut output)
            });
            assert!(result.is_some());
            assert_eq!(allocated, test_alloc::Allocations::default());
            let mapped: Vec<_> = drivers.iter().map(|&index| local[index]).collect();
            assert_bits(&compose_pose(&bones, &mapped).unwrap(), &output);
        }
    }
}
