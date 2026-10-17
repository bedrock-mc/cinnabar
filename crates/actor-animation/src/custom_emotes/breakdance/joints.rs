//! Joint channels and supported hand contacts for the full breakdance skeleton.
use super::*;

/// Required joints are resolved together so custom partial rigs keep their authored shape.
struct Joints {
    body: usize,
    head: usize,
    spine: usize,
    waist: Option<usize>,
    arms: [[usize; 3]; 2],
    legs: [[usize; 3]; 2],
}

impl Joints {
    /// Resolves the render-only markers independently of source bone order.
    fn resolve(names: &[Box<str>]) -> Option<Self> {
        let find = |name: &str| names.iter().position(|part| part.as_ref() == name);
        let chain = |root: &str, middle: &str, end: &str| {
            Some([
                find(root)?,
                find(&format!("{root}.cinnabar_{middle}"))?,
                find(&format!("{root}.cinnabar_{end}"))?,
            ])
        };
        Some(Self {
            body: find("body")?,
            head: find("head")?,
            spine: find("body.cinnabar_spine")?,
            waist: find("waist"),
            arms: [
                chain("leftarm", "elbow", "wrist")?,
                chain("rightarm", "elbow", "wrist")?,
            ],
            legs: [
                chain("leftleg", "knee", "ankle")?,
                chain("rightleg", "knee", "ankle")?,
            ],
        })
    }
}

/// Adds spine, hinge and wrist channels, then transfers weight onto both palms for the freeze.
pub(super) fn apply(
    names: &[Box<str>],
    rest: &[BoneTransform],
    targets: &mut [Option<BoneTransform>],
    frame: Frame,
    height: f32,
    bounds: Option<&[Option<articulated::BoneBounds>]>,
) {
    let Some(bounds) = bounds else {
        return;
    };
    let Some(joints) = Joints::resolve(names) else {
        return;
    };
    let freeze = freeze_weight(frame.phase);
    if freeze > 0.0 {
        let shoulder_y = joints
            .arms
            .iter()
            .map(|chain| targets[chain[0]].unwrap().translation_scale[1])
            .sum::<f32>()
            * 0.5;
        let lift = (height * 0.75 - shoulder_y) * freeze;
        for target in targets.iter_mut().flatten() {
            target.translation_scale[1] += lift;
        }
        let body = targets[joints.body].unwrap();
        let tucked = quat_multiply(
            body.rotation,
            quat_from_euler([100.0 - frame.turn[0], 0.0, 0.0]),
        );
        let head = targets[joints.head].as_mut().unwrap();
        head.rotation = blend_rotation(head.rotation, tucked, freeze);
    }
    let body = targets[joints.body].unwrap();
    let twist = if (WINDMILL_START..WINDMILL_END).contains(&frame.phase) {
        frame.turn[1].to_radians().sin() * 12.0
    } else {
        0.0
    };
    let spine = child(
        body,
        joints.body,
        joints.spine,
        rest,
        [frame.bends[4], twist, 0.0],
    );
    targets[joints.spine] = Some(spine);
    let hip = [0.0, height, 0.0];
    let old_hip = point(body, subtract(hip, position(rest[joints.body])));
    let new_hip = point(spine, subtract(hip, position(rest[joints.spine])));
    let pelvis_delta = subtract(new_hip, old_hip);
    if let Some(waist) = joints.waist {
        let target = targets[waist].as_mut().unwrap();
        target.rotation = spine.rotation;
        for (axis, delta) in pelvis_delta.iter().enumerate() {
            target.translation_scale[axis] += delta;
        }
    }
    for (side, [hip, knee, ankle]) in joints.legs.into_iter().enumerate() {
        let thigh = targets[hip].as_mut().unwrap();
        let old = point(
            body,
            subtract(position(rest[hip]), position(rest[joints.body])),
        );
        let new = point(
            spine,
            subtract(position(rest[hip]), position(rest[joints.spine])),
        );
        for axis in 0..3 {
            thigh.translation_scale[axis] += new[axis] - old[axis];
        }
        let inverse_body = [
            -body.rotation[0],
            -body.rotation[1],
            -body.rotation[2],
            body.rotation[3],
        ];
        thigh.rotation = quat_multiply(spine.rotation, quat_multiply(inverse_body, thigh.rotation));
        let thigh = *thigh;
        // Flexion is a hinge: the shin folds behind the thigh rather than changing length.
        let shin = child(thigh, hip, knee, rest, [-frame.bends[side + 2], 0.0, 0.0]);
        let foot = child(
            shin,
            knee,
            ankle,
            rest,
            [frame.bends[side + 2] * 0.45, 0.0, 0.0],
        );
        targets[knee] = Some(shin);
        targets[ankle] = Some(foot);
    }
    for (side, [arm, elbow, wrist]) in joints.arms.into_iter().enumerate() {
        let upper = targets[arm].unwrap();
        let forearm = child(upper, arm, elbow, rest, [frame.bends[side], 0.0, 0.0]);
        let hand = child(
            forearm,
            elbow,
            wrist,
            rest,
            [-frame.bends[side] * 0.3, 0.0, 0.0],
        );
        targets[elbow] = Some(forearm);
        targets[wrist] = Some(hand);
        if freeze > 0.0 {
            let shoulder = position(upper);
            let side = position(rest[arm])[0].signum();
            // Attached sleeves can extend below the bare hand. Match their
            // shared bind pivot so the outer palm, rather than the skin, bears weight.
            let palm_y = bounds
                .iter()
                .zip(rest)
                .filter_map(|(bounds, part)| {
                    if length(subtract(position(*part), position(rest[wrist]))) > 0.001 {
                        return None;
                    }
                    Some(-bounds.as_ref()?.min[1])
                })
                .reduce(f32::max)
                .unwrap_or(0.0);
            let palm = [
                shoulder[0] + side * height * 0.10,
                palm_y,
                shoulder[2] + height * 0.10,
            ];
            support_arm([arm, elbow, wrist], rest, targets, palm, freeze);
        }
    }
}

