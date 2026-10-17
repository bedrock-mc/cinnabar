//! Original top rock, windmill and inverted freeze, authored for the named biped rig.
use super::*;
mod joints;

const WINDMILL_START: f32 = 0.32;
const WINDMILL_END: f32 = 0.70;
const FLOOR_ENTRY: f32 = 0.24;
const FLOOR_EXIT: f32 = 0.78;
const FREEZE_END: f32 = 0.85;
const RECOVERY_END: f32 = 0.94;
const SPIN_DEGREES: f32 = 3.0 * 360.0;

#[derive(Clone, Copy)]
struct Frame {
    phase: f32,
    turn: [f32; 3],
    arms: [[f32; 3]; 2],
    legs: [[f32; 3]; 2],
    bends: [f32; 5],
}

/// Stores the authored channels once; phase is a fraction of the catalog period.
const fn frame(
    phase: f32,
    turn: [f32; 3],
    arms: [[f32; 3]; 2],
    legs: [[f32; 3]; 2],
    bends: [f32; 5],
) -> Frame {
    Frame {
        phase,
        turn,
        arms,
        legs,
        bends,
    }
}

const FRAMES: &[Frame] = &[
    frame(
        0.0,
        [0.0, 0.0, 0.0],
        [[0.0; 3]; 2],
        [[0.0; 3]; 2],
        [8.0, 8.0, 6.0, 6.0, 0.0],
    ),
    frame(
        0.08,
        [0.0, -18.0, -8.0],
        [[-45.0, 0.0, -35.0], [30.0, 0.0, 40.0]],
        [[-25.0, 0.0, -18.0], [15.0, 0.0, 8.0]],
        [55.0, 85.0, 40.0, 10.0, 8.0],
    ),
    frame(
        0.16,
        [0.0, 18.0, 8.0],
        [[30.0, 0.0, -40.0], [-45.0, 0.0, 35.0]],
        [[15.0, 0.0, -8.0], [-25.0, 0.0, 18.0]],
        [85.0, 55.0, 10.0, 40.0, -8.0],
    ),
    frame(
        FLOOR_ENTRY,
        [45.0, 0.0, -25.0],
        [[-85.0, 0.0, -20.0], [-50.0, 0.0, 30.0]],
        [[65.0, 0.0, -25.0], [-55.0, 0.0, 35.0]],
        [90.0, 60.0, 95.0, 45.0, 22.0],
    ),
    frame(
        WINDMILL_START,
        [125.0, 0.0, 0.0],
        [[0.0, 0.0, -75.0], [0.0, 0.0, 75.0]],
        [[-15.0, 0.0, -65.0], [15.0, 0.0, 65.0]],
        [20.0, 55.0, 20.0, 45.0, -10.0],
    ),
    frame(
        WINDMILL_END,
        [125.0, SPIN_DEGREES, 0.0],
        [[0.0, 0.0, -75.0], [0.0, 0.0, 75.0]],
        [[-15.0, 0.0, -65.0], [15.0, 0.0, 65.0]],
        [20.0, 55.0, 20.0, 45.0, -10.0],
    ),
    frame(
        FLOOR_EXIT,
        [180.0, SPIN_DEGREES, -12.0],
        [[-165.0, 0.0, -8.0], [-155.0, 0.0, 12.0]],
        [[-20.0, 0.0, -40.0], [35.0, 0.0, 25.0]],
        [60.0, 95.0, 20.0, 100.0, -12.0],
    ),
    frame(
        FREEZE_END,
        [180.0, SPIN_DEGREES, -12.0],
        [[-165.0, 0.0, -8.0], [-155.0, 0.0, 12.0]],
        [[-20.0, 0.0, -40.0], [35.0, 0.0, 25.0]],
        [60.0, 95.0, 20.0, 100.0, -12.0],
    ),
    frame(
        RECOVERY_END,
        [330.0, SPIN_DEGREES, 0.0],
        [[-65.0, 0.0, -20.0], [-65.0, 0.0, 20.0]],
        [[35.0, 0.0, -15.0], [35.0, 0.0, 15.0]],
        [60.0, 60.0, 80.0, 80.0, 20.0],
    ),
    frame(
        1.0,
        [360.0, SPIN_DEGREES, 0.0],
        [[0.0; 3]; 2],
        [[0.0; 3]; 2],
        [8.0, 8.0, 6.0, 6.0, 0.0],
    ),
];

/// Eases entrances and the freeze; the windmill keeps its angular speed between turns.
fn sample(seconds: f64) -> Frame {
    let phase = (seconds.rem_euclid(CustomEmote::Breakdance.duration_seconds())
        / CustomEmote::Breakdance.duration_seconds()) as f32;
    let [a, b] = FRAMES
        .windows(2)
        .find(|pair| phase <= pair[1].phase)
        .unwrap()
    else {
        unreachable!("windows always yields two frames")
    };
    let linear = (phase - a.phase) / (b.phase - a.phase);
    let amount = if a.phase == WINDMILL_START {
        linear
    } else {
        linear * linear * (3.0 - 2.0 * linear)
    };
    let blend = |a: [f32; 3], b: [f32; 3]| {
        std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * amount)
    };
    let mut result = frame(
        phase,
        blend(a.turn, b.turn),
        std::array::from_fn(|side| blend(a.arms[side], b.arms[side])),
        std::array::from_fn(|side| blend(a.legs[side], b.legs[side])),
        std::array::from_fn(|index| a.bends[index] + (b.bends[index] - a.bends[index]) * amount),
    );
    if (WINDMILL_START..WINDMILL_END).contains(&phase) {
        let scissor = (result.turn[1].to_radians() * 2.0).sin() * 22.0;
        result.legs[0][0] += scissor;
        result.legs[1][0] -= scissor;
        result.bends[2] += scissor * 0.5;
        result.bends[3] -= scissor * 0.5;
    }
    result
}

