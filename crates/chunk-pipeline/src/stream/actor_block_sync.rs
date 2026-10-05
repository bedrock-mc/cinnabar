use super::*;

const MAX_PENDING_ACTOR_BLOCK_SYNCS: usize = 16_384;

#[derive(Debug)]
struct PendingSync {
    generation: u64,
    order: u64,
    event: SyncedBlockUpdateEvent,
}

/// A visibility transition fenced by one accepted terrain mesh generation.
#[derive(Debug, Clone, Copy)]
pub struct ActorBlockSyncFence {
    pub key: SubChunkKey,
    pub generation: u64,
    pub sync: client_world::ingestion::ActorBlockSyncMessage,
}

#[derive(Debug, Default)]
pub(super) struct ActorBlockSyncs {
    pending: BTreeMap<SubChunkKey, Vec<PendingSync>>,
    count: usize,
    next_order: u64,
}

impl ActorBlockSyncs {
    pub(super) fn remove_columns(&mut self, columns: &BTreeSet<ChunkKey>) {
        self.pending.retain(|key, entries| {
            if columns.contains(&key.chunk()) {
                self.count -= entries.len();
                false
            } else {
                true
            }
        });
    }
}

impl WorldStream {
    /// Pending transitions retained until main-world acknowledgment consumes them.
    pub fn actor_block_sync_fences(&self) -> Vec<ActorBlockSyncFence> {
        let mut entries = self
            .actor_block_syncs
            .pending
            .iter()
            .flat_map(|(&key, entries)| {
                entries.iter().map(move |entry| {
                    (
                        entry.order,
                        ActorBlockSyncFence {
                            key,
                            generation: entry.generation,
                            sync: entry.event.sync,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|entry| entry.0);
        entries.into_iter().map(|(_, fence)| fence).collect()
    }

    pub(super) fn queue_actor_block_syncs(&mut self, events: Vec<SyncedBlockUpdateEvent>) {
        for event in events {
            if event.update.layer != 0
                || event.sync.actor_unique_id == -1
                || event.sync.message == 0
            {
                continue;
            }
            if !matches!(event.sync.message, 1 | 2) {
                self.record_normalization_error(NormalizationErrorReason::InvalidActorBlockSync);
                tracing::debug!(?event, "skipped unknown actor/terrain transition");
                continue;
            }
            let Ok((key, _)) = split_block_update(event.update) else {
                continue;
            };
            let generation = if let Some(dirty) = self.revisions.dirty(key) {
                dirty.revision
            } else if let Some(&generation) = self.applied_mesh_generations.get(&key) {
                self.deliver_actor_block_sync(key, generation, event);
                continue;
            } else {
                // A no-op still needs a mesh fence when this terrain has never been presented.
                self.sync_resident(key);
                self.mark_live_mutation_changed(key, Instant::now(), false);
                let Some(dirty) = self.revisions.dirty(key) else {
                    continue;
                };
                dirty.revision
            };
            if self.actor_block_syncs.count >= MAX_PENDING_ACTOR_BLOCK_SYNCS {
                self.record_normalization_error(NormalizationErrorReason::ActorBlockSyncCapacity);
                tracing::debug!(
                    ?event,
                    ?key,
                    generation,
                    "actor/terrain transition capacity reached"
                );
                continue;
            }
            tracing::debug!(actor_unique_id = event.sync.actor_unique_id, message = event.sync.message,
                position = ?event.update.position, flags = event.flags, ?key, generation,
                "actor/terrain transition waits for mesh upload");
            self.actor_block_syncs
                .pending
                .entry(key)
                .or_default()
                .push(PendingSync {
                    generation,
                    order: self.actor_block_syncs.next_order,
                    event,
                });
            self.actor_block_syncs.next_order = self.actor_block_syncs.next_order.saturating_add(1);
            self.actor_block_syncs.count += 1;
        }
    }

    pub(super) fn acknowledge_actor_block_syncs(&mut self, key: SubChunkKey, generation: u64) {
        let Some(mut entries) = self.actor_block_syncs.pending.remove(&key) else {
            return;
        };
        entries.retain(|entry| {
            if entry.generation <= generation {
                self.actor_block_syncs.count -= 1;
                self.deliver_actor_block_sync(key, generation, entry.event);
                false
            } else {
                true
            }
        });
        if !entries.is_empty() {
            self.actor_block_syncs.pending.insert(key, entries);
        }
    }

    fn deliver_actor_block_sync(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        event: SyncedBlockUpdateEvent,
    ) {
        let applied = self.authority.apply_actor_block_sync(event.sync);
        tracing::debug!(actor_unique_id = event.sync.actor_unique_id, message = event.sync.message,
            position = ?event.update.position, flags = event.flags, ?key, generation, applied,
            "mesh upload released actor/terrain transition");
    }
}
