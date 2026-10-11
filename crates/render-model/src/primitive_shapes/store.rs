//! Network identities retain stable GPU slots; frames never traverse the shape map.

use super::{
    PrimitiveActor, PrimitiveInstance, PrimitiveMeshKey, PrimitiveSlots, PrimitiveState,
    actors::Attachments,
};
use crate::NametagAtlasRect;
use render_api::primitive_shapes::{
    PrimitiveShapeChange, PrimitiveShapeUpdate, PrimitiveShapesEvent, PrimitiveText,
};
use std::{collections::HashMap, sync::Arc};

/// One atlas quad referring to a retained text shape, so movement never rebuilds text.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PrimitiveTextRecord {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    /// Shape slot, alive, text option flags, and multiline world lift encoded as f32 bits.
    pub meta: [u32; 4],
}

/// Text contents changed independently of its position, colour, lifetime and attachment.
#[derive(Debug)]
pub struct PrimitiveTextChange {
    pub network_id: u64,
    pub slot: u32,
    pub text: PrimitiveText,
}

/// All instances using one shared unit mesh, including reusable hidden holes.
#[derive(Debug)]
pub struct PrimitiveBatch {
    pub key: PrimitiveMeshKey,
    pub instances: PrimitiveSlots<PrimitiveInstance>,
    queued: bool,
}

/// Deterministic work performed by one instance publication.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimitiveUploadStats {
    pub slots: usize,
    pub bytes: usize,
    pub ranges: usize,
}

#[derive(Debug)]
struct Entry {
    state: PrimitiveState,
    batch: usize,
    slot: u32,
    actor_slot: u32,
    text_slots: Vec<u32>,
    text_queued: bool,
    text_dynamic: bool,
}

/// Owns packet state and dirty lists; GPU time and dimension checks need no frame-time scan.
#[derive(Debug, Default)]
pub struct PrimitiveShapeStore {
    entries: HashMap<u64, Entry>,
    pub batches: Vec<PrimitiveBatch>,
    batch_indices: HashMap<PrimitiveMeshKey, usize>,
    dirty_batches: Vec<usize>,
    attachments: Attachments,
    pub actors: PrimitiveSlots<PrimitiveActor>,
    pub text_records: PrimitiveSlots<PrimitiveTextRecord>,
    pub atlas: Arc<[NametagAtlasRect]>,
    text_changes: Vec<u64>,
    pub skipped_entries: u64,
    pub instance_rebuilds: u64,
}

impl PrimitiveShapeStore {
    /// Counts retained network identities without considering GPU visibility.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Indicates that a session has no retained network identities.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Reads one authoritative retained shape for diagnostics and deterministic tests.
    pub fn get(&self, id: u64) -> Option<&PrimitiveState> {
        self.entries.get(&id).map(|entry| &entry.state)
    }

    /// Returns stable instance pools without copying their records.
    pub fn batches(&self) -> &[PrimitiveBatch] {
        &self.batches
    }

    /// Detects pending uploads without visiting shapes or allocating scratch storage.
    pub fn has_changes(&self) -> bool {
        !self.dirty_batches.is_empty() || !self.actors.is_clean() || !self.text_records.is_clean()
    }

    /// Applies server creation, patch and removal entries in their network order.
    pub fn apply(&mut self, event: PrimitiveShapesEvent) {
        self.skipped_entries += u64::from(event.skipped_entries);
        for change in event.changes {
            match change {
                PrimitiveShapeChange::Remove { network_id } => {
                    self.remove(network_id);
                }
                PrimitiveShapeChange::Upsert(update) => self.upsert(update),
            }
        }
    }

    /// Samples the compact attachment list, never the list of shapes.
    pub fn update_actors(&mut self, position: impl FnMut(i64) -> Option<[f32; 3]>) {
        self.attachments.update(&mut self.actors, position);
    }

    /// Drains coalesced writes directly into the GPU queue, retaining all staging allocations.
    pub fn drain_uploads(
        &mut self,
        mut upload: impl FnMut(usize, PrimitiveMeshKey, u32, &[PrimitiveInstance]),
    ) -> PrimitiveUploadStats {
        let mut stats = PrimitiveUploadStats::default();
        for index in self.dirty_batches.drain(..) {
            let batch = &mut self.batches[index];
            batch.instances.drain(|start, values| {
                stats.slots += values.len();
                stats.bytes += std::mem::size_of_val(values);
                stats.ranges += 1;
                upload(index, batch.key, start, values);
            });
            batch.queued = false;
        }
        stats
    }

    /// Requeues retained text only when an atlas or font change invalidates its cells.
    pub fn queue_all_text(&mut self) {
        for (&id, entry) in &mut self.entries {
            if entry.state.text.is_some() && !entry.text_queued {
                entry.text_queued = true;
                self.text_changes.push(id);
            }
        }
    }

    /// Returns only changed text payloads; an unchanged frame returns an unallocated empty vector.
    pub fn take_text_changes(&mut self) -> Vec<PrimitiveTextChange> {
        let mut changes = Vec::with_capacity(self.text_changes.len());
        for id in self.text_changes.drain(..) {
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            entry.text_queued = false;
            if let Some(text) = &entry.state.text {
                changes.push(PrimitiveTextChange {
                    network_id: id,
                    slot: entry.slot,
                    text: text.clone(),
                });
            }
        }
        changes
    }

