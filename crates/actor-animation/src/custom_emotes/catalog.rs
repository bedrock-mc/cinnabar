//! Retargets bounded catalog channels through the schema's logical articulated rig.
use super::*;
use cosmetics::emotes::{Clip, Key};

/// Samples unit rotations on the short arc and bind-relative local translations.
fn sample(keys: &[Key], time: f64) -> Key {
    let end = keys.partition_point(|key| key.time <= time);
    if end == 0 {
        return keys[0];
    }
    if end == keys.len() {
        return keys[end - 1];
    }
    let a = keys[end - 1];
    let b = keys[end];
    let t = (time - a.time) / (b.time - a.time);
    let hemisphere = if a
        .rotation
        .iter()
        .zip(b.rotation)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        < 0.0
    {
        -1.0
    } else {
        1.0
    };
    let q: [f64; 4] = std::array::from_fn(|axis| {
        a.rotation[axis] + (b.rotation[axis] * hemisphere - a.rotation[axis]) * t
    });
    let length = q.iter().map(|value| value * value).sum::<f64>().sqrt();
    Key {
        time,
        rotation: q.map(|value| value / length),
        offset: std::array::from_fn(|axis| a.offset[axis] + (b.offset[axis] - a.offset[axis]) * t),
    }
}

/// Logical bone targets and the authored vertical root offset in model units.
pub(super) struct Targets {
    pub bones: Vec<Option<BoneTransform>>,
    pub root_height: f32,
}

/// Applies local logical-joint channels while leaving clothing to the skin's own hierarchy.
pub(super) fn targets(
    names: &[Box<str>],
    rest: &[BoneTransform],
    clip: &Clip,
    seconds: f64,
    height: f32,
) -> Targets {
    let time = if clip.looping {
        seconds.rem_euclid(clip.duration)
    } else {
        seconds.min(clip.duration)
    };
    let rig = cosmetics::emotes::rig();
    let reference_height = rig
        .iter()
        .find(|joint| joint.joint == "leftleg")
        .expect("rig leg height")
        .pivot[1];
    let indices: Vec<_> = rig
        .iter()
        .map(|joint| names.iter().position(|name| **name == joint.joint))
        .collect();
    let bind: Vec<_> = rig
        .iter()
        .zip(&indices)
        .map(|(joint, index)| {
            index.map_or_else(
                || {
                    crate::pose::with_scale(
                        [0.0, 0.0, 0.0, 1.0],
                        joint.pivot.map(|value| value * height / reference_height),
                        [1.0; 3],
                    )
                },
                |index| rest[index],
            )
        })
        .collect();
    let mut logical: Vec<BoneTransform> = Vec::with_capacity(rig.len());
    let mut targets = vec![None; names.len()];
    let mut root_height = 0.0;
    for (index, joint) in rig.iter().enumerate() {
        let key = clip
            .tracks
            .iter()
            .find(|track| track.joint == joint.joint)
            .map(|track| sample(&track.keys, time));
        let rotation = key.map_or([0.0, 0.0, 0.0, 1.0], |key| {
            key.rotation.map(|value| value as f32)
        });
        let offset = key.map_or([0.0; 3], |key| {
            key.offset.map(|value| value as f32 * height)
        });
        let mut target = bind[index];
        if let Some(parent) = joint.parent_index {
            let parent_pose = logical[parent];
            let inverse_bind = [
                -bind[parent].rotation[0],
                -bind[parent].rotation[1],
                -bind[parent].rotation[2],
                bind[parent].rotation[3],
            ];
            let bind_rotation = quat_multiply(inverse_bind, bind[index].rotation);
            let bind_offset = rotate_vector(
                inverse_bind,
                std::array::from_fn(|axis| {
                    bind[index].translation_scale[axis] - bind[parent].translation_scale[axis]
                }),
            );
            let moved = rotate_vector(
                parent_pose.rotation,
                std::array::from_fn(|axis| bind_offset[axis] + offset[axis]),
            );
            target.rotation =
                quat_multiply(parent_pose.rotation, quat_multiply(rotation, bind_rotation));
            for (axis, value) in moved.into_iter().enumerate() {
                target.translation_scale[axis] = parent_pose.translation_scale[axis] + value;
            }
        } else {
            root_height = offset[1];
            target.rotation = quat_multiply(rotation, bind[index].rotation);
            for (axis, value) in offset.into_iter().enumerate() {
                target.translation_scale[axis] += value;
            }
        }
        logical.push(target);
        if let Some(index) = indices[index] {
            targets[index] = Some(target);
        }
    }
    Targets {
        bones: targets,
        root_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframes_interpolate_rotations_and_offsets_and_clamp_nonloops() {
        let keys = [
            Key {
                time: 0.0,
                rotation: [0.0, 0.0, 0.0, 1.0],
                offset: [0.0; 3],
            },
            Key {
                time: 2.0,
                rotation: [0.0, 0.0, 1.0, 0.0],
                offset: [0.0, 0.25, 0.0],
            },
        ];
        let key = sample(&keys, 1.0);
        assert!((key.rotation[2] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert_eq!(key.offset, [0.0, 0.125, 0.0]);
        let names: Vec<Box<str>> = ["root", "body", "head"].map(Into::into).into();
        let rest = [
            BoneTransform {
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation_scale: [0.0, 0.0, 0.0, 1.0],
                axis_scale: [1.0; 3],
            },
            BoneTransform {
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation_scale: [0.0, 24.0, 0.0, 1.0],
                axis_scale: [1.0; 3],
            },
            BoneTransform {
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation_scale: [0.0, 24.0, 0.0, 1.0],
                axis_scale: [1.0; 3],
            },
        ];
        let mut clip = Clip {
            version: 1,
            duration: 2.0,
            looping: false,
            tracks: vec![cosmetics::emotes::Track {
                joint: "body".into(),
                keys: keys.into(),
            }],
        };
        let end = targets(&names, &rest, &clip, 2.0, 12.0).bones;
        assert_eq!(targets(&names, &rest, &clip, 200.0, 12.0).bones, end);
        assert_eq!(end[1].unwrap().translation_scale[1], 27.0);
        assert_eq!(
            end[2], end[1],
            "untracked head inherits its logical parent's motion"
        );
        clip.looping = true;
        assert_eq!(
            targets(&names, &rest, &clip, 0.0, 12.0).bones,
            targets(&names, &rest, &clip, 2.0, 12.0).bones
        );
        clip.tracks = vec![cosmetics::emotes::Track {
            joint: "root".into(),
            keys: vec![Key {
                time: 0.0,
                rotation: [0.0, 0.0, 1.0, 0.0],
                offset: [0.0; 3],
            }],
        }];
        let complete = targets(&names, &rest, &clip, 0.0, 12.0).bones;
        let rootless = targets(&names[1..], &rest[1..], &clip, 0.0, 12.0).bones;
        assert_eq!(
            rootless,
            complete[1..],
            "virtual logical joints also drive independently rooted skin bones"
        );
    }

    #[test]
    fn opposite_quaternion_signs_take_the_same_short_rotation() {
        let keys = [
            Key {
                time: 0.0,
                rotation: [0.0, 0.0, 0.0, 1.0],
                offset: [0.0; 3],
            },
            Key {
                time: 1.0,
                rotation: [0.0, 0.0, 0.0, -1.0],
                offset: [0.0; 3],
            },
        ];
        assert_eq!(sample(&keys, 0.5).rotation, [0.0, 0.0, 0.0, 1.0]);
    }
}
