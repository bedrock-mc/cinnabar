//! Bone matrices appended directly to the frame arena without per-actor temporary buffers.

use std::{collections::HashMap, sync::Arc};

use super::{EntityRigId, RenderBoneTransform, affine_matrix};

type CachedMatrices = (
    Arc<[RenderBoneTransform]>,
    Vec<[[f32; 4]; 3]>,
    u64,
    Vec<[f32; 3]>,
);

/// Bone matrices of recently drawn poses keyed by pose allocation and geometry: every frame of
/// a tick shares a pose, so its matrices are computed once.
#[derive(Debug, Default)]
pub(super) struct PoseMatrixCache {
    /// Pose, its matrices and the frame it was last drawn.
    entries: HashMap<(usize, EntityRigId), CachedMatrices>,
    frame: u64,
}

impl PoseMatrixCache {
    /// Releases poses no frame drew since the previous one.
    pub(super) fn begin_frame(&mut self) {
        self.frame += 1;
        let oldest = self.frame.saturating_sub(1);
        self.entries.retain(|_, entry| entry.2 >= oldest);
    }

    /// [`append_pose_matrices`] through the cache.
    pub(super) fn append(
        &mut self,
        arena: &mut Vec<[[f32; 4]; 3]>,
        pose: &Arc<[RenderBoneTransform]>,
        geometry: EntityRigId,
        pivots: &[[f32; 3]],
    ) -> bool {
        let key = (Arc::as_ptr(pose).cast::<u8>() as usize, geometry);
        if let Some(entry) = self.entries.get_mut(&key)
            && Arc::ptr_eq(&entry.0, pose)
            && entry.3.as_slice() == pivots
        {
            entry.2 = self.frame;
            arena.extend_from_slice(&entry.1);
            return true;
        }
        let start = arena.len();
        if !append_pose_matrices(arena, pose, pivots) {
            return false;
        }
        self.entries.insert(
            key,
            (
                Arc::clone(pose),
                arena[start..].to_vec(),
                self.frame,
                pivots.to_vec(),
            ),
        );
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