    /// Marks parsed text objects for reevaluation when a packet changes their shape.
    pub fn set_text_dynamic(&mut self, id: u64, dynamic: bool) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.text_dynamic = dynamic;
        }
    }

    /// Reuses a text shape's quad slots when its line count stays the same.
    pub fn set_text_records(&mut self, id: u64, records: Vec<PrimitiveTextRecord>) {
        let Some(entry) = self.entries.get_mut(&id) else {
            return;
        };
        while entry.text_slots.len() > records.len() {
            let slot = entry.text_slots.pop().expect("text slot exists");
            self.text_records.set(slot, PrimitiveTextRecord::default());
            self.text_records.release(slot);
        }
        for (index, mut record) in records.into_iter().enumerate() {
            record.meta[0] = entry.slot;
            record.meta[1] = 1;
            if let Some(&slot) = entry.text_slots.get(index) {
                self.text_records.set(slot, record);
            } else {
                entry.text_slots.push(self.text_records.insert(record));
            }
        }
    }

    /// Creates a shared mesh pool once, without tying instance count to draw count.
    fn batch_index(
        batches: &mut Vec<PrimitiveBatch>,
        indices: &mut HashMap<PrimitiveMeshKey, usize>,
        key: PrimitiveMeshKey,
    ) -> usize {
        if let Some(&index) = indices.get(&key) {
            return index;
        }
        let index = batches.len();
        batches.push(PrimitiveBatch {
            key,
            instances: PrimitiveSlots::default(),
            queued: false,
        });
        indices.insert(key, index);
        index
    }

    /// Schedules each modified pool once, even when several packets touch it before extraction.
    fn queue_batch(&mut self, index: usize) {
        if !self.batches[index].queued && !self.batches[index].instances.is_clean() {
            self.batches[index].queued = true;
            self.dirty_batches.push(index);
        }
    }

    /// Retains the existing concrete type and rewrites only its changed instance slot.
    fn upsert(&mut self, update: PrimitiveShapeUpdate) {
        let id = update.network_id;
        let mut retired_batch = None;
        let batch;
        if let Some(entry) = self.entries.get_mut(&id) {
            let previous = entry.state.clone();
            entry.state.patch(update);
            if (entry.text_dynamic || previous.text != entry.state.text) && !entry.text_queued {
                entry.text_queued = true;
                self.text_changes.push(id);
            }
            if previous == entry.state {
                return;
            }
            if previous.attached_actor != entry.state.attached_actor {
                if let Some(actor) = previous.attached_actor {
                    self.attachments.release(actor, &mut self.actors);
                }
                entry.actor_slot = entry.state.attached_actor.map_or(u32::MAX, |actor| {
                    self.attachments.acquire(actor, &mut self.actors)
                });
            }
            batch = Self::batch_index(
                &mut self.batches,
                &mut self.batch_indices,
                entry.state.mesh_key(),
            );
            let instance = entry.state.instance(entry.actor_slot);
            if entry.batch == batch {
                self.batches[batch].instances.set(entry.slot, instance);
            } else {
                let old = &mut self.batches[entry.batch];
                old.instances.set(entry.slot, PrimitiveInstance::default());
                old.instances.release(entry.slot);
                retired_batch = Some(entry.batch);
                entry.slot = self.batches[batch].instances.insert(instance);
                entry.batch = batch;
            }
        } else {
            let mut state = PrimitiveState::new(update.kind);
            state.patch(update);
            batch = Self::batch_index(&mut self.batches, &mut self.batch_indices, state.mesh_key());
            let actor_slot = state.attached_actor.map_or(u32::MAX, |actor| {
                self.attachments.acquire(actor, &mut self.actors)
            });
            let slot = self.batches[batch]
                .instances
                .insert(state.instance(actor_slot));
            let text_queued = state.text.is_some();
            if text_queued {
                self.text_changes.push(id);
            }
            self.entries.insert(
                id,
                Entry {
                    state,
                    batch,
                    slot,
                    actor_slot,
                    text_slots: Vec::new(),
                    text_queued,
                    text_dynamic: false,
                },
            );
        }
        self.instance_rebuilds += 1;
        if let Some(batch) = retired_batch {
            self.queue_batch(batch);
        }
        self.queue_batch(batch);
    }

    /// Hides explicit removals immediately while keeping unrelated instance indices stable.
    fn hide_slot(&mut self, batch: usize, slot: u32) {
        self.batches[batch]
            .instances
            .set(slot, PrimitiveInstance::default());
        self.batches[batch].instances.release(slot);
        self.queue_batch(batch);
    }

    /// Server removals release identity, text and attachment ownership together.
    fn remove(&mut self, id: u64) {
        let Some(entry) = self.entries.remove(&id) else {
            return;
        };
        self.hide_slot(entry.batch, entry.slot);
        if let Some(actor) = entry.state.attached_actor {
            self.attachments.release(actor, &mut self.actors);
        }
        for slot in entry.text_slots {
            self.text_records.set(slot, PrimitiveTextRecord::default());
            self.text_records.release(slot);
        }
    }
}
