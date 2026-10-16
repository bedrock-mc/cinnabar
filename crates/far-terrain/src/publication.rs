//! Chooses one terrain drawer per column from current render publications.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use world::{ChunkKey, SubChunkKey};

use crate::ColumnPos;

/// The main world's requested near sections, with exact dirty or published generations.
/// Missing columns request far terrain. Empty far columns carry decoded evidence that no far draw is needed.
#[derive(Clone, Debug)]
pub struct TerrainTarget {
    pub centre: ChunkKey,
    pub horizon: i32,
    pub near: Arc<HashMap<ChunkKey, HashMap<SubChunkKey, u64>>>,
    pub retained: Arc<HashSet<SubChunkKey>>,
    pub empty_far: Arc<HashSet<ColumnPos>>,
}

impl TerrainTarget {
    /// The current dimension and bounded mask viewport; retired views need no handoff.
    #[must_use]
    pub fn contains(&self, column: ChunkKey) -> bool {
        column.dimension == self.centre.dimension
            && (i64::from(column.x) - i64::from(self.centre.x)).abs()
                <= i64::from(self.horizon.max(0))
            && (i64::from(column.z) - i64::from(self.centre.z)).abs()
                <= i64::from(self.horizon.max(0))
    }
}

/// Render-owned admission shared by near draw lists and the far mask.
#[derive(Default)]
pub struct TerrainPublication {
    near: HashSet<ChunkKey>,
    suppressed: HashSet<ChunkKey>,
    empty: HashMap<SubChunkKey, u64>,
}

impl TerrainPublication {
    /// Forgets drawer ownership when a feature toggle changes which passes are active.
    pub fn reset_draws(&mut self) {
        self.near.clear();
        self.suppressed.clear();
    }

    /// Installs the current source's authoritative empty receipts after its revision changes.
    pub fn replace_empty(&mut self, empty: HashMap<SubChunkKey, u64>) {
        self.empty = empty;
    }

    /// A withdrawal cannot later satisfy an incoming empty-section requirement.
    pub fn withdraw(&mut self, key: SubChunkKey, generation: u64) {
        if self
            .empty
            .get(&key)
            .is_some_and(|empty| *empty <= generation)
        {
            self.empty.remove(&key);
        }
    }

    /// Records a successfully prepared zero-byte publication, before any main-world receipt.
    pub fn publish_empty(&mut self, key: SubChunkKey, generation: u64) {
        let current = self.empty.entry(key).or_default();
        *current = (*current).max(generation);
    }

    /// Publishes both drawers' admission from the same actual GPU allocation snapshot.
    /// Existing near columns keep older live meshes during ordinary replacements. Incoming
    /// columns wait for every exact section; a newer removal cannot satisfy a stale target.
    pub fn publish(
        &mut self,
        target: &TerrainTarget,
        live: &HashMap<SubChunkKey, u64>,
        mut far_available: impl FnMut(ChunkKey) -> bool,
    ) -> bool {
        self.empty.retain(|key, generation| {
            target.retained.contains(key)
                && live.get(key).is_none_or(|actual| *actual < *generation)
        });
        let columns: HashSet<_> = live
            .keys()
            .map(|key| key.chunk())
            .chain(target.near.keys().copied())
            .collect();
        let mut near = HashSet::new();
        let mut suppressed = HashSet::new();
        for column in columns {
            if !target.contains(column) {
                suppressed.insert(column);
                continue;
            }
            let requested = target.near.get(&column);
            let actual = |key: &SubChunkKey| live.get(key).or_else(|| self.empty.get(key));
            let complete = requested.is_some_and(|keys| {
                !keys.is_empty()
                    && keys.iter().all(|(key, generation)| {
                        *generation != 0 && actual(key) == Some(generation)
                    })
            });
            let retained = self.near.contains(&column)
                && requested.is_some_and(|keys| {
                    !keys.is_empty()
                        && keys.iter().all(|(key, generation)| {
                            actual(key).is_some_and(|actual| *actual != 0 && actual <= generation)
                        })
                });
            if complete || retained || !far_available(column) {
                near.insert(column);
            } else {
                suppressed.insert(column);
            }
        }
        let changed = near != self.near || suppressed != self.suppressed;
        self.near = near;
        self.suppressed = suppressed;
        changed
    }

    /// Whether this render publication admits a near allocation's draw commands.
    #[must_use]
    pub fn draws_near(&self, column: ChunkKey) -> bool {
        !self.suppressed.contains(&column)
    }

    /// Columns whose far quads must be hidden in the same frame as near admission.
    pub fn near_columns(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.near.iter().copied()
    }

    /// Far-owned columns with staged near resources that still need their far fallback.
    pub fn far_columns(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.suppressed.iter().copied()
    }
}

#[cfg(test)]
mod tests;
