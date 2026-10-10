//! Last submitted actor poses, remapped by lifetime rather than instance-arena position.

use std::collections::HashMap;

use crate::actor::{ActorDrawManifestEntry, ActorRigRenderFrame};

type BoneMatrix = [[f32; 4]; 3];
type Lifetime = (u64, i32, u64, u64, u8);

/// Matches the four vec4 rows of `MotionInstance` in the actor motion vertex path.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ActorMotionInstanceGpu {
    pub(crate) world_from_actor: BoneMatrix,
    /// Bone base, bone count, usable submitted pose, and unused padding.
    pub(crate) metadata: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<ActorMotionInstanceGpu>() == 64);

#[cfg(test)]
#[path = "actor_motion_gpu_tests.rs"]
mod gpu_tests;

#[derive(Default)]
pub(crate) struct ActorMotionHistory {
    submitted: Option<ActorRigRenderFrame>,
    pending: Option<ActorRigRenderFrame>,
    submitted_indices: HashMap<Lifetime, usize>,
    prepared_key: Option<(u64, Option<u64>)>,
    instances: Vec<ActorMotionInstanceGpu>,
    bones: Vec<BoneMatrix>,
}

/// Retains exactly the bytes uploaded so generation-only changes do not write GPU storage again.
#[derive(Default)]
pub(crate) struct ActorMotionUploadCache {
    instances: Vec<ActorMotionInstanceGpu>,
    bones: Vec<BoneMatrix>,
}

impl ActorMotionUploadCache {
    pub(crate) fn update(
        &mut self,
        instances: &[ActorMotionInstanceGpu],
        bones: &[BoneMatrix],
    ) -> [bool; 2] {
        let instances_changed = bytemuck::cast_slice::<_, u8>(&self.instances)
            != bytemuck::cast_slice::<_, u8>(instances);
        let bones_changed =
            bytemuck::cast_slice::<_, u8>(&self.bones) != bytemuck::cast_slice::<_, u8>(bones);
        if instances_changed {
            self.instances.clear();
            self.instances.extend_from_slice(instances);
        }
        if bones_changed {
            self.bones.clear();
            self.bones.extend_from_slice(bones);
        }
        [instances_changed, bones_changed]
    }
}

fn lifetime(entry: &ActorDrawManifestEntry) -> Lifetime {
    let id = entry.identity;
    (
        id.session_id,
        id.dimension,
        id.runtime_id,
        id.spawn_revision,
        id.layer,
    )
}

fn posed_bone(
    frame: &ActorRigRenderFrame,
    instance_index: usize,
    bone: usize,
) -> Option<BoneMatrix> {
    let instance = frame.instances.get(instance_index)?;
    let old = frame
        .previous_bones
        .get(instance.previous_bone_base as usize + bone)?;
    let new = frame
        .current_bones
        .get(instance.current_bone_base as usize + bone)?;
    let fraction = instance.partial_tick.clamp(0.0, 1.0);
    Some(std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            old[row][column] * (1.0 - fraction) + new[row][column] * fraction
        })
    }))
}

impl ActorMotionHistory {
    /// Returns whether aligned motion arenas were refreshed; identical submitted inputs reuse them.
    pub(crate) fn prepare(&mut self, frame: &ActorRigRenderFrame) -> bool {
        self.pending = Some(frame.clone());
        let key = (
            frame.frame_generation,
            self.submitted.as_ref().map(|old| old.frame_generation),
        );
        if self.prepared_key == Some(key) {
            return false;
        }
        self.instances.clear();
        self.bones.clear();
        for (index, instance) in frame.instances.iter().enumerate() {
            let current_entry = frame.manifest.get(index);
            let old = current_entry
                .and_then(|entry| self.submitted_indices.get(&lifetime(entry)).copied())
                .and_then(|old_index| {
                    let old_frame = self.submitted.as_ref()?;
                    let old_entry = old_frame.manifest.get(old_index)?;
                    let current_entry = current_entry?;
                    let old_instance = old_frame.instances.get(old_index)?;
                    let moved_squared: f32 = (0..3)
                        .map(|axis| {
                            (old_instance.world_from_actor[axis][3]
                                - instance.world_from_actor[axis][3])
                                .powi(2)
                        })
                        .sum();
                    (old_frame.geometry_revision == frame.geometry_revision
                        && old_entry.rig == current_entry.rig
                        && old_entry.reset_generation == current_entry.reset_generation
                        && old_entry.bone_count == current_entry.bone_count
                        && old_instance.uv_anim == instance.uv_anim
                        && moved_squared.is_finite()
                        && moved_squared < 16.0)
                        .then_some((old_frame, old_index, old_instance))
                });
            let base = self.bones.len() as u32;
            let count = current_entry.map_or(0, |entry| entry.bone_count);
            let mut usable = old.is_some() && count > 0;
            let transform = old.map_or(instance.world_from_actor, |(_, _, old)| {
                old.world_from_actor
            });
            for bone in 0..count as usize {
                let posed =
                    old.and_then(|(frame, old_index, _)| posed_bone(frame, old_index, bone));
                usable &= posed.is_some();
                self.bones.push(posed.unwrap_or_default());
            }
            self.instances.push(ActorMotionInstanceGpu {
                world_from_actor: transform,
                metadata: [base, count, u32::from(usable), 0],
            });
        }
        self.prepared_key = Some(key);
        true
    }

