use assets::{EntityAnimationKeyframe, EntityAnimationProperty};

use super::{
    tick::{ControllerBlend, WeightedClip},
    *,
};

// Vanilla bone loading uses a 24-pixel Y origin, then negates Y for the
// bone's default position.
pub const MODEL_PART_ORIGIN_Y: f32 = assets::gui_item::SHIELD_MODEL_PART_HEIGHT;

#[derive(Debug, Clone, Copy)]
pub(super) struct LocalDelta {
    pub(super) translation: [f32; 3],
    pub(super) rotation: [f32; 3],
    pub(super) scale: [f32; 3],
    pub(super) rotation_relative_to_entity: bool,
}

impl Default for LocalDelta {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
            rotation_relative_to_entity: false,
        }
    }
}

impl LocalDelta {
    fn property(&mut self, property: EntityAnimationProperty) -> &mut [f32; 3] {
        match property {
            EntityAnimationProperty::Translation => &mut self.translation,
            EntityAnimationProperty::Rotation => &mut self.rotation,
            EntityAnimationProperty::Scale => &mut self.scale,
        }
    }
}

/// Blends weighted clips into per-bone deltas, evaluating keyframe expressions against the
/// native default orientation plus the value earlier clips produced for the same channel.
pub(super) fn sample_clips(
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
    bones: &[RuntimeBone],
    bone_names: &[Box<str>],
    clips: &[WeightedClip],
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<LocalDelta>, EvalError> {
    let assets = evaluator.assets;
    let mut local = vec![LocalDelta::default(); bones.len()];
    // Shortest-path blends sample each state into its own fresh pose before composing.
    let mut sides = clips.iter().any(|clip| clip.blend.is_some()).then(|| {
        [
            vec![LocalDelta::default(); bones.len()],
            vec![LocalDelta::default(); bones.len()],
        ]
    });
    let mut pending: Option<ControllerBlend> = None;
    for weighted in clips {
        budget.charge_work()?;
        if let (Some(blend), Some(sides)) = (pending, sides.as_mut())
            && weighted.blend.is_none_or(|next| {
                next.controller != blend.controller || (blend.incoming && !next.incoming)
            })
        {
            compose_blend(&mut local, sides, blend.amount);
            pending = None;
        }
        let pose = match (weighted.blend, sides.as_mut()) {
            (Some(blend), Some(sides)) => {
                pending = Some(blend);
                &mut sides[usize::from(blend.incoming)]
            }
            _ => &mut local,
        };
        let weight = weighted.weight;
        if weight < f32::EPSILON {
            continue;
        }
        let clip = assets
            .animation_clips()
            .get(weighted.clip)
            .ok_or(EvalError::Invalid)?;
        let length = clip.length_seconds.get();
        // A clip's own clock starts when its controller state was entered.
        let clip_tick = (if weighted.clock == super::clock::Basis::Lifetime {
            evaluator.life_tick
        } else {
            evaluator.anim_tick
        })
        .saturating_sub(weighted.started_tick);
        let evaluator = &Evaluator {
            anim_tick: clip_tick,
            anim_time: Some(weighted.time),
            ..*evaluator
        };
        if clip.loop_mode == EntityAnimationLoop::Once && weighted.time > length {
            continue;
        }
        let time = weighted.time;
        let first = clip.first_channel as usize;
        let end = first
            .checked_add(clip.channel_count as usize)
            .ok_or(EvalError::Invalid)?;
        let channels = assets
            .animation_channels()
            .get(first..end)
            .ok_or(EvalError::Invalid)?;
        // An override clip first restores every bone it animates to its whole default pose.
        if clip.override_previous {
            for channel in channels {
                let Some(index) = channel_bone(channel, bone_names) else {
                    continue;
                };
                *pose.get_mut(index).ok_or(EvalError::Invalid)? = LocalDelta::default();
            }
        }
        for channel in channels {
            budget.charge_work()?;
            let Some(index) = channel_bone(channel, bone_names) else {
                continue;
            };
            let bone = pose.get_mut(index).ok_or(EvalError::Invalid)?;
            // Native blending retains the greatest frame setting across active clips.
            bone.rotation_relative_to_entity |= channel.rotation_relative_to_entity;
            let current = bone.property(channel.property);
            // `this` reads the bone orientation, not an animation-only delta. The
            // bone's defaults are copied into that orientation before channels add their values.
            let defaults =
                default_channel(bones, index, channel.property).ok_or(EvalError::Invalid)?;
            let this = std::array::from_fn(|axis| match channel.property {
                EntityAnimationProperty::Scale => defaults[axis] * current[axis],
                _ => defaults[axis] + current[axis],
            });
            let value = sample_channel(
                assets,
                channel.first_keyframe,
                channel.keyframe_count,
                time,
                |keyframe| keyframe_value(evaluator, variables, keyframe, this, budget),
            )?;
            for (axis, value) in value.into_iter().enumerate() {
                if channel.property == EntityAnimationProperty::Scale {
                    current[axis] *= 1.0 + (value - 1.0) * weight;
                } else {
                    current[axis] += value * weight;
                }
            }
        }
    }
    if let (Some(blend), Some(sides)) = (pending, sides.as_mut()) {
        compose_blend(&mut local, sides, blend.amount);
    }
    Ok(local)
}

fn channel_bone(channel: &assets::EntityAnimationChannel, names: &[Box<str>]) -> Option<usize> {
    match &channel.bone_name {
        Some(name) => names.iter().position(|candidate| candidate == name),
        None => Some(channel.bone as usize),
    }
}

/// Lerps the outgoing and incoming poses, rotating the short way round, then adds translation
/// and rotation to `local` and multiplies its scale. Both sides reset for the next blend.
fn compose_blend(local: &mut [LocalDelta], sides: &mut [Vec<LocalDelta>; 2], amount: f32) {
    let [from, to] = sides;
    for ((bone, from), to) in local.iter_mut().zip(from.iter_mut()).zip(to.iter_mut()) {
        for axis in 0..3 {
            let (a, b) = (from.translation[axis], to.translation[axis]);
            bone.translation[axis] += a + (b - a) * amount;
            let (a, b) = (from.rotation[axis], to.rotation[axis]);
            bone.rotation[axis] += a + ((b - a + 180.0).rem_euclid(360.0) - 180.0) * amount;
            let (a, b) = (from.scale[axis], to.scale[axis]);
            bone.scale[axis] *= a + (b - a) * amount;
        }
        bone.rotation_relative_to_entity |=
            from.rotation_relative_to_entity || to.rotation_relative_to_entity;
        *from = LocalDelta::default();
        *to = LocalDelta::default();
    }
}

fn default_channel(
    bones: &[RuntimeBone],
    index: usize,
    property: EntityAnimationProperty,
) -> Option<[f32; 3]> {
    let bone = bones.get(index)?;
    if matches!(bone.attachable_root, AttachableRootFrame::MatchingOwnerName) {
        return Some(match property {
            EntityAnimationProperty::Scale => [1.0; 3],
            _ => [0.0; 3],
        });
    }
    Some(match property {
        EntityAnimationProperty::Rotation => bone.rotation,
        EntityAnimationProperty::Scale => [1.0; 3],
        EntityAnimationProperty::Translation => {
            // Bones use an authored X/Z frame and a 24-pixel Y origin. A
            // parented part stores a relative pivot; only roots retain that origin.
            // Vanilla negates that Y before exposing it to Molang.
            let origin = match bone.parent {
                Some(parent) => bones.get(parent)?.pivot,
                None => [0.0, MODEL_PART_ORIGIN_Y, 0.0],
            };
            [
                origin[0] - bone.pivot[0],
                bone.pivot[1] - origin[1],
                bone.pivot[2] - origin[2],
            ]
        }
    })
}

fn keyframe_value(
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
    keyframe: &EntityAnimationKeyframe,
    this: [f32; 3],
    budget: &mut EvalBudget<'_>,
) -> Result<[f32; 3], EvalError> {
    let mut value = keyframe.value.map(|value| value.get());
    for axis in 0..3 {
        if let Some(expression) = keyframe.expressions[axis] {
            value[axis] = evaluator.number(expression as usize, variables, this[axis], budget)?;
        }
    }
    Ok(value)
}

fn sample_channel(
    assets: &RuntimeEntityAssets,
    first: u32,
    count: u32,
    time: f32,
    mut value: impl FnMut(&EntityAnimationKeyframe) -> Result<[f32; 3], EvalError>,
) -> Result<[f32; 3], EvalError> {
    let first = first as usize;
    let frames = assets
        .animation_keyframes()
        .get(
            first
                ..first
                    .checked_add(count as usize)
                    .ok_or(EvalError::Invalid)?,
        )
        .ok_or(EvalError::Invalid)?;
    let first_frame = frames.first().ok_or(EvalError::Invalid)?;
    if time < first_frame.time_seconds.get() {
        return value(first_frame);
    }
    let exact_end = frames.partition_point(|frame| frame.time_seconds.get() <= time);
    if exact_end > 0 && frames[exact_end - 1].time_seconds.get() == time {
        return value(&frames[exact_end - 1]);
    }
    if exact_end == frames.len() {
        return value(&frames[frames.len() - 1]);
    }
    let left_index = exact_end - 1;
    let right_index = exact_end;
    let left = &frames[left_index];
    let right = &frames[right_index];
    let left_time = left.time_seconds.get();
    let right_time = right.time_seconds.get();
    let amount = ((time - left_time) / (right_time - left_time)).clamp(0.0, 1.0);
    let left_value = value(left)?;
    match left.interpolation {
        EntityAnimationInterpolation::Step => Ok(left_value),
        EntityAnimationInterpolation::Linear => Ok(lerp3(left_value, value(right)?, amount)),
        EntityAnimationInterpolation::CatmullRom => {
            let right_value = value(right)?;
            let previous = match frames.get(left_index.wrapping_sub(1)) {
                Some(frame) if left_index > 0 => value(frame)?,
                _ => left_value,
            };
            let next = match frames.get(right_index + 1) {
                Some(frame) => value(frame)?,
                None => right_value,
            };
            Ok(std::array::from_fn(|axis| {
                catmull(
                    previous[axis],
                    left_value[axis],
                    right_value[axis],
                    next[axis],
                    amount,
                )
            }))
        }
    }
}

pub(super) fn compose_pose(
    bones: &[RuntimeBone],
    local: &[LocalDelta],
) -> Option<Vec<BoneTransform>> {
    compose_pose_with_targets(bones, local, &[])
}

/// Retarget named joints in model space while composing their clothing and other children.
/// Callers validate the complete hierarchy with an untargeted composition first.
pub(super) fn compose_pose_with_targets(
    bones: &[RuntimeBone],
    local: &[LocalDelta],
    targets: &[Option<BoneTransform>],
) -> Option<Vec<BoneTransform>> {
    let mut transforms = vec![None; bones.len()];
    let mut visiting = vec![false; bones.len()];
    for index in 0..bones.len() {
        compose_bone(index, bones, local, targets, &mut transforms, &mut visiting)?;
    }
    transforms.into_iter().collect()
}

fn compose_bone(
    index: usize,
    bones: &[RuntimeBone],
    local: &[LocalDelta],
    targets: &[Option<BoneTransform>],
    transforms: &mut [Option<BoneTransform>],
    visiting: &mut [bool],
) -> Option<BoneTransform> {
    if let Some(transform) = transforms.get(index).copied().flatten() {
        return Some(transform);
    }
    if *visiting.get(index)? {
        return None;
    }
    visiting[index] = true;
    if let Some(target) = targets.get(index).copied().flatten() {
        if target
            .rotation
            .iter()
            .chain(target.translation_scale.iter())
            .chain(target.axis_scale.iter())
            .any(|value| !value.is_finite())
        {
            return None;
        }
        visiting[index] = false;
        transforms[index] = Some(target);
        return Some(target);
    }
    let bone = bones.get(index)?;
    let delta = local.get(index).copied().unwrap_or_default();
    // Owner-name binding clears defaults; an explicit expression keeps bone defaults.
    // Keep the authored pivot unchanged: child offsets and mesh bind coordinates still use it.
    let (root_pivot, root_rotation) = match bone.attachable_root {
        AttachableRootFrame::Actor => (bone.pivot, bone.rotation),
        AttachableRootFrame::MatchingOwnerName => ([0.0; 3], [0.0; 3]),
        AttachableRootFrame::BindingExpression => (
            [
                bone.pivot[0],
                bone.pivot[1] - MODEL_PART_ORIGIN_Y,
                bone.pivot[2],
            ],
            bone.rotation,
        ),
    };
    // Pivots are already in the X-mirrored rig frame; authored offsets and angles are not.
    let translation = std::array::from_fn(|axis| {
        let parent_pivot = bone
            .parent
            .and_then(|parent| bones.get(parent))
            .map_or(0.0, |parent| parent.pivot[axis]);
        let offset = if axis == 0 {
            -delta.translation[axis]
        } else {
            delta.translation[axis]
        };
        root_pivot[axis] - parent_pivot + offset
    });
    let [x, y, z] = std::array::from_fn(|axis| root_rotation[axis] + delta.rotation[axis]);
    // Authored X and Y angles turn against the right-hand rule in the mirrored frame.
    let rotation = quat_from_euler([-x, -y, z]);
    let transform = if let Some(parent_index) = bone.parent {
        let parent = compose_bone(parent_index, bones, local, targets, transforms, visiting)?;
        let parent_scale = total_scale(&parent);
        let scaled = std::array::from_fn(|axis| translation[axis] * parent_scale[axis]);
        let rotated = rotate_vector(parent.rotation, scaled);
        // A non-uniform parent scale under a rotated child would shear; the child keeps the
        // componentwise product, exact only for a uniform parent scale or an unturned child.
        // Entity-relative rotation resets the inherited basis after translating the pivot.
        // This removes both parent rotation and scale; descendants inherit our new basis.
        let (rotation, scale) = if delta.rotation_relative_to_entity {
            (rotation, delta.scale)
        } else {
            (
                quat_multiply(parent.rotation, rotation),
                std::array::from_fn(|axis| parent_scale[axis] * delta.scale[axis]),
            )
        };
        with_scale(
            rotation,
            std::array::from_fn(|axis| parent.translation_scale[axis] + rotated[axis]),
            scale,
        )
    } else {
        with_scale(rotation, translation, delta.scale)
    };
    if transform
        .rotation
        .iter()
        .chain(transform.translation_scale.iter())
        .chain(transform.axis_scale.iter())
        .any(|value| !value.is_finite())
    {
        return None;
    }
    visiting[index] = false;
    transforms[index] = Some(transform);
    Some(transform)
}

pub(super) fn total_scale(transform: &BoneTransform) -> [f32; 3] {
    transform
        .axis_scale
        .map(|axis| axis * transform.translation_scale[3])
}

/// Stores a uniform scale in `translation_scale[3]` and anything else per axis.
pub(super) fn with_scale(
    rotation: [f32; 4],
    translation: [f32; 3],
    scale: [f32; 3],
) -> BoneTransform {
    let uniform = scale[0] == scale[1] && scale[1] == scale[2];
    BoneTransform {
        rotation,
        translation_scale: [
            translation[0],
            translation[1],
            translation[2],
            if uniform { scale[0] } else { 1.0 },
        ],
        axis_scale: if uniform { [1.0; 3] } else { scale },
    }
}

pub(super) fn quat_from_euler(rotation: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = rotation.map(|value| value.to_radians() * 0.5);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}

pub(super) fn quat_multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub(super) fn rotate_vector(rotation: [f32; 4], vector: [f32; 3]) -> [f32; 3] {
    let qvector = [vector[0], vector[1], vector[2], 0.0];
    let inverse = [-rotation[0], -rotation[1], -rotation[2], rotation[3]];
    let result = quat_multiply(quat_multiply(rotation, qvector), inverse);
    [result[0], result[1], result[2]]
}

fn lerp3(left: [f32; 3], right: [f32; 3], amount: f32) -> [f32; 3] {
    std::array::from_fn(|axis| left[axis] + (right[axis] - left[axis]) * amount)
}

fn catmull(p0: f32, p1: f32, p2: f32, p3: f32, amount: f32) -> f32 {
    let amount2 = amount * amount;
    let amount3 = amount2 * amount;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * amount
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * amount2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * amount3)
}
