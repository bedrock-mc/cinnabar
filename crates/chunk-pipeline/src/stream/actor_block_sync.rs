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
    pending: ColumnRecords<SubChunkKey, Vec<PendingSync>>,
    count: usize,
    next_order: u64,
}

impl ActorBlockSyncs {
    /// Retires only actor transitions belonging to the removed columns.
    pub(super) fn remove_columns(&mut self, columns: &BTreeSet<ChunkKey>) {
        for &column in columns {
            for entries in self.pending.remove_column(column).into_values() {
                self.count -= entries.len();
            }
        }
    }
    /// Reads and resets the number of section records examined during retirement.
    #[cfg(test)]
    fn take_cleanup_visits(&mut self) -> usize {
        self.pending.take_cleanup_visits()
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
            if !self.actor_block_syncs.pending.contains_key(&key) {
                self.actor_block_syncs.pending.insert(key, Vec::new());
            }
            self.actor_block_syncs
                .pending
                .get_mut(&key)
                .unwrap()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Populated actor fences retire by column without touching surviving section records.
    #[test]
    fn column_retirement_visits_only_its_actor_syncs() {
        let columns = 4096;
        let mut syncs = ActorBlockSyncs::default();
        let event = SyncedBlockUpdateEvent {
            update: BlockUpdateEvent {
                dimension: 0,
                position: [0; 3],
                layer: 0,
                network_id: 0,
            },
            flags: 3,
            sync: client_world::ingestion::ActorBlockSyncMessage {
                actor_unique_id: 1,
                message: 1,
            },
        };
        for x in 0..columns {
            for y in 0..2 {
                syncs.pending.insert(
                    SubChunkKey::new(0, x, y, 0),
                    vec![
                        PendingSync {
                            generation: 1,
                            order: 0,
                            event,
                        },
                        PendingSync {
                            generation: 2,
                            order: 1,
                            event,
                        },
                    ],
                );
                syncs.count += 2;
            }
        }
        let survivor = SubChunkKey::new(1, 0, 0, 0);
        syncs.pending.insert(
            survivor,
            vec![PendingSync {
                generation: 1,
                order: 0,
                event,
            }],
        );
        syncs.count += 1;
        let (mut total, mut max) = (0, 0);
        for x in 0..columns {
            syncs.remove_columns(&BTreeSet::from([ChunkKey::new(0, x, 0)]));
            let visits = syncs.take_cleanup_visits();
            total += visits;
            max = max.max(visits);
        }
        assert_eq!(syncs.count, 1);
        assert_eq!(syncs.pending.get(&survivor).unwrap().len(), 1);
        eprintln!("actor retirement: columns={columns} total_visits={total} max_visits={max}");
        assert_eq!(max, 2, "retirement visits only its own actor records");
        assert_eq!(total, columns as usize * 2);
    }
}