    pub(crate) fn instances(&self) -> &[ActorMotionInstanceGpu] {
        &self.instances
    }

    pub(crate) fn bones(&self) -> &[BoneMatrix] {
        &self.bones
    }

    /// Call only after the owning camera submitted its actor coverage and temporal resolve.
    pub(crate) fn commit_submitted(&mut self, rendered: impl Fn(usize) -> bool) {
        let Some(frame) = self.pending.take() else {
            return;
        };
        let visible_count = frame
            .manifest
            .iter()
            .enumerate()
            .filter(|(index, _)| rendered(*index))
            .count();
        let visibility_changed = visible_count != self.submitted_indices.len()
            || frame.manifest.iter().enumerate().any(|(index, entry)| {
                rendered(index)
                    && self.submitted_indices.get(&lifetime(entry)).copied() != Some(index)
            });
        if visibility_changed {
            self.prepared_key = None;
        }
        self.submitted_indices.clear();
        self.submitted_indices.extend(
            frame
                .manifest
                .iter()
                .enumerate()
                .filter(|(index, _)| rendered(*index))
                .map(|(index, entry)| (lifetime(entry), index)),
        );
        self.submitted = Some(frame);
    }

    pub(crate) fn clear(&mut self) {
        self.submitted = None;
        self.pending = None;
        self.submitted_indices.clear();
        self.prepared_key = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::{ActorGpuInstance, ActorRenderIdentity, ActorRigRoute, IDENTITY_UV_ANIM};
    use render_model::EntityRigId;
    use std::sync::Arc;

    fn frame(generation: u64, runtime_ids: &[u64], x: f32, fraction: f32) -> ActorRigRenderFrame {
        let bone = |x| {
            [
                [1.0, 0.0, 0.0, x],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ]
        };
        ActorRigRenderFrame {
            frame_generation: generation,
            geometry_revision: 1,
            instances: runtime_ids
                .iter()
                .enumerate()
                .map(|(index, _)| ActorGpuInstance {
                    world_from_actor: bone(x + index as f32 * 10.0),
                    previous_bone_base: index as u32,
                    current_bone_base: index as u32,
                    partial_tick: fraction,
                    uv_anim: IDENTITY_UV_ANIM,
                    ..Default::default()
                })
                .collect::<Vec<_>>()
                .into(),
            previous_bones: vec![bone(0.0); runtime_ids.len()].into(),
            current_bones: vec![bone(4.0); runtime_ids.len()].into(),
            manifest: runtime_ids
                .iter()
                .enumerate()
                .map(|(index, runtime_id)| ActorDrawManifestEntry {
                    identity: ActorRenderIdentity {
                        session_id: 1,
                        dimension: 0,
                        runtime_id: *runtime_id,
                        spawn_revision: 1,
                        ingress_sequence: 1,
                        source_tick: Some(1),
                        movement_revision: generation,
                        pose_generation: generation,
                        layer: 0,
                    },
                    rig: EntityRigId(1),
                    completed_tick: generation,
                    reset_generation: 1,
                    route: ActorRigRoute::Compiled,
                    instance_index: index as u32,
                    previous_bone_base: index as u32,
                    current_bone_base: index as u32,
                    bone_count: 1,
                })
                .collect::<Vec<_>>()
                .into(),
            ..Default::default()
        }
    }

    #[test]
    fn history_uses_submitted_interpolation_instead_of_the_previous_tick_pose() {
        let mut history = ActorMotionHistory::default();
        let old = frame(1, &[1], 0.0, 0.75);
        history.prepare(&old);
        assert_eq!(history.instances()[0].metadata[2], 0);
        history.commit_submitted(|_| true);
        let current = frame(2, &[1], 1.0, 0.5);
        history.prepare(&current);
        assert_eq!(history.instances()[0].world_from_actor[0][3], 0.0);
        assert_eq!(history.bones()[0][0][3], 3.0);
        assert_eq!(history.instances()[0].metadata[2], 1);
    }

    #[test]
    fn unsubmitted_prepared_frames_cannot_advance_actor_history() {
        let mut history = ActorMotionHistory::default();
        history.prepare(&frame(1, &[1], 0.0, 0.25));
        history.commit_submitted(|_| true);
        history.prepare(&frame(2, &[1], 1.0, 0.5));
        history.prepare(&frame(3, &[1], 2.0, 0.75));
        assert_eq!(history.bones()[0][0][3], 1.0);
        assert_eq!(history.instances()[0].world_from_actor[0][3], 0.0);
    }

    #[test]
    fn arena_reordering_tracks_actor_lifetime_and_resets_respawns_and_teleports() {
        let mut history = ActorMotionHistory::default();
        history.prepare(&frame(1, &[1, 2], 0.0, 0.25));
        history.commit_submitted(|_| true);
        let mut reordered = frame(2, &[2, 1], 0.0, 0.5);
        let instances = Arc::make_mut(&mut reordered.instances);
        instances[0].world_from_actor[0][3] = 11.0;
        instances[1].world_from_actor[0][3] = 1.0;
        history.prepare(&reordered);
        assert_eq!(history.instances()[0].world_from_actor[0][3], 10.0);
        assert_eq!(history.instances()[1].world_from_actor[0][3], 0.0);
        assert!(
            history
                .instances()
                .iter()
                .all(|instance| instance.metadata[2] == 1)
        );
        let mut respawn = frame(3, &[1], 0.0, 0.5);
        Arc::make_mut(&mut respawn.manifest)[0]
            .identity
            .spawn_revision = 2;
        history.prepare(&respawn);
        assert_eq!(history.instances()[0].metadata[2], 0);
        history.prepare(&frame(4, &[1], 8.0, 0.5));
        assert_eq!(history.instances()[0].metadata[2], 0);
    }

    #[test]
    fn identical_inputs_reuse_arenas_and_submission_rebases_stationary_motion() {
        let mut history = ActorMotionHistory::default();
        let frame = frame(1, &[1], 0.0, 0.25);
        assert!(history.prepare(&frame));
        let capacity = (history.instances.capacity(), history.bones.capacity());
        assert!(!history.prepare(&frame));
        history.commit_submitted(|_| true);
        assert!(history.prepare(&frame));
        assert_eq!(history.instances()[0].metadata[2], 1);
        assert_eq!(
            (history.instances.capacity(), history.bones.capacity()),
            capacity
        );
        assert!(!history.prepare(&frame));
        history.clear();
        assert!(history.prepare(&frame));
        assert_eq!(history.instances()[0].metadata[2], 0);
    }

    #[test]
    fn a_missing_artwork_draw_cannot_publish_an_actor_pose_from_another_draw_span() {
        let mut history = ActorMotionHistory::default();
        let first = frame(1, &[1, 2], 0.0, 0.25);
        history.prepare(&first);
        history.commit_submitted(|index| index == 0);
        let second = frame(2, &[1, 2], 1.0, 0.5);
        history.prepare(&second);
        assert_eq!(history.instances()[0].metadata[2], 1);
        assert_eq!(history.instances()[1].metadata[2], 0);
        history.commit_submitted(|_| true);
        history.prepare(&frame(3, &[1, 2], 2.0, 0.75));
        assert!(
            history
                .instances()
                .iter()
                .all(|instance| instance.metadata[2] == 1)
        );
    }

    #[test]
    fn unchanged_generation_still_reconciles_which_actor_spans_were_drawn() {
        let mut history = ActorMotionHistory::default();
        let stationary = frame(1, &[1, 2], 0.0, 0.25);
        history.prepare(&stationary);
        history.commit_submitted(|_| true);
        history.prepare(&stationary);
        history.commit_submitted(|_| true);
        assert!(!history.prepare(&stationary));
        history.commit_submitted(|index| index == 0);
        assert!(history.prepare(&stationary));
        assert_eq!(history.instances()[0].metadata[2], 1);
        assert_eq!(history.instances()[1].metadata[2], 0);
    }

    #[test]
    fn generation_only_changes_reuse_motion_upload_bytes_and_arenas() {
        let mut history = ActorMotionHistory::default();
        let mut uploads = ActorMotionUploadCache::default();
        let first = frame(1, &[1], 0.0, 0.25);
        history.prepare(&first);
        uploads.update(history.instances(), history.bones());
        history.commit_submitted(|_| true);
        history.prepare(&first);
        assert_eq!(
            uploads.update(history.instances(), history.bones()),
            [true, true]
        );
        let capacity = (uploads.instances.capacity(), uploads.bones.capacity());
        history.commit_submitted(|_| true);
        history.prepare(&frame(2, &[1], 0.0, 0.25));
        assert_eq!(
            uploads.update(history.instances(), history.bones()),
            [false, false]
        );
        assert_eq!(
            (uploads.instances.capacity(), uploads.bones.capacity()),
            capacity
        );
        history.commit_submitted(|_| true);
        history.prepare(&frame(3, &[1], 1.0, 0.75));
        assert_eq!(
            uploads.update(history.instances(), history.bones()),
            [false, false]
        );
        history.commit_submitted(|_| true);
        history.prepare(&frame(4, &[1], 1.0, 0.75));
        assert_eq!(
            uploads.update(history.instances(), history.bones()),
            [true, true]
        );
        let instances = history.instances().to_vec();
        let mut bones = history.bones().to_vec();
        bones[0][0][3] += 1.0;
        assert_eq!(uploads.update(&instances, &bones), [false, true]);
    }
}
