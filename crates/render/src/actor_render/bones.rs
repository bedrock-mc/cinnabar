//! Stable GPU endpoint ranges; CPU frame publications keep their dense layout.

use render_model::actor::{ACTOR_BONE_MATRIX_BYTES, MAX_ACTOR_POSE_BONES};
use render_model::{
    ActorDrawManifestEntry, ActorGpuInstance, ActorRigRenderFrame, ActorRigRoute, EntityRigId,
    MAX_ACTOR_RENDER_INSTANCES, MAX_RENDER_BONES_PER_ACTOR,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
};

type Matrix = [[f32; 4]; 3];
/// Shares the endpoint allocation on devices that can bind only four actor storage buffers.
pub(super) fn gpu_buffers(
    device: &bevy::render::renderer::RenderDevice,
) -> Option<(
    bevy::render::render_resource::Buffer,
    bevy::render::render_resource::Buffer,
    u64,
)> {
    use bevy::render::render_resource::{BufferDescriptor, BufferUsages};
    let compact = device.limits().max_storage_buffers_per_shader_stage
        < crate::device_settings::vertex_storage_buffers(
            &super::pipeline::actor_bind_group_layout(),
        );
    let endpoint_bytes = (MAX_ACTOR_POSE_BONES * ACTOR_BONE_MATRIX_BYTES) as u64;
    let allocate = |label, size| {
        crate::gpu_allocation::buffer(
            device,
            &BufferDescriptor {
                label: Some(label),
                size,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        )
    };
    let previous = allocate(
        "bounded shared actor previous-bone arena",
        endpoint_bytes * if compact { 2 } else { 1 },
    )?;
    let current = if compact {
        previous.clone()
    } else {
        allocate("bounded shared actor current-bone arena", endpoint_bytes)?
    };
    Some((previous, current, if compact { endpoint_bytes } else { 0 }))
}
const _: () = assert!(std::mem::size_of::<Matrix>() == ACTOR_BONE_MATRIX_BYTES);
type Key = (u64, i32, u64, u64, u8, EntityRigId);

/// One actor layer's allocation, independent of ingress, frame and draw order.
struct Slot {
    range: Range<usize>,
    reset: u64,
    route: ActorRigRoute,
}

/// A single bounded mirror of the two GPU endpoint buffers, with no frame history.
#[derive(Default)]
pub(super) struct BoneResidency {
    slots: BTreeMap<Key, Slot>,
    previous: Box<[Matrix]>,
    current: Box<[Matrix]>,
    geometry: Option<u64>,
}

/// Exact GPU instance/receipt projection and the endpoint ranges this preparation changed.
pub(super) struct BoneFrame {
    pub instances: Arc<[ActorGpuInstance]>,
    pub manifest: Arc<[ActorDrawManifestEntry]>,
    pub previous: Vec<Range<usize>>,
    pub current: Vec<Range<usize>>,
    #[cfg(any(test, feature = "tracy"))]
    pub compacted: bool,
}

impl BoneFrame {
    /// Counts endpoint bytes scheduled for upload, excluding the per-frame instance records.
    pub fn uploaded_bytes(&self) -> usize {
        self.previous
            .iter()
            .chain(&self.current)
            .map(|range| range.len())
            .sum::<usize>()
            * ACTOR_BONE_MATRIX_BYTES
    }
}

impl BoneResidency {
    /// Forgets all uploaded ownership while keeping only bounded allocation capacity.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.geometry = None;
    }

    /// Validates and prepares one complete publication; skipped frames need no delta replay.
    /// The caller must enqueue every returned write before installing or drawing this frame.
    pub fn prepare(
        &mut self,
        frame: &ActorRigRenderFrame,
        mut instances: Arc<[ActorGpuInstance]>,
    ) -> Option<BoneFrame> {
        let Some(keys) = validated_keys(frame).filter(|_| instances.len() == frame.instances.len())
        else {
            self.clear();
            return None;
        };
        if self.geometry != Some(frame.geometry_revision) {
            self.clear();
        }
        self.geometry = Some(frame.geometry_revision);
        self.slots.retain(|key, _| keys.contains(key));
        let mut assignments = Vec::with_capacity(frame.manifest.len());
        for entry in frame.manifest.iter() {
            let old = self.slots.get(&key(entry));
            let count = entry.bone_count as usize;
            let range = old
                .filter(|slot| slot.range.len() == count)
                .map(|slot| slot.range.clone());
            let dirty = old.is_none_or(|slot| {
                slot.reset != entry.reset_generation
                    || slot.route != entry.route
                    || slot.range.len() != count
            });
            assignments.push((range, dirty));
        }
        // Only this frame's survivors reserve space. Departed actors cannot cause admission loss.
        let occupied = assignments
            .iter()
            .filter_map(|(range, _)| range.clone())
            .collect();
        let mut holes = free_ranges(occupied, MAX_ACTOR_POSE_BONES);
        let mut compact = false;
        for (entry, (range, dirty)) in frame.manifest.iter().zip(&mut assignments) {
            if range.is_some() {
                continue;
            }
            let count = entry.bone_count as usize;
            if let Some(hole) = holes.iter_mut().find(|hole| hole.len() >= count) {
                *range = Some(hole.start..hole.start + count);
                hole.start += count;
                *dirty = true;
            } else {
                compact = true;
                break;
            }
        }
        if compact {
            let mut next = 0;
            for (entry, (range, dirty)) in frame.manifest.iter().zip(&mut assignments) {
                let end = next + entry.bone_count as usize;
                *range = Some(next..end);
                *dirty = true;
                next = end;
            }
        }
        let high = assignments
            .iter()
            .filter_map(|(range, _)| range.as_ref().map(|r| r.end))
            .max()
            .unwrap_or(0);
        grow(&mut self.previous, high);
        grow(&mut self.current, high);
        let mut previous = Vec::new();
        let mut current = Vec::new();
        let mut manifest = frame.manifest.to_vec();
        let projected = Arc::make_mut(&mut instances);
        for (index, (range, force)) in assignments.into_iter().enumerate() {
            let range = range.expect("validated live set fits after compaction");
            let entry = &mut manifest[index];
            let count = entry.bone_count as usize;
            update(
                &mut self.previous,
                &frame.previous_bones,
                entry.previous_bone_base as usize,
                range.clone(),
                force,
                &mut previous,
            );
            update(
                &mut self.current,
                &frame.current_bones,
                entry.current_bone_base as usize,
                range.clone(),
                force,
                &mut current,
            );
            self.slots.insert(
                key(entry),
                Slot {
                    range: range.clone(),
                    reset: entry.reset_generation,
                    route: entry.route,
                },
            );
            let base = range.start as u32;
            projected[index].previous_bone_base = base;
            projected[index].current_bone_base = base;
            entry.previous_bone_base = base;
            entry.current_bone_base = base;
            debug_assert_eq!(range.len(), count);
        }
        coalesce(&mut previous);
        coalesce(&mut current);
        Some(BoneFrame {
            instances,
            manifest: manifest.into(),
            previous,
            current,
            #[cfg(any(test, feature = "tracy"))]
            compacted: compact,
        })
    }

    /// Visits only changed ranges in byte offsets accepted by the existing GPU buffers.
    pub fn writes<'a>(&'a self, frame: &BoneFrame, mut write: impl FnMut(bool, u64, &'a [u8])) {
        for (is_current, ranges, mirror) in [
            (false, &frame.previous, &self.previous),
            (true, &frame.current, &self.current),
        ] {
            for range in ranges {
                write(
                    is_current,
                    (range.start * ACTOR_BONE_MATRIX_BYTES) as u64,
                    bytemuck::cast_slice(&mirror[range.clone()]),
                );
            }
        }
    }
}

