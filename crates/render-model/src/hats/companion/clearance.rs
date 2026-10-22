//! Body sections covering locomotion artwork without depending on catalog bone names.
use super::{Gait, Pose};
use crate::hats::HatRig;
use glam::{Quat, Vec3};

/// A bone-local horizontal circle over its animated vertical range.
/// The centre rotates with the companion, keeping the tail behind the body.
#[derive(Clone, Copy, Debug)]
pub struct Clearance {
    pub centre: [f32; 2],
    pub radius: f32,
    pub bottom: f32,
    pub top: f32,
}

/// Computes immutable clearance once at admission, retaining the feet as the ground anchor.
pub(in crate::hats) fn build(model: &HatRig) -> Box<[Clearance]> {
    let Some(rig) = model.companion() else {
        return Box::new([]);
    };
    let mut bounds =
        vec![(Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)); model.parents.len()];
    for gait in [Gait::Idle, Gait::Sit, Gait::Trot, Gait::Gallop] {
        for step in 0..=64 {
            let pose = model.companion_pose(Pose {
                gait,
                seconds: rig.clips[gait.index()].length() * step as f32 / 64.0,
            });
            for vertex in model.geometry.vertices.iter() {
                let index = vertex.bone_index as usize;
                let bone = pose[index];
                let point = Vec3::from_slice(&bone.translation_scale[..3])
                    + Quat::from_array(bone.rotation)
                        * (Vec3::from_array(vertex.position)
                            - Vec3::from_array(model.geometry.bone_pivots[index]));
                let bounds = &mut bounds[index];
                bounds.0 = bounds.0.min(point);
                bounds.1 = bounds.1.max(point);
            }
        }
    }
    bounds
        .into_iter()
        .filter(|(_, max)| max.y > 0.0)
        .map(|(min, max)| Clearance {
            centre: [(min.x + max.x) * 0.5, (min.z + max.z) * 0.5],
            // Allow for interpolation between samples and pose blending at gait changes.
            radius: (max.x - min.x).hypot(max.z - min.z) * 0.5 + 0.03,
            bottom: (min.y - 0.03).max(0.0),
            top: max.y + 0.03,
        })
        .collect()
}
