//! Session-scoped receipts for actual near terrain publications, including empty geometry.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use world::SubChunkKey;

/// A stream epoch's latest GPU publications, bounded by its retained sections.
#[derive(Clone, Default)]
pub struct TerrainPublications(Arc<Mutex<Publications>>);

#[derive(Default)]
struct Publications {
    retained: HashSet<SubChunkKey>,
    latest: HashMap<SubChunkKey, u64>,
    empty: HashSet<SubChunkKey>,
    revision: u64,
}

impl TerrainPublications {
    /// Whether two extracted handles belong to the same stream and dimension epoch.
    pub fn same_source(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Reconciles source residency only when its owning indexes change.
    pub fn retain(&self, keys: impl Iterator<Item = SubChunkKey>) {
        let retained: HashSet<_> = keys.collect();
        let mut state = self.0.lock().expect("terrain publications");
        if state.retained == retained {
            return;
        }
        state.latest.retain(|key, _| retained.contains(key));
        let before = state.empty.len();
        state.empty.retain(|key| retained.contains(key));
        if state.empty.len() != before {
            state.revision = state.revision.wrapping_add(1);
        }
        state.retained = retained;
    }

    /// Applies each section's current membership, given for every section that may have
    /// changed since the last reconciliation; the result equals [`Self::retain`] over the
    /// current sections, without visiting unchanged ones.
    pub fn apply_membership(&self, changes: impl Iterator<Item = (SubChunkKey, bool)>) {
        let mut state = self.0.lock().expect("terrain publications");
        let mut withdrew_empty = false;
        for (key, retained) in changes {
            if retained {
                state.retained.insert(key);
            } else if state.retained.remove(&key) {
                // Receipts exist only for retained sections.
                state.latest.remove(&key);
                withdrew_empty |= state.empty.remove(&key);
            }
        }
        if withdrew_empty {
            state.revision = state.revision.wrapping_add(1);
        }
    }

    /// Records a prepared section; full geometry and withdrawals invalidate older empty proofs.
    /// Equal-generation withdrawals win over empty receipts, regardless of delivery order.
    pub fn record(&self, key: SubChunkKey, generation: u64, empty: bool) {
        let mut state = self.0.lock().expect("terrain publications");
        if generation == 0 || !state.retained.contains(&key) {
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

#[cfg(test)]
mod tests;