/// Names one lifetime/layer without treating advancing pose or movement stamps as allocation IDs.
fn key(entry: &ActorDrawManifestEntry) -> Key {
    let id = entry.identity;
    (
        id.session_id,
        id.dimension,
        id.runtime_id,
        id.spawn_revision,
        id.layer,
        entry.rig,
    )
}

/// Rejects malformed ranges and duplicate owners before mutating the upload mirror.
fn validated_keys(frame: &ActorRigRenderFrame) -> Option<BTreeSet<Key>> {
    if frame.instances.is_empty()
        || frame.instances.len() > MAX_ACTOR_RENDER_INSTANCES
        || frame.manifest.len() != frame.instances.len()
        || frame.previous_bones.len() != frame.current_bones.len()
        || frame.previous_bones.len() > MAX_ACTOR_POSE_BONES
    {
        return None;
    }
    let mut keys = BTreeSet::new();
    let mut total = 0usize;
    for (index, (entry, instance)) in frame
        .manifest
        .iter()
        .zip(frame.instances.iter())
        .enumerate()
    {
        let count = entry.bone_count as usize;
        total = total.checked_add(count)?;
        let previous_end = (entry.previous_bone_base as usize).checked_add(count)?;
        let current_end = (entry.current_bone_base as usize).checked_add(count)?;
        if count == 0
            || count > MAX_RENDER_BONES_PER_ACTOR
            || total > MAX_ACTOR_POSE_BONES
            || entry.instance_index as usize != index
            || !keys.insert(key(entry))
            || entry.previous_bone_base != instance.previous_bone_base
            || entry.current_bone_base != instance.current_bone_base
            || previous_end > frame.previous_bones.len()
            || current_end > frame.current_bones.len()
            || !frame
                .geometry_spans
                .get(instance.geometry_id as usize)
                .is_some_and(|span| span.vertex_count != 0)
            || entry.route == ActorRigRoute::NoDraw
        {
            return None;
        }
    }
    Some(keys)
}

