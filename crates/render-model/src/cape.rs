//! Shared cape geometry and poses for world players and the HUD.

use crate::{
    ActorRigGeometry, RenderBoneTransform, entity_geometry, equipment_rig_id, find_geometry_index,
    java_animation::{CAPE_MAX_SWING_DEGREES, JavaCapeInput, java_cape_bone},
    resolve_geometry_bones,
};
use assets::{CAPE_GEOMETRY_IDENTIFIER, RuntimeEntityAssets};
use glam::{EulerRot, Quat, Vec3};
use std::sync::Arc;

mod pose;
#[cfg(test)]
mod tests;

/// The cape geometry and its bone order, resolved once from the entity catalog.
#[derive(Clone)]
pub struct CapeRig {
    pub geometry: ActorRigGeometry,
    bone_names: Vec<Box<str>>,
    pose_frames: Vec<CapePoseFrame>,
}

#[derive(Clone)]
struct CapePoseFrame {
    parent: Option<Box<str>>,
    bind: Quat,
    pivot_offset: Vec3,
}

/// Resolves each cape bone's bind frame and shoulder offset.
fn pose_frames(bones: &[assets::EntityGeometryBone], pivots: &[[f32; 3]]) -> Vec<CapePoseFrame> {
    bones
        .iter()
        .enumerate()
        .map(|(index, bone)| {
            let [x, y, z] = bone.bind_pose_rotation.map_or([0.0; 3], |angles| {
                angles.map(|angle| angle.get().to_radians())
            });
            CapePoseFrame {
                parent: bone.parent.clone(),
                bind: Quat::from_euler(EulerRot::XYZEx, -x, -y, z),
                pivot_offset: Vec3::from_array(pivots[index])
                    - bone
                        .parent
                        .as_ref()
                        .and_then(|parent| {
                            bones
                                .iter()
                                .position(|bone| bone.name.eq_ignore_ascii_case(parent))
                        })
                        .map_or(Vec3::ZERO, |parent| Vec3::from_array(pivots[parent])),
            }
        })
        .collect()
}

impl CapeRig {
    /// Retains a resolved mesh with its matching authored bone order and bind frames.
    pub fn new(geometry: ActorRigGeometry, bones: &[assets::EntityGeometryBone]) -> Self {
        Self {
            pose_frames: pose_frames(bones, &geometry.bone_pivots),
            bone_names: bones.iter().map(|bone| bone.name.clone()).collect(),
            geometry,
        }
    }

    /// Resolves the cape mesh and attachment frames from the active entity catalog.
    pub fn resolve(assets: &RuntimeEntityAssets) -> Option<Self> {
        let index = find_geometry_index(assets, CAPE_GEOMETRY_IDENTIFIER)?;
        let id = equipment_rig_id(index);
        let bones = resolve_geometry_bones(assets, index as usize).ok()?;
        let geometry = entity_geometry(assets, index as usize, id).ok()?;
        Some(Self::new(geometry, &bones))
    }

    /// Retargets body animation onto the cape, optionally selecting the Java cape motion.
    /// `rest` reads the matching body's immutable rest transform in block coordinates.
    pub fn pose(
        &self,
        body_names: &[Box<str>],
        rest: impl Fn(usize) -> Option<RenderBoneTransform>,
        body: &[RenderBoneTransform],
        java: Option<&JavaCapeInput>,
    ) -> Arc<[RenderBoneTransform]> {
        match java {
            Some(input) => pose::java_cape_pose(self, body_names, &rest, body, input),
            None => pose::cape_pose(self, body_names, &rest, body),
        }
    }
}
