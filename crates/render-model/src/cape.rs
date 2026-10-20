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
    pub(crate) binding_key: [u8; 32],
    bone_names: Vec<Box<str>>,
    pose_frames: Vec<CapePoseFrame>,
}

#[derive(Clone)]
struct CapePoseFrame {
    is_cape: bool,
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
                is_cape: bone.name.eq_ignore_ascii_case("cape"),
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
            binding_key: crate::actor_bindings::cape_binding_key(bones),
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
        let bindings = self.bind(body_names);
        self.pose_bound(&bindings, rest, body, java)
    }

    /// Resolves a caller-owned appearance before its repeated frame samples.
    pub fn bind(&self, body_names: &[Box<str>]) -> Vec<crate::CapeBoneBinding> {
        self.bone_names
            .iter()
            .zip(&self.pose_frames)
            .map(|(name, frame)| crate::CapeBoneBinding {
                bone: body_names
                    .iter()
                    .position(|bone| bone.eq_ignore_ascii_case(name)),
                parent: frame.parent.as_deref().and_then(|parent| {
                    body_names
                        .iter()
                        .position(|bone| bone.eq_ignore_ascii_case(parent))
                }),
                needs_parent: frame.parent.is_some(),
            })
            .collect()
    }

    /// Retargets the cape using indices prepared for this exact cape and body skeleton.
    pub fn pose_bound(
        &self,
        bindings: &[crate::CapeBoneBinding],
        rest: impl Fn(usize) -> Option<RenderBoneTransform>,
        body: &[RenderBoneTransform],
        java: Option<&JavaCapeInput>,
    ) -> Arc<[RenderBoneTransform]> {
        match java {
            Some(input) => pose::java_cape_pose(self, bindings, &rest, body, input),
            None => pose::cape_pose(self, bindings, &rest, body),
        }
    }
}
