use super::*;

fn root(rotation: [f32; 4], translation: [f32; 3]) -> BoneTransform {
    with_scale(rotation, translation, [1.0; 3])
}

/// A child of a zero-scaled parent still retargets to finite transforms.
#[test]
fn retarget_survives_a_zero_scaled_parent() {
    let bones = vec![
        RuntimeBone::default(),
        RuntimeBone {
            parent: Some(0),
            ..Default::default()
        },
    ];
    let hidden = with_scale([0.0, 0.0, 0.0, 1.0], [0.0, 24.0, 0.0], [0.0; 3]);
    let pose = vec![hidden, root([0.0, 0.0, 0.0, 1.0], [0.0, 24.0, 3.0])];
    let target = root([0.0, 0.0, 0.0, 1.0], [1.0, 20.0, 0.0]);
    let posed = retarget(&bones, &pose, &pose, 0.0, &[Some(target), None]).unwrap();
    assert_eq!(translation(posed[1]), [1.0, 20.0, 3.0]);
}

/// Untargeted children keep their animated offset from the parent under its new transform.
#[test]
fn retarget_carries_children_with_their_animated_offsets() {
    let bones = vec![
        RuntimeBone::default(),
        RuntimeBone {
            parent: Some(0),
            ..Default::default()
        },
    ];
    let half_turn = [0.0, 0.0, 1.0, 0.0];
    let pose = vec![
        root([0.0, 0.0, 0.0, 1.0], [0.0, 24.0, 0.0]),
        root([0.0, 0.0, 0.0, 1.0], [0.0, 24.0, 3.0]),
    ];
    let target = root(half_turn, [1.0, 20.0, 0.0]);
    let posed = retarget(&bones, &pose, &pose, 0.5, &[Some(target), None]).unwrap();
    assert_eq!(posed[0], target);
    assert_eq!(translation(posed[1]), [1.0, 20.0, 3.0]);
    assert_eq!(posed[1].rotation, half_turn);
}

/// Retargeting as it ran before tick transforms were retained: every frame derives each bone's
/// parent-relative transforms from both poses again.
fn reference(
    bones: &[RuntimeBone],
    previous: &[BoneTransform],
    current: &[BoneTransform],
    alpha: f32,
    targets: &[Option<BoneTransform>],
) -> Option<Vec<BoneTransform>> {
    if previous.len() != bones.len() || current.len() != bones.len() {
        return None;
    }
    let mut posed: Vec<Option<BoneTransform>> = vec![None; bones.len()];
    for index in 0..bones.len() {
        reference_bone(
            index, bones, previous, current, alpha, targets, &mut posed, 0,
        )?;
    }
    posed.into_iter().collect()
}

#[allow(clippy::too_many_arguments)]
fn reference_bone(
    index: usize,
    bones: &[RuntimeBone],
    previous: &[BoneTransform],
    current: &[BoneTransform],
    alpha: f32,
    targets: &[Option<BoneTransform>],
    posed: &mut [Option<BoneTransform>],
    depth: usize,
) -> Option<BoneTransform> {
    if let Some(done) = posed[index] {
        return Some(done);
    }
    if depth > bones.len() {
        return None;
    }
    let bone = match (targets.get(index).copied().flatten(), bones[index].parent) {
        (Some(target), _) => target,
        (None, None) => blend(previous[index], current[index], alpha),
        (None, Some(parent)) => {
            let local = blend(
                relative(previous[parent], previous[index]),
                relative(current[parent], current[index]),
                alpha,
            );
            let parent = reference_bone(
                parent,
                bones,
                previous,
                current,
                alpha,
                targets,
                posed,
                depth + 1,
            )?;
            compose(parent, local)
        }
    };
    posed[index] = Some(bone);
    Some(bone)
}

/// Deterministic xorshift stream for generated skeletons and poses.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    /// Mostly ordinary values, with signed zeros, non-finite values and extremes mixed in.
    fn value(&mut self) -> f32 {
        match self.below(40) {
            0 => -0.0,
            1 => 0.0,
            2 => f32::NAN,
            3 => f32::INFINITY,
            4 => f32::NEG_INFINITY,
            5 => 1e-30,
            6 => -3.0e30,
            _ => (self.below(20_001) as f32 - 10_000.0) / 2_500.0,
        }
    }

    fn bone(&mut self) -> BoneTransform {
        let mut bone = BoneTransform {
            rotation: std::array::from_fn(|_| self.value()),
            translation_scale: std::array::from_fn(|_| self.value()),
            axis_scale: std::array::from_fn(|_| self.value()),
        };
        match self.below(4) {
            0 => bone.rotation = [0.0, 0.0, 0.0, 1.0],
            1 => {
                bone.translation_scale[3] = 1.0;
                bone.axis_scale = [1.0; 3];
            }
            _ => {}
        }
        bone
    }
}