/// Returns ordered holes between disjoint retained ranges and the fixed arena end.
fn free_ranges(mut occupied: Vec<Range<usize>>, capacity: usize) -> Vec<Range<usize>> {
    occupied.sort_unstable_by_key(|range| range.start);
    let mut holes = Vec::with_capacity(occupied.len() + 1);
    let mut next = 0;
    for range in occupied {
        if next < range.start {
            holes.push(next..range.start);
        }
        next = range.end;
    }
    if next < capacity {
        holes.push(next..capacity);
    }
    holes
}

/// Grows one mirror without retaining spare capacity outside the fixed endpoint budget.
fn grow(mirror: &mut Box<[Matrix]>, high: usize) {
    if high <= mirror.len() {
        return;
    }
    let mut words = std::mem::take(mirror).into_vec();
    words.reserve_exact(high - words.len());
    words.resize(high, [[0.0; 4]; 3]);
    *mirror = words.into_boxed_slice();
}

/// Copies changed bytes, preserving negative zero and every other exact matrix bit.
fn update(
    mirror: &mut [Matrix],
    source: &[Matrix],
    base: usize,
    range: Range<usize>,
    force: bool,
    dirty: &mut Vec<Range<usize>>,
) {
    let source = &source[base..base + range.len()];
    let target = &mut mirror[range.clone()];
    if force || bytemuck::cast_slice::<_, u8>(target) != bytemuck::cast_slice::<_, u8>(source) {
        target.copy_from_slice(source);
        dirty.push(range);
    }
}

/// Combines adjacent changed allocations without uploading unchanged gaps.
fn coalesce(ranges: &mut Vec<Range<usize>>) {
    ranges.sort_unstable_by_key(|range| range.start);
    let mut used = 0;
    for read in 0..ranges.len() {
        if used != 0 && ranges[used - 1].end == ranges[read].start {
            ranges[used - 1].end = ranges[read].end;
        } else {
            ranges[used] = ranges[read].clone();
            used += 1;
        }
    }
    ranges.truncate(used);
}

#[cfg(test)]
mod tests;