/// Transfers the spin support from hips to upper back during the floor entry and exit.
fn shoulder_support(phase: f32) -> f32 {
    let entering = (phase - FLOOR_ENTRY) / (WINDMILL_START - FLOOR_ENTRY);
    let leaving = (FLOOR_EXIT - phase) / (FLOOR_EXIT - WINDMILL_END);
    let amount = entering.min(leaving).clamp(0.0, 1.0);
    amount * amount * (3.0 - 2.0 * amount)
}

/// Applies world-space joint targets so independent limbs and hierarchical skins agree.
pub(super) fn targets(
    bones: &[RuntimeBone],
    names: &[Box<str>],
    rest: &[BoneTransform],
    seconds: f64,
    height: f32,
    bounds: Option<&[Option<articulated::BoneBounds>]>,
) -> Vec<Option<BoneTransform>> {
    let mut frame = sample(seconds);
    if bounds.is_some() {
        // Settle both shoulders level as the hands take the weight.
        frame.turn[2] *= 1.0 - joints::freeze_weight(frame.phase);
    }
    let support = shoulder_support(frame.phase);
    if bounds.is_some() {
        frame.turn[0] += 10.0 * support;
        for arm in &mut frame.arms {
            arm[0] += 35.0 * support;
            arm[2] *= 1.0 - support;
        }
    }
    let shoulders = names
        .iter()
        .position(|name| name.as_ref() == "body")
        .map_or([0.0, height * 2.0, 0.0], |index| {
            std::array::from_fn(|axis| rest[index].translation_scale[axis])
        });
    let pivot = [
        shoulders[0] * support,
        height + (shoulders[1] - height) * support,
        (shoulders[2] + height / 6.0) * support,
    ];
    // Separate world yaw from the floor tilt so the windmill circles the floor.
    let turn = quat_multiply(
        quat_from_euler([0.0, frame.turn[1].rem_euclid(360.0), 0.0]),
        quat_from_euler([frame.turn[0].rem_euclid(360.0), 0.0, frame.turn[2]]),
    );
    let mut targets: Vec<_> = bones
        .iter()
        .zip(names)
        .zip(rest)
        .map(|((bone, name), rest)| {
            let rotation = match name.as_ref() {
                "waist" | "body" => [0.0; 3],
                "head" => [(10.0 - frame.turn[0]) * support, 0.0, 0.0],
                "leftarm" => frame.arms[0],
                "rightarm" => frame.arms[1],
                "leftleg" => frame.legs[0],
                "rightleg" => frame.legs[1],
                _ if bone.parent.is_none() => [0.0; 3],
                _ => return None,
            };
            let mut target = *rest;
            let relative = std::array::from_fn(|axis| rest.translation_scale[axis] - pivot[axis]);
            let pivot = rotate_vector(turn, relative);
            target.translation_scale[..3].copy_from_slice(&[pivot[0], pivot[1] + height, pivot[2]]);
            target.rotation = quat_multiply(
                turn,
                quat_multiply(quat_from_euler(rotation), rest.rotation),
            );
            Some(target)
        })
        .collect();
    joints::apply(names, rest, &mut targets, frame, height, bounds);
    targets
}

/// Places the composed silhouette on the floor, including every articulated segment.
pub(super) fn ground_pose(
    names: &[Box<str>],
    pose: &mut [BoneTransform],
    height: f32,
    bounds: Option<&[Option<articulated::BoneBounds>]>,
) {
    let lowest = names
        .iter()
        .zip(pose.iter())
        .enumerate()
        .filter_map(|(index, (name, target))| {
            let bounds = if let Some(bounds) = bounds {
                bounds.get(index).copied().flatten()?
            } else {
                let (width, bottom, top, depth) = match name.as_ref() {
                    "head" => (4.0, 0.0, 8.0, 4.0),
                    "body" => (4.0, -12.0, 0.0, 2.0),
                    "leftarm" | "rightarm" | "leftleg" | "rightleg" => (2.0, -12.0, 0.0, 2.0),
                    _ => return None,
                };
                let scale = height / 12.0;
                articulated::BoneBounds {
                    min: [-width * scale, bottom * scale, -depth * scale],
                    max: [width * scale, top * scale, depth * scale],
                }
            };
            let mut minimum = f32::INFINITY;
            for x in [bounds.min[0], bounds.max[0]] {
                for y in [bounds.min[1], bounds.max[1]] {
                    for z in [bounds.min[2], bounds.max[2]] {
                        minimum = minimum.min(
                            target.translation_scale[1]
                                + rotate_vector(
                                    target.rotation,
                                    [x, y, z].map(|v| v * target.translation_scale[3]),
                                )[1],
                        );
                    }
                }
            }
            Some(minimum)
        })
        .fold(f32::INFINITY, f32::min);
    if lowest.is_finite() {
        for target in pose {
            target.translation_scale[1] -= lowest;
        }
    }
}