/// Transfers support to both hands during the held freeze and releases it for the return.
pub(super) fn freeze_weight(phase: f32) -> f32 {
    ease((phase - WINDMILL_END) / (FLOOR_EXIT - WINDMILL_END))
        * ease((RECOVERY_END - phase) / (RECOVERY_END - FREEZE_END))
}

/// Clamps and smooths a transition without changing its held endpoints.
fn ease(amount: f32) -> f32 {
    let t = amount.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Places a child pivot exactly at its parent's transformed bind offset.
fn child(
    parent: BoneTransform,
    a: usize,
    b: usize,
    rest: &[BoneTransform],
    angles: [f32; 3],
) -> BoneTransform {
    let mut target = rest[b];
    target.translation_scale[..3].copy_from_slice(&point(
        parent,
        subtract(position(rest[b]), position(rest[a])),
    ));
    target.rotation = quat_multiply(parent.rotation, quat_from_euler(angles));
    target
}

/// Solves fixed-length arm segments to a level palm with a forward elbow pole.
fn support_arm(
    chain: [usize; 3],
    rest: &[BoneTransform],
    targets: &mut [Option<BoneTransform>],
    palm: [f32; 3],
    amount: f32,
) {
    let [arm, elbow, wrist] = chain;
    let shoulder = targets[arm].unwrap();
    let start = position(shoulder);
    let a = length(subtract(position(rest[elbow]), position(rest[arm])));
    let b = length(subtract(position(rest[wrist]), position(rest[elbow])));
    let delta = subtract(palm, start);
    let distance = length(delta).clamp(0.001, a + b - 0.001);
    let direction = normalize(delta);
    let projection = direction[2];
    let pole = normalize(std::array::from_fn(|axis| {
        if axis == 2 {
            1.0 - direction[axis] * projection
        } else {
            -direction[axis] * projection
        }
    }));
    let along = (a * a - b * b + distance * distance) / (2.0 * distance);
    let bend = (a * a - along * along).max(0.0).sqrt();
    let joint =
        std::array::from_fn(|axis| start[axis] + direction[axis] * along + pole[axis] * bend);
    let endpoint = std::array::from_fn(|axis| start[axis] + direction[axis] * distance);
    let upper_rotation = blend_rotation(shoulder.rotation, aim(subtract(joint, start)), amount);
    targets[arm].as_mut().unwrap().rotation = upper_rotation;
    let mut forearm = targets[elbow].unwrap();
    forearm.translation_scale[..3].copy_from_slice(&point(
        targets[arm].unwrap(),
        subtract(position(rest[elbow]), position(rest[arm])),
    ));
    forearm.rotation = blend_rotation(forearm.rotation, aim(subtract(endpoint, joint)), amount);
    targets[elbow] = Some(forearm);
    let mut hand = targets[wrist].unwrap();
    hand.translation_scale[..3].copy_from_slice(&point(
        forearm,
        subtract(position(rest[wrist]), position(rest[elbow])),
    ));
    hand.rotation = blend_rotation(hand.rotation, [0.0, 0.0, 0.0, 1.0], amount);
    targets[wrist] = Some(hand);
}

/// Rotates the downward bind axis onto a segment without changing its length.
fn aim(delta: [f32; 3]) -> [f32; 4] {
    let d = normalize(delta);
    if d[1] > 0.99999 {
        return [1.0, 0.0, 0.0, 0.0];
    }
    let q = [-d[2], 0.0, d[0], 1.0 - d[1]];
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    q.map(|v| v / length)
}

/// Interpolates unit rotations along the hemisphere containing the start pose.
fn blend_rotation(a: [f32; 4], mut b: [f32; 4], t: f32) -> [f32; 4] {
    if a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>() < 0.0 {
        b = b.map(|v| -v);
    }
    let q: [f32; 4] = std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t);
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    q.map(|v| v / length)
}

/// Returns the world position of a rigid joint.
fn position(bone: BoneTransform) -> [f32; 3] {
    std::array::from_fn(|axis| bone.translation_scale[axis])
}
/// Transforms a bind-relative offset into a pose position.
fn point(bone: BoneTransform, offset: [f32; 3]) -> [f32; 3] {
    let rotated = rotate_vector(bone.rotation, offset);
    std::array::from_fn(|axis| bone.translation_scale[axis] + rotated[axis])
}
/// Subtracts two joint positions.
fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|axis| a[axis] - b[axis])
}
/// Returns a segment length.
fn length(v: [f32; 3]) -> f32 {
    v.iter().map(|v| v * v).sum::<f32>().sqrt()
}
/// Returns a unit direction, with a stable fallback for degenerate poles.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = length(v);
    if len < 0.00001 {
        [1.0, 0.0, 0.0]
    } else {
        v.map(|v| v / len)
    }
}
