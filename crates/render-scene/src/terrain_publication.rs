//! Session-scoped receipts for actual near terrain publications, including empty geometry.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use world::{ChunkKey, SubChunkKey};

/// A stream epoch's latest GPU publications, bounded by its retained sections.
#[derive(Clone, Default)]
pub struct TerrainPublications(Arc<Mutex<Publications>>);

#[derive(Default)]
struct Publications {
    retained: HashMap<ChunkKey, Vec<SubChunkKey>>,
    latest: HashMap<SubChunkKey, u64>,
    empty: HashSet<SubChunkKey>,
    revision: u64,
}

impl TerrainPublications {
    /// Whether two extracted handles belong to the same stream and dimension epoch.
    pub fn same_source(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Replaces changed columns, or reconciles a full snapshot when the consumer resets.
    /// Missing columns are unchanged in a delta; an empty column explicitly retires its receipts.
    pub fn retain(
        &self,
        reset: bool,
        columns: impl IntoIterator<Item = (ChunkKey, Vec<SubChunkKey>)>,
    ) {
        let mut state = self.0.lock().expect("terrain publications");
        let mut removed_empty = false;
        if reset {
            let retained: HashMap<_, _> = columns
                .into_iter()
                .map(|(column, keys)| (column, normalized_column(column, keys)))
                .filter(|(_, keys)| !keys.is_empty())
                .collect();
            state.latest.retain(|key, _| contains(&retained, key));
            let before = state.empty.len();
            state.empty.retain(|key| contains(&retained, key));
            removed_empty = state.empty.len() != before;
            state.retained = retained;
        } else {
            for (column, keys) in columns {
                let keys = normalized_column(column, keys);
                if let Some(previous) = state.retained.remove(&column) {
                    for key in previous {
                        if keys.binary_search(&key).is_err() {
                            state.latest.remove(&key);
                            removed_empty |= state.empty.remove(&key);
                        }
                    }
                }
                if !keys.is_empty() {
                    state.retained.insert(column, keys);
                }
            }
        }
        if removed_empty {
            state.revision = state.revision.wrapping_add(1);
        }
    }

    /// Records a prepared section; full geometry and withdrawals invalidate older empty proofs.
    /// Equal-generation withdrawals win over empty receipts, regardless of delivery order.
    pub fn record(&self, key: SubChunkKey, generation: u64, empty: bool) {
        let mut state = self.0.lock().expect("terrain publications");
        if generation == 0 || !contains(&state.retained, &key) {
            return;
        }
        if state.latest.get(&key).is_some_and(|current| {
            *current > generation
                || (*current == generation && (!state.empty.contains(&key) || empty))
        }) {
            return;
        }
        let before = state.empty.contains(&key).then(|| state.latest[&key]);
        state.latest.insert(key, generation);
        if empty {
            state.empty.insert(key);
        } else {
            state.empty.remove(&key);
        }
        if before != empty.then_some(generation) {
            state.revision = state.revision.wrapping_add(1);
        }
    }

    /// Copies changed empty proofs, or replays them for a new target; unchanged hits allocate nothing.
    pub fn changed_empty(&self, previous: Option<u64>) -> Option<(u64, HashMap<SubChunkKey, u64>)> {
        let state = self.0.lock().expect("terrain publications");
        (previous != Some(state.revision)).then(|| {
            (
                state.revision,
                state
                    .empty
                    .iter()
                    .map(|&key| (key, state.latest[&key]))
                    .collect(),
            )
        })
    }
}

/// Sorts a column's union once so membership and deleted-height checks need no section hash set.
fn normalized_column(column: ChunkKey, mut keys: Vec<SubChunkKey>) -> Vec<SubChunkKey> {
    assert!(keys.iter().all(|key| key.chunk() == column));
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Checks the retained column scope shared by every extracted receipt handle.
fn contains(columns: &HashMap<ChunkKey, Vec<SubChunkKey>>, key: &SubChunkKey) -> bool {
    columns
        .get(&key.chunk())
        .is_some_and(|keys| keys.binary_search(key).is_ok())
}

#[cfg(test)]
mod tests;