fn same(left: &Option<Vec<BoneTransform>>, right: Option<&[BoneTransform]>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => same_bits(left, right),
        _ => false,
    }
}

/// Retained tick transforms give the frame-by-frame result bit for bit across ticks whose poses
/// keep, move or replace bones, with any targets, at every frame fraction including -0 and NaN.
#[test]
fn retained_ticks_match_per_frame_retargeting_bit_for_bit() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let alphas = [0.0, -0.0, 0.25, 0.5, 0.999, 1.0, f32::NAN, 1e-7, 2.0, -0.5];
    let mut cache = JavaRetargetCache::default();
    let mut compared = 0;
    for rig in 0..300u64 {
        let count = 1 + random.below(10) as usize;
        let bones: Vec<RuntimeBone> = (0..count)
            .map(|_| RuntimeBone {
                parent: (random.below(10) >= 3).then(|| random.below(count as u64) as usize),
                ..Default::default()
            })
            .collect();
        let mut current: Vec<BoneTransform> = (0..count).map(|_| random.bone()).collect();
        for _tick in 0..4 {
            let previous = current.clone();
            match random.below(4) {
                // An unchanged tick, the common case for a still rig.
                0 => {}
                // Each bone either keeps its exact transform or moves.
                1 | 2 => {
                    for bone in &mut current {
                        if random.below(2) == 0 {
                            *bone = random.bone();
                        }
                    }
                }
                _ => current = (0..count).map(|_| random.bone()).collect(),
            }
            for _frame in 0..alphas.len() {
                cache.begin_frame();
                let alpha = alphas[random.below(alphas.len() as u64) as usize];
                let targets: Vec<Option<BoneTransform>> = (0..count)
                    .map(|_| (random.below(4) == 0).then(|| random.bone()))
                    .collect();
                let expected = reference(&bones, &previous, &current, alpha, &targets);
                let retained =
                    cache.retarget((rig, 0), &bones, [&previous, &current], alpha, &targets);
                assert!(same(&expected, retained), "rig {rig} at {alpha}");
                let single = retarget(&bones, &previous, &current, alpha, &targets);
                assert!(same(&expected, single.as_deref()), "rig {rig} at {alpha}");
                compared += 1;
            }
        }
    }
    assert!(compared > 10_000);
}

/// A pose whose length differs from the skeleton is refused, as before.
#[test]
fn mismatched_poses_are_refused() {
    let bones = vec![RuntimeBone::default(); 2];
    let pose = vec![BoneTransform::default(); 1];
    let mut cache = JavaRetargetCache::default();
    assert!(
        cache
            .retarget((1, 0), &bones, [&pose, &pose], 0.5, &[])
            .is_none()
    );
    assert!(retarget(&bones, &pose, &pose, 0.5, &[]).is_none());
}

/// Frames of one tick reuse its parent-relative transforms; a new pose or skeleton rebuilds them.
#[test]
fn frames_of_a_tick_reuse_its_relative_transforms() {
    let bones = vec![
        RuntimeBone::default(),
        RuntimeBone {
            parent: Some(0),
            ..Default::default()
        },
    ];
    let previous = vec![
        root([0.0, 0.0, 0.0, 1.0], [0.0, 24.0, 0.0]),
        root([0.0, 0.0, 0.0, 1.0], [0.0, 24.0, 3.0]),
    ];
    let mut current = previous.clone();
    current[1].translation_scale[2] = 4.0;
    let mut cache = JavaRetargetCache::default();
    let rebuilds = |cache: &JavaRetargetCache| cache.rigs[&(7, 0)].0.rebuilds;
    for alpha in [0.1, 0.4, 0.9] {
        cache.begin_frame();
        cache.retarget((7, 0), &bones, [&previous, &current], alpha, &[None, None]);
    }
    assert_eq!(rebuilds(&cache), 1);
    let next = vec![current[1], current[0]];
    cache.retarget((7, 0), &bones, [&current, &next], 0.5, &[None, None]);
    assert_eq!(rebuilds(&cache), 2);
    let reparented = vec![bones[1].clone(), bones[0].clone()];
    cache.retarget((7, 0), &reparented, [&current, &next], 0.5, &[None, None]);
    assert_eq!(rebuilds(&cache), 3);
}

/// Rigs no recent frame retargeted release their retained transforms.
#[test]
fn unused_rigs_are_released() {
    let bones = vec![RuntimeBone::default()];
    let pose = vec![root([0.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0])];
    let mut cache = JavaRetargetCache::default();
    cache.begin_frame();
    cache.retarget((1, 0), &bones, [&pose, &pose], 0.5, &[None]);
    for _ in 0..RETENTION_FRAMES {
        cache.begin_frame();
        assert!(cache.rigs.contains_key(&(1, 0)));
    }
    cache.begin_frame();
    assert!(cache.rigs.is_empty());
}
