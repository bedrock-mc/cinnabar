//! Bone matrices appended directly to the frame arena without per-actor temporary buffers.

use std::{collections::HashMap, sync::Arc};

use super::{EntityRigId, RenderBoneTransform, affine_matrix};

/// One drawn pose's matrices for one geometry.
#[derive(Debug)]
struct CachedMatrices {
    /// Held so the pose's address names this pose while the entry lives.
    pose: Arc<[RenderBoneTransform]>,
    matrices: Vec<[[f32; 4]; 3]>,
    /// The frame that last drew the pose.
    frame: u64,
    pivots: Arc<[[f32; 3]]>,
}

impl CachedMatrices {
    /// Whether these are `pose`'s matrices about `pivots`.
    fn matches(&self, pose: &Arc<[RenderBoneTransform]>, pivots: &Arc<[[f32; 3]]>) -> bool {
        Arc::ptr_eq(&self.pose, pose)
            && (Arc::ptr_eq(&self.pivots, pivots) || *self.pivots == **pivots)
    }
}

/// Bone matrices of recently drawn poses keyed by pose allocation and geometry: every frame of
/// a tick shares a pose, so its matrices are computed once.
#[derive(Debug, Default)]
pub(super) struct PoseMatrixCache {
    entries: HashMap<(usize, EntityRigId), CachedMatrices>,
    /// Matrix buffers of released entries, reused by the next poses computed.
    spare: Vec<Vec<[[f32; 4]; 3]>>,
    frame: u64,
}

impl PoseMatrixCache {
    /// Releases poses no frame drew since the previous one, keeping their buffers for reuse.
    pub(super) fn begin_frame(&mut self) {
        self.frame += 1;
        let oldest = self.frame.saturating_sub(1);
        let spare = &mut self.spare;
        spare.clear();
        self.entries.retain(|_, entry| {
            let keep = entry.frame >= oldest;
            if !keep {
                spare.push(std::mem::take(&mut entry.matrices));
            }
            keep
        });
    }

    /// Validates a pose without changing cache ownership or frame counters.
    pub(super) fn pose_is_valid(
        &self,
        pose: &Arc<[RenderBoneTransform]>,
        geometry: EntityRigId,
        pivots: &Arc<[[f32; 3]]>,
    ) -> bool {
        let key = (Arc::as_ptr(pose).cast::<u8>() as usize, geometry);
        if let Some(entry) = self.entries.get(&key)
            && entry.matches(pose, pivots)
        {
            return true;
        }
        pose.iter().enumerate().all(|(index, transform)| {
            affine_matrix(*transform, pivots.get(index).copied().unwrap_or([0.0; 3])).is_some()
        })
    }

    /// [`append_pose_matrices`] through the cache.
    pub(super) fn append(
        &mut self,
        arena: &mut Vec<[[f32; 4]; 3]>,
        pose: &Arc<[RenderBoneTransform]>,
        geometry: EntityRigId,
        pivots: &Arc<[[f32; 3]]>,
    ) -> bool {
        let key = (Arc::as_ptr(pose).cast::<u8>() as usize, geometry);
        if let Some(entry) = self.entries.get_mut(&key)
            && entry.matches(pose, pivots)
        {
            entry.frame = self.frame;
            arena.extend_from_slice(&entry.matrices);
            return true;
        }
        let start = arena.len();
        if !append_pose_matrices(arena, pose, pivots) {
            return false;
        }
        let mut matrices = self.spare.pop().unwrap_or_default();
        matrices.clear();
        matrices.extend_from_slice(&arena[start..]);
        let replaced = self.entries.insert(
            key,
            CachedMatrices {
                pose: Arc::clone(pose),
                matrices,
                frame: self.frame,
                pivots: Arc::clone(pivots),
            },
        );
        if let Some(replaced) = replaced {
            self.spare.push(replaced.matrices);
        }
        true
    }
}

/// Appends one pose to its final arena; an invalid bone restores the original length.
pub(super) fn append_pose_matrices(
    arena: &mut Vec<[[f32; 4]; 3]>,
    transforms: &[RenderBoneTransform],
    pivots: &[[f32; 3]],
) -> bool {
    let start = arena.len();
    arena.reserve(transforms.len());
    for (index, transform) in transforms.iter().enumerate() {
        let Some(matrix) =
            affine_matrix(*transform, pivots.get(index).copied().unwrap_or([0.0; 3]))
        else {
            arena.truncate(start);
            return false;
        };
        arena.push(matrix);
    }
    true
}

#[cfg(test)]
#[path = "bone_arena/tests.rs"]
mod tests;
