//! Previous rendered roots and interpolated bones, independent of instance ordering.

use super::history::CameraHistory;
use bevy::math::Mat4;
use render_model::actor::MAX_ACTOR_POSE_BONES;
use render_model::{ActorRenderIdentity, ActorRigRenderFrame, EntityRigId};
use std::collections::HashMap;

/// Storage layout shared with the actor velocity vertex shader.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MotionInstance {
    pub current_clip_from_actor: Mat4,
    pub previous_clip_from_actor: Mat4,
    /// Bone offset and history-valid flag, followed by uniform storage padding.
    pub pose: [u32; 4],
}

type Key = (u64, i32, u64, u64, u8, EntityRigId);
type BoneKey = (u32, u32, u32, u32);

/// Optional exposure keeps only one previous publication and reuses bounded scratch vectors.
#[derive(Default)]
pub(crate) struct ActorHistory {
    previous: ActorRigRenderFrame,
    indices: HashMap<Key, usize>,
    sampled: HashMap<BoneKey, u32>,
    pub instances: Vec<MotionInstance>,
    pub bones: Vec<[[f32; 4]; 3]>,
}

/// Lifetime and layer remain stable while ingress, movement and pose revisions advance.
fn key(identity: ActorRenderIdentity, rig: EntityRigId) -> Key {
    (
        identity.session_id,
        identity.dimension,
        identity.runtime_id,
        identity.spawn_revision.get(),
        identity.layer,
        rig,
    )
}

impl ActorHistory {
    /// Publishes the previous rendered pose in current instance order and retains `current`.
    /// Cuts, new lifetimes, changed rigs and reset generations produce zero object velocity.
    pub fn advance(
        &mut self,
        current: &ActorRigRenderFrame,
        previous_camera: CameraHistory,
        current_camera: CameraHistory,
        continuous: bool,
    ) -> bool {
        self.instances.clear();
        self.bones.clear();
        self.sampled.clear();
        if current.instances.len() != current.manifest.len() {
            self.indices.clear();
            self.previous = ActorRigRenderFrame::default();
            return false;
        }
        let projection = previous_camera.actor_projection();
        let current_projection = current_camera.actor_projection();
        let mut moved = false;
        for (instance, entry) in current.instances.iter().zip(current.manifest.iter()) {
            let old = continuous
                .then(|| self.indices.get(&key(entry.identity, entry.rig)).copied())
                .flatten()
                .filter(|&index| {
                    let old = &self.previous.manifest[index];
                    old.reset_generation == entry.reset_generation
                        && old.route == entry.route
                        && old.bone_count == entry.bone_count
                        && self.previous.geometry_revision == current.geometry_revision
                });
            let mut motion = MotionInstance {
                current_clip_from_actor: current_projection
                    .clip_from_actor(instance.world_from_actor),
                previous_clip_from_actor: Mat4::IDENTITY,
                pose: [0; 4],
            };
            if let Some(index) = old {
                let old = &self.previous.instances[index];
                let count = entry.bone_count as usize;
                let a = self
                    .previous
                    .previous_bones
                    .get(old.previous_bone_base as usize..)
                    .and_then(|bones| bones.get(..count));
                let b = self
                    .previous
                    .current_bones
                    .get(old.current_bone_base as usize..)
                    .and_then(|bones| bones.get(..count));
                if let (Some(a), Some(b)) = (a, b) {
                    let t = old.partial_tick.clamp(0.0, 1.0);
                    let bone_key = (
                        old.previous_bone_base,
                        old.current_bone_base,
                        t.to_bits(),
                        entry.bone_count,
                    );
                    let base = if let Some(&base) = self.sampled.get(&bone_key) {
                        Some(base)
                    } else if self.bones.len() + count <= MAX_ACTOR_POSE_BONES {
                        let base = self.bones.len() as u32;
                        for (a, b) in a.iter().zip(b) {
                            self.bones.push(std::array::from_fn(|row| {
                                std::array::from_fn(|col| {
                                    a[row][col] + (b[row][col] - a[row][col]) * t
                                })
                            }));
                        }
                        self.sampled.insert(bone_key, base);
                        Some(base)
                    } else {
                        None
                    };
                    if let Some(base) = base {
                        motion.previous_clip_from_actor =
                            projection.clip_from_actor(old.world_from_actor);
                        motion.pose = [
                            base,
                            u32::from(
                                motion.current_clip_from_actor.is_finite()
                                    && motion.previous_clip_from_actor.is_finite(),
                            ),
                            0,
                            0,
                        ];
                        moved |= old.world_from_actor != instance.world_from_actor;
                        let a = current
                            .previous_bones
                            .get(instance.previous_bone_base as usize..)
                            .and_then(|bones| bones.get(..count));
                        let b = current
                            .current_bones
                            .get(instance.current_bone_base as usize..)
                            .and_then(|bones| bones.get(..count));
                        if let (Some(a), Some(b)) = (a, b) {
                            let t = instance.partial_tick.clamp(0.0, 1.0);
                            for (bone, (a, b)) in a.iter().zip(b).enumerate() {
                                let current = std::array::from_fn::<_, 3, _>(|row| {
                                    std::array::from_fn::<_, 4, _>(|col| {
                                        a[row][col] + (b[row][col] - a[row][col]) * t
                                    })
                                });
                                moved |= current != self.bones[base as usize + bone];
                            }
                        }
                    }
                }
            }
            self.instances.push(motion);
        }
        self.indices.clear();
        for (index, entry) in current.manifest.iter().enumerate() {
            self.indices.insert(key(entry.identity, entry.rig), index);
        }
        self.previous = current.clone();
        moved
    }
}

#[cfg(test)]
pub(crate) mod tests;
